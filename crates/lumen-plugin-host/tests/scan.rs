//! Scanning a module in a child process: what a good module reports and
//! every way a bad one can end, none of which may reach the test process.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lumen_plugin_host::scan::{ScanOutcome, run_scan};
use support::{FAULT, HOST, plugin};

const DEADLINE: Duration = Duration::from_secs(20);

fn scan(fault: &str, deadline: Duration) -> ScanOutcome {
    run_scan(
        Path::new(HOST),
        plugin(),
        deadline,
        &[(FAULT, fault)],
        &AtomicBool::new(false),
    )
}

#[test]
fn a_good_module_reports_its_classes() {
    let ScanOutcome::Scanned(report) = scan("", DEADLINE) else {
        panic!("a good module scans");
    };
    let names: Vec<&str> = report.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"Deterministic Test Instrument"),
        "{names:?}"
    );
    assert!(names.contains(&"Deterministic Test Effect"), "{names:?}");
    let audio: Vec<_> = report
        .classes
        .iter()
        .filter(|c| c.category == lumen_plugin_host::module::AUDIO_CLASS)
        .collect();
    assert_eq!(audio.len(), 2, "an instrument and an effect");
    assert!(
        audio.iter().any(|c| c.subcategories.contains("Instrument")),
        "{audio:?}"
    );
    assert!(audio.iter().any(|c| c.subcategories.contains("Fx")));
    assert!(!report.vendor.is_empty());
}

#[test]
fn a_factory_that_returns_nothing_is_a_failure_with_a_message() {
    let ScanOutcome::Failed(message) = scan("factory_null", DEADLINE) else {
        panic!("a failure");
    };
    assert!(message.contains("factory"), "{message}");
}

#[test]
fn a_factory_that_crashes_is_a_crash_not_ours() {
    let ScanOutcome::Crashed { how, .. } = scan("factory_crash", DEADLINE) else {
        panic!("a crash");
    };
    assert!(how.contains("signal") || how.contains("exception"), "{how}");
}

#[test]
fn a_factory_that_hangs_is_killed_at_the_deadline() {
    let started = Instant::now();
    let outcome = scan("factory_hang", Duration::from_millis(500));
    assert_eq!(outcome, ScanOutcome::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn cancel_ends_a_hung_scan_quickly() {
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let outcome = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            run_scan(
                Path::new(HOST),
                plugin(),
                DEADLINE,
                &[(FAULT, "factory_hang")],
                &cancel,
            )
        });
        std::thread::sleep(Duration::from_millis(300));
        cancel.store(true, Ordering::Relaxed);
        handle.join().unwrap()
    });
    assert_eq!(outcome, ScanOutcome::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_missing_module_and_a_file_that_is_not_a_module_fail_cleanly() {
    let missing = run_scan(
        Path::new(HOST),
        Path::new("/definitely/not/here.vst3"),
        DEADLINE,
        &[],
        &AtomicBool::new(false),
    );
    assert!(matches!(missing, ScanOutcome::Failed(_)), "{missing:?}");

    let dir = std::env::temp_dir().join(format!("lumen-scan-notamodule-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Fake.vst3");
    std::fs::write(&file, b"not a library").unwrap();
    let outcome = run_scan(
        Path::new(HOST),
        &file,
        DEADLINE,
        &[],
        &AtomicBool::new(false),
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matches!(outcome, ScanOutcome::Failed(_)), "{outcome:?}");
}

#[test]
fn a_host_binary_that_is_missing_is_unstarted() {
    let outcome = run_scan(
        Path::new("/definitely/not/a/host"),
        plugin(),
        DEADLINE,
        &[],
        &AtomicBool::new(false),
    );
    assert!(matches!(outcome, ScanOutcome::Unstarted(_)), "{outcome:?}");
}

#[cfg(unix)]
#[test]
fn a_child_that_ends_without_a_reply_is_malformed() {
    // `true` exits cleanly and says nothing.
    let outcome = run_scan(
        Path::new("/bin/true"),
        plugin(),
        DEADLINE,
        &[],
        &AtomicBool::new(false),
    );
    assert!(matches!(outcome, ScanOutcome::Malformed(_)), "{outcome:?}");
}

#[cfg(unix)]
#[test]
fn a_child_that_exits_with_a_failure_code_is_reported_with_the_code() {
    let outcome = run_scan(
        Path::new("/bin/false"),
        plugin(),
        DEADLINE,
        &[],
        &AtomicBool::new(false),
    );
    let ScanOutcome::Crashed { how, .. } = outcome else {
        panic!("a failure code is a crash");
    };
    assert!(how.contains("exit code 1"), "{how}");
}

#[test]
fn scans_do_not_disturb_each_other_when_run_side_by_side() {
    let good = std::thread::spawn(|| scan("", DEADLINE));
    let crash = std::thread::spawn(|| scan("factory_crash", DEADLINE));
    let hang = std::thread::spawn(|| scan("factory_hang", Duration::from_millis(400)));
    assert!(matches!(good.join().unwrap(), ScanOutcome::Scanned(_)));
    assert!(matches!(crash.join().unwrap(), ScanOutcome::Crashed { .. }));
    assert_eq!(hang.join().unwrap(), ScanOutcome::TimedOut);
}
