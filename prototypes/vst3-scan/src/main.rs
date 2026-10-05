//! P-VST3 prototype: scan real VST3 modules without ever risking the app.
//!
//! ```text
//! vst3-scan scan [--jobs N] [--timeout S] [--fast] [--json FILE] [ROOT...]
//! vst3-scan selftest        crash / hang / no-factory test modules must be isolated
//! ```
//! `scan` enumerates every module in a child process; with `--fast` it uses
//! `moduleinfo.json` instead where one exists.

mod discover;
#[cfg(windows)]
mod factory;
mod report;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use discover::{Layout, Module};
use report::FactoryReport;

#[derive(Debug, Clone, Serialize)]
enum ScanResult {
    Enumerated(FactoryReport),
    FromModuleInfo(FactoryReport),
    Failed(String),
    TimedOut,
    Crashed(Option<i32>),
    NoBinary,
}

#[derive(Debug, Clone, Serialize)]
struct Outcome {
    module: Module,
    moduleinfo: Option<Result<FactoryReport, String>>,
    result: ScanResult,
    wall_ms: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match args.first().map(String::as_str) {
        #[cfg(windows)]
        Some("scan-one") => {
            let binary = PathBuf::from(args.get(1).expect("binary path"));
            let result = factory::enumerate(&binary);
            println!("{}", serde_json::to_string(&result).expect("serializable"));
        }
        Some("selftest") => std::process::exit(selftest()),
        None | Some("scan") => {
            let jobs = value("--jobs").and_then(|v| v.parse().ok()).unwrap_or(4);
            let timeout = Duration::from_secs(
                value("--timeout")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
            );
            let fast = args.iter().any(|a| a == "--fast");
            let roots: Vec<PathBuf> = args
                .iter()
                .skip(1)
                .enumerate()
                .filter(|(i, a)| !a.starts_with("--") && !args[*i].starts_with("--"))
                .map(|(_, a)| PathBuf::from(a))
                .collect();
            let roots = if roots.is_empty() {
                discover::default_roots()
            } else {
                roots
            };
            let outcomes = scan(&roots, jobs, timeout, fast);
            print_table(&roots, &outcomes);
            if let Some(file) = value("--json") {
                std::fs::write(
                    &file,
                    serde_json::to_string_pretty(&outcomes).expect("serializable"),
                )
                .expect("write json");
                println!("\nwrote {file}");
            }
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; use scan or selftest");
            std::process::exit(2);
        }
    }
}

fn scan(roots: &[PathBuf], jobs: usize, timeout: Duration, fast: bool) -> Vec<Outcome> {
    let started = Instant::now();
    let modules = discover::discover(roots);
    println!(
        "{} modules under {} root(s); {} parallel scan processes, timeout {:?}, {}\n",
        modules.len(),
        roots.len(),
        jobs,
        timeout,
        if fast {
            "moduleinfo.json fast path ON"
        } else {
            "every module enumerated out of process"
        }
    );
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(modules.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(module) = modules.get(index) else {
                        break;
                    };
                    let outcome = scan_module(module, timeout, fast);
                    results.lock().expect("results lock").push((index, outcome));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("results lock");
    results.sort_by_key(|(index, _)| *index);
    println!("total wall time: {:.2} s", started.elapsed().as_secs_f64());
    results.into_iter().map(|(_, outcome)| outcome).collect()
}

fn scan_module(module: &Module, timeout: Duration, fast: bool) -> Outcome {
    let started = Instant::now();
    let moduleinfo = module.moduleinfo.as_ref().map(|path| {
        std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| report::parse_moduleinfo(&t))
    });
    let result = match (&moduleinfo, &module.binary) {
        (Some(Ok(info)), _) if fast => ScanResult::FromModuleInfo(info.clone()),
        (_, None) => ScanResult::NoBinary,
        (_, Some(binary)) => run_sandboxed(binary, timeout),
    };
    Outcome {
        module: module.clone(),
        moduleinfo,
        result,
        wall_ms: started.elapsed().as_secs_f64() * 1e3,
    }
}

/// Runs `scan-one` in a child process with a deadline.
fn run_sandboxed(binary: &Path, timeout: Duration) -> ScanResult {
    let exe = std::env::current_exe().expect("own executable path");
    let spawned = Command::new(exe)
        .arg("scan-one")
        .arg(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return ScanResult::Failed(format!("spawn failed: {e}")),
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let Some(status) = status else {
        return ScanResult::TimedOut;
    };
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        return ScanResult::Crashed(status.code());
    }
    // Plugins sometimes print to stdout themselves; our JSON is the last line.
    let last_line = text
        .lines()
        .rev()
        .find(|l| l.starts_with('{'))
        .unwrap_or("");
    match serde_json::from_str::<Result<FactoryReport, String>>(last_line) {
        Ok(Ok(report)) => ScanResult::Enumerated(report),
        Ok(Err(message)) => ScanResult::Failed(message),
        Err(e) => ScanResult::Failed(format!("unreadable child output ({e})")),
    }
}

