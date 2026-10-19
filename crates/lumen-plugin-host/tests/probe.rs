//! The compatibility probe, run against the test plugins: what it reports
//! for a plugin that behaves, and for each way a plugin can fail a stage.

mod support;

use std::collections::HashMap;
use std::process::Command;

use support::{FAULT, HOST, plugin};

const PROBE: &str = env!("CARGO_BIN_EXE_lumen-host-probe");
const WORDS: [&str; 6] = ["pass", "note", "fail", "crashed", "timed out", "skipped"];

struct Probed {
    code: i32,
    out: String,
}

/// Runs the probe over the test plugin with `fault` set and `extra` options.
fn probe(fault: &str, extra: &[&str]) -> Probed {
    let run = Command::new(PROBE)
        .args(["--host", HOST, "--no-editor", "--blocks", "40"])
        .args(extra)
        .arg(plugin())
        .env(FAULT, fault)
        .output()
        .expect("run the probe");
    Probed {
        code: run.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&run.stdout).into_owned(),
    }
}

/// `(verdict, detail)` of each stage of the class of `kind`.
fn stages(out: &str, kind: &str) -> HashMap<String, (String, String)> {
    let mut found = HashMap::new();
    let mut mine = false;
    for line in out.lines() {
        if !line.starts_with(' ') {
            mine = line.contains(&format!("({kind},"));
            continue;
        }
        if !mine {
            continue;
        }
        let line = line.trim_start();
        let (name, rest) = line.split_once(' ').expect("a stage line");
        let rest = rest.trim_start();
        let word = WORDS
            .iter()
            .find(|w| rest.starts_with(**w))
            .unwrap_or_else(|| panic!("no verdict in `{line}`"));
        // After the verdict: the time, then the detail.
        let detail = rest[word.len()..]
            .split_once(" ms")
            .map_or("", |(_, d)| d.trim());
        found.insert(name.to_owned(), ((*word).to_owned(), detail.to_owned()));
    }
    found
}

fn verdict(out: &str, kind: &str, stage: &str) -> String {
    stages(out, kind)
        .get(stage)
        .unwrap_or_else(|| panic!("no {stage} stage for the {kind}:\n{out}"))
        .0
        .clone()
}

#[test]
fn a_plugin_that_behaves_passes_every_stage_it_runs() {
    let found = probe("", &[]);
    assert_eq!(found.code, 0, "{}", found.out);
    for kind in ["instrument", "effect"] {
        let all = stages(&found.out, kind);
        for name in [
            "scan",
            "load",
            "buses",
            "configure",
            "process",
            "link",
            "state",
            "parameters",
            "shutdown",
        ] {
            assert_eq!(all[name].0, "pass", "{kind} {name}:\n{}", found.out);
        }
        assert_eq!(all["editor"].0, "skipped");
    }
    assert!(found.out.contains("2 of 2 classes passed every stage"));
}

#[test]
fn the_json_file_holds_a_line_for_each_class() {
    let dir = std::env::temp_dir().join(format!("lumen-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp folder");
    let file = dir.join("probe.jsonl");
    let _ = std::fs::remove_file(&file);
    let found = probe("", &["--json", file.to_str().expect("utf-8 path")]);
    assert_eq!(found.code, 0, "{}", found.out);
    let text = std::fs::read_to_string(&file).expect("the json file");
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    for line in lines {
        assert!(line.starts_with("{\"module\":"), "{line}");
        assert!(line.contains("\"compatible\":true"), "{line}");
        assert!(line.contains("{\"stage\":\"link\",\"verdict\":\"pass\""));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_table_rows_name_no_plugin() {
    let found = probe("", &["--table"]);
    assert!(
        found
            .out
            .contains("| Probe 1 | instrument | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | ✅ |"),
        "{}",
        found.out
    );
    let table = found.out.split("| Plugin |").nth(1).expect("the table");
    assert!(!table.contains("Deterministic"), "{table}");
}

#[test]
fn a_plugin_that_crashes_processing_is_a_result_and_the_rest_are_skipped() {
    let found = probe("process_crash@3", &[]);
    assert_eq!(found.code, 1, "{}", found.out);
    for kind in ["instrument", "effect"] {
        assert_eq!(verdict(&found.out, kind, "process"), "crashed");
        let (verdict_of_link, why) = stages(&found.out, kind)["link"].clone();
        assert_eq!(verdict_of_link, "skipped");
        assert!(why.contains("process crashed"), "{why}");
        assert_eq!(verdict(&found.out, kind, "shutdown"), "skipped");
        // What went before is kept.
        assert_eq!(verdict(&found.out, kind, "configure"), "pass");
    }
}

#[test]
fn a_plugin_that_hangs_is_ended_at_the_deadline() {
    let began = std::time::Instant::now();
    let found = probe("process_hang@5", &["--deadline", "2"]);
    assert_eq!(found.code, 1, "{}", found.out);
    assert_eq!(verdict(&found.out, "instrument", "process"), "timed out");
    assert_eq!(verdict(&found.out, "instrument", "state"), "skipped");
    assert!(
        began.elapsed() < std::time::Duration::from_secs(30),
        "the run was not held up: {:?}",
        began.elapsed()
    );
}

#[test]
fn output_that_is_not_finite_fails_the_process_stage_and_not_the_run() {
    let found = probe("process_nan@6", &[]);
    assert_eq!(found.code, 1, "{}", found.out);
    let process = &stages(&found.out, "instrument")["process"];
    assert_eq!(process.0, "fail", "{}", found.out);
    assert!(process.1.contains("not finite"), "{}", process.1);
    // The plugin did not die, so the later stages still ran.
    assert_ne!(verdict(&found.out, "instrument", "state"), "skipped");
}

#[test]
fn a_plugin_that_cannot_save_its_state_fails_that_stage_only() {
    let found = probe("state_fail", &[]);
    assert_eq!(found.code, 1, "{}", found.out);
    assert_eq!(verdict(&found.out, "instrument", "state"), "fail");
    assert_eq!(verdict(&found.out, "instrument", "process"), "pass");
    assert_eq!(verdict(&found.out, "instrument", "parameters"), "pass");
    assert_eq!(verdict(&found.out, "instrument", "shutdown"), "pass");
}

#[test]
fn a_module_that_crashes_when_scanned_is_one_result() {
    let found = probe("factory_crash", &[]);
    assert_eq!(found.code, 1, "{}", found.out);
    assert!(found.out.contains("scan        crashed"), "{}", found.out);
    assert!(found.out.contains("0 of 1 classes passed every stage"));
    assert!(found.out.contains("load        skipped"));
}

#[test]
fn a_bad_option_is_refused_with_the_help() {
    let run = Command::new(PROBE)
        .args(["--blocks"])
        .output()
        .expect("run the probe");
    assert_eq!(run.status.code(), Some(2));
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(
        said.contains("needs a number") && said.contains("Usage:"),
        "{said}"
    );
}
