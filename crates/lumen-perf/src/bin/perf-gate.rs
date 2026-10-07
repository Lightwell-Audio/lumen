//! Runs every scenario under Callgrind and compares instruction counts with
//! `perf/baseline.json` (ADR-0022).
//!
//! ```text
//! cargo build --release -p lumen-perf
//! target/release/perf-gate            # compare; exit 1 on a regression
//! target/release/perf-gate --update   # write the current counts as the baseline
//! ```
//!
//! Linux only (Valgrind). Counts depend on the toolchain, so the baseline is
//! regenerated when `rust-toolchain.toml` or a dependency changes the code
//! being measured; a change above the threshold says so in the failure.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

const BASELINE: &str = "perf/baseline.json";
/// Default allowed growth, in percent. Counts are deterministic to well
/// under 1% (hash-table seeds are the only source of variation), so this
/// catches real regressions without flapping.
const DEFAULT_THRESHOLD: f64 = 3.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let update = args.iter().any(|a| a == "--update");
    let only = args
        .iter()
        .position(|a| a == "--only")
        .and_then(|i| args.get(i + 1));

    let root = workspace_root();
    let baseline_path = root.join(BASELINE);
    let baseline: Value = std::fs::read_to_string(&baseline_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let threshold = baseline["threshold_percent"]
        .as_f64()
        .unwrap_or(DEFAULT_THRESHOLD);

    let scenario_exe = sibling("perf-scenario");
    let mut measured = Map::new();
    let mut rows = Vec::new();
    let mut failed = false;
    for (name, what) in lumen_perf::SCENARIOS {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let count = match callgrind(&scenario_exe, name) {
            Ok(count) => count,
            Err(e) => {
                eprintln!("{name}: {e}");
                std::process::exit(2);
            }
        };
        measured.insert((*name).to_owned(), json!(count));
        let before = baseline["scenarios"][name].as_u64();
        let verdict = match before {
            None => {
                failed |= !update;
                "no baseline".to_owned()
            }
            Some(before) => {
                let change = (count as f64 - before as f64) / before as f64 * 100.0;
                if change > threshold {
                    failed |= !update;
                    format!("{change:+.2}% REGRESSION")
                } else if change < -threshold {
                    format!("{change:+.2}% faster: update the baseline")
                } else {
                    format!("{change:+.2}%")
                }
            }
        };
        rows.push(format!(
            "| {name} | {} | {count} | {verdict} | {what} |",
            before.map_or("—".into(), |b| b.to_string())
        ));
    }

    let table = format!(
        "### Performance gate (instructions, threshold +{threshold}%)\n\n\
         | Scenario | Baseline | Now | Change | Measures |\n|---|---|---|---|---|\n{}\n",
        rows.join("\n")
    );
    println!("{table}");
    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(summary) {
            let _ = writeln!(file, "{table}");
        }
    }
    // Always print the counts as JSON, so a CI log is enough to refresh
    // the baseline after a toolchain change.
    println!("current counts: {}", Value::Object(measured.clone()));

    if update {
        let mut scenarios = baseline["scenarios"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        scenarios.extend(measured);
        let out = json!({
            "about": "Instruction counts per scenario under Callgrind (ADR-0022). \
                      Regenerate with `perf-gate --update` on Linux.",
            "threshold_percent": threshold,
            "scenarios": scenarios,
        });
        let text = serde_json::to_string_pretty(&out).expect("JSON") + "\n";
        if let Some(dir) = baseline_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&baseline_path, text) {
            eprintln!("could not write {}: {e}", baseline_path.display());
            std::process::exit(2);
        }
        println!("baseline written to {}", baseline_path.display());
    } else if failed {
        eprintln!(
            "performance gate failed. If the change is intended (or the toolchain \
             changed), run `perf-gate --update` on Linux and commit {BASELINE}."
        );
        std::process::exit(1);
    }
}

/// Instructions executed inside `measured_<name>` and everything it calls.
fn callgrind(exe: &Path, name: &str) -> Result<u64, String> {
    let out = std::env::temp_dir().join(format!("lumen-callgrind-{name}.out"));
    let status = Command::new("valgrind")
        .arg("--tool=callgrind")
        .arg(format!("--toggle-collect=measured_{name}"))
        .arg(format!("--callgrind-out-file={}", out.display()))
        .arg("--quiet")
        .arg(exe)
        .arg(name)
        .status()
        .map_err(|e| format!("could not start valgrind ({e}); is it installed?"))?;
    if !status.success() {
        return Err(format!("scenario failed under valgrind: {status}"));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("no callgrind output: {e}"))?;
    let _ = std::fs::remove_file(&out);
    let count = text
        .lines()
        .find_map(|l| {
            l.strip_prefix("totals:")
                .or_else(|| l.strip_prefix("summary:"))
        })
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .ok_or("callgrind output has no totals line")?;
    if count == 0 {
        return Err(format!(
            "0 instructions counted: measured_{name} was not found (inlined or renamed?)"
        ));
    }
    Ok(count)
}

fn sibling(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("own path");
    exe.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/lumen-perf sits two levels below the root")
        .to_path_buf()
}
