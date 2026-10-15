//! The faults that end a process (a crash, a hang) cannot run inside the
//! test harness, so each test runs this same test binary again as a child,
//! with the fault in its environment, and looks at how the child ended.
//! The helper tests are ignored in normal runs.

mod support;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use lumen_test_plugin::faults::ENV;
use lumen_test_plugin::params::Kind;
use support::{Plugin, note_on};

/// Runs helper test `name` as a child with `fault`, and gives up on it after
/// `limit`. Returns the child's exit status, or `None` if it was killed
/// for running too long.
fn run_child(name: &str, fault: &str, limit: Duration) -> Option<std::process::ExitStatus> {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--ignored", "--nocapture"])
        .env(ENV, fault)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the test binary");
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            return Some(status);
        }
        if start.elapsed() > limit {
            child.kill().expect("kill");
            child.wait().expect("reap");
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "helper: run by the tests below with a fault set"]
fn child_process_blocks() {
    let p = Plugin::from_factory(Kind::Instrument);
    p.start();
    for _ in 0..4 {
        p.process(16, None, vec![note_on(0, 60, 1.0)], &[]);
    }
}

#[test]
#[ignore = "helper: run by the tests below with a fault set"]
fn child_factory() {
    let factory = lumen_test_plugin::plugin_factory();
    std::process::exit(if factory.is_null() { 42 } else { 0 });
}

#[test]
fn without_a_fault_the_child_finishes_cleanly() {
    let status = run_child("child_process_blocks", "", Duration::from_secs(20));
    assert!(status.is_some_and(|s| s.success()));
}

#[test]
fn a_process_crash_kills_the_child_on_the_named_block() {
    let status = run_child(
        "child_process_blocks",
        "process_crash@2",
        Duration::from_secs(20),
    );
    let status = status.expect("a crash ends the child");
    assert!(!status.success());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(status.signal().is_some(), "ended by a signal: {status:?}");
    }
}

#[test]
fn a_process_hang_never_returns() {
    let status = run_child(
        "child_process_blocks",
        "process_hang@1",
        Duration::from_millis(1500),
    );
    assert!(status.is_none(), "the child was still running");
}

#[test]
fn a_hang_after_the_last_block_is_not_reached() {
    // Four blocks are processed (calls 0 to 3): a hang at block 9 never fires.
    let status = run_child(
        "child_process_blocks",
        "process_hang@9",
        Duration::from_secs(20),
    );
    assert!(status.is_some_and(|s| s.success()));
}

#[test]
fn factory_faults_end_or_empty_the_module() {
    let status = run_child("child_factory", "factory_null", Duration::from_secs(20));
    assert_eq!(status.and_then(|s| s.code()), Some(42));
    let crashed = run_child("child_factory", "factory_crash", Duration::from_secs(20));
    assert!(crashed.is_some_and(|s| !s.success() && s.code() != Some(42)));
    assert!(run_child("child_factory", "factory_hang", Duration::from_millis(1500)).is_none());
}