fn print_table(roots: &[PathBuf], outcomes: &[Outcome]) {
    let short = |path: &Path| {
        roots
            .iter()
            .find_map(|r| path.strip_prefix(r).ok())
            .unwrap_or(path)
            .display()
            .to_string()
    };
    println!(
        "\n{:<44} {:<6} {:<10} {:>8}  result",
        "module", "layout", "moduleinfo", "ms"
    );
    let (mut ok, mut failed, mut timeouts, mut crashes, mut agree, mut compared) =
        (0, 0, 0, 0, 0, 0);
    for o in outcomes {
        let layout = if o.module.layout == Layout::Bundle {
            "bundle"
        } else {
            "file"
        };
        let info = match &o.moduleinfo {
            None => "-".to_string(),
            Some(Ok(_)) => "yes".to_string(),
            Some(Err(_)) => "BAD".to_string(),
        };
        let result = match &o.result {
            ScanResult::Enumerated(r) | ScanResult::FromModuleInfo(r) => {
                ok += 1;
                let audio: Vec<String> = r
                    .audio_classes()
                    .map(|c| format!("{} [{}]", c.name, c.sub_categories))
                    .collect();
                if let (ScanResult::Enumerated(r), Some(Ok(mi))) = (&o.result, &o.moduleinfo) {
                    compared += 1;
                    let names = |rep: &FactoryReport| {
                        let mut v: Vec<(String, String)> = rep
                            .classes
                            .iter()
                            .map(|c| (c.name.clone(), c.cid.clone()))
                            .collect();
                        v.sort();
                        v
                    };
                    if names(r) == names(mi) {
                        agree += 1;
                    }
                }
                let source = if matches!(o.result, ScanResult::FromModuleInfo(_)) {
                    "moduleinfo"
                } else {
                    "factory"
                };
                format!(
                    "OK via {source}: {} ({} audio class(es)) {} load {:.0} ms",
                    r.vendor,
                    audio.len(),
                    audio.join(", "),
                    r.load_ms
                )
            }
            ScanResult::Failed(m) => {
                failed += 1;
                format!("FAILED: {m}")
            }
            ScanResult::TimedOut => {
                timeouts += 1;
                "TIMED OUT: scan process killed".to_string()
            }
            ScanResult::Crashed(code) => {
                crashes += 1;
                format!(
                    "CRASHED: exit code {}",
                    code.map(|c| format!("0x{:08X}", c as u32))
                        .unwrap_or("?".into())
                )
            }
            ScanResult::NoBinary => {
                failed += 1;
                "no x86_64-win binary in bundle".to_string()
            }
        };
        println!(
            "{:<44} {:<6} {:<10} {:>8.0}  {}",
            short(&o.module.path),
            layout,
            info,
            o.wall_ms,
            result
        );
    }
    let mut slowest: Vec<&Outcome> = outcomes.iter().collect();
    slowest.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
    let sum: f64 = outcomes.iter().map(|o| o.wall_ms).sum();
    println!(
        "\nsummary: {} modules: {ok} ok, {failed} failed, {timeouts} timed out, {crashes} crashed",
        outcomes.len()
    );
    println!(
        "moduleinfo.json present for {} modules; factory and moduleinfo agree (names and class ids) on {agree}/{compared} compared",
        outcomes.iter().filter(|o| o.moduleinfo.is_some()).count()
    );
    println!(
        "sum of per-module times: {:.2} s; slowest: {}",
        sum / 1e3,
        slowest
            .iter()
            .take(5)
            .map(|o| format!("{} {:.0} ms", short(&o.module.path), o.wall_ms))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Copies the misbehaving test module under telling names and checks that
/// each failure is contained and classified.
fn selftest() -> i32 {
    let exe = std::env::current_exe().expect("own executable path");
    let dll = exe.with_file_name("vst3_test_module.dll");
    if !dll.is_file() {
        eprintln!("build the test module first: cargo build --release -p vst3-test-module");
        return 2;
    }
    let dir = std::env::temp_dir().join(format!("lumen-vst3-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for name in ["LumenCrash.vst3", "LumenHang.vst3", "LumenNoFactory.vst3"] {
        std::fs::copy(&dll, dir.join(name)).expect("copy test module");
    }
    let outcomes = scan(std::slice::from_ref(&dir), 3, Duration::from_secs(5), false);
    print_table(std::slice::from_ref(&dir), &outcomes);
    let verdict = |name: &str| {
        outcomes
            .iter()
            .find(|o| o.module.path.ends_with(name))
            .map(|o| &o.result)
    };
    let checks = [
        (
            "crash contained",
            matches!(verdict("LumenCrash.vst3"), Some(ScanResult::Crashed(Some(c))) if *c as u32 == 0xC000_0005),
        ),
        (
            "hang contained",
            matches!(verdict("LumenHang.vst3"), Some(ScanResult::TimedOut)),
        ),
        (
            "missing factory reported",
            matches!(verdict("LumenNoFactory.vst3"), Some(ScanResult::Failed(m)) if m.contains("returned null")),
        ),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    println!();
    for (name, passed) in &checks {
        println!("{} {name}", if *passed { "PASS" } else { "FAIL" });
    }
    i32::from(!checks.iter().all(|(_, passed)| *passed))
}
