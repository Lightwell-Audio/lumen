use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::lifecycle::join_within;
use super::log::host_line;
use super::*;

/// The host shares its stderr with the plugin, so a line is only the host's
/// when it carries the nonce this process chose: one that merely looks like a
/// host line is a plugin's, and is dropped.
#[test]
fn only_a_line_with_this_hosts_nonce_is_the_hosts() {
    let mark = crate::process::mark("4f2a");
    assert_eq!(
        host_line("lumen-host[4f2a]: crash in a plugin\n", &mark),
        Some("crash in a plugin")
    );
    assert_eq!(host_line("lumen-host: crash in a plugin", &mark), None);
    assert_eq!(
        host_line("lumen-host[9c1d]: crash in a plugin", &mark),
        None
    );
    assert_eq!(
        host_line("a plugin wrote lumen-host[4f2a]: crash", &mark),
        None
    );
    assert_eq!(host_line("", &mark), None);
}

#[test]
fn a_thread_that_does_not_end_is_let_go_not_waited_for() {
    let (stop, wait) = mpsc::channel::<()>();
    let stuck = std::thread::spawn(move || {
        let _ = wait.recv();
    });
    let quick = std::thread::spawn(|| {});
    let started = Instant::now();
    join_within([quick, stuck].into_iter(), Duration::from_millis(100));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "dropping waited for a reader that never ends"
    );
    drop(stop);
}

#[test]
fn a_crash_is_worded_with_its_code_and_what_the_code_means() {
    let words = ExitKind::Crashed("exception 0xc0000005".into()).to_string();
    assert!(words.contains("access violation"), "{words}");
    assert!(words.contains("0xc0000005"), "the code stays: {words}");
    let unknown = ExitKind::Crashed("exception 0xc0001234".into()).to_string();
    assert_eq!(unknown, "it crashed: exception 0xc0001234");
    assert_eq!(
        ExitKind::Crashed("signal 11".into()).to_string(),
        "it crashed: signal 11"
    );
    assert_eq!(ExitKind::Failed(3).to_string(), "it exited with code 3");
}
