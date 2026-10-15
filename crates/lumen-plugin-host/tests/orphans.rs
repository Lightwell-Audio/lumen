//! A host must not outlive the Lumen that started it, even when Lumen is
//! killed outright and the host is hung inside a plugin.
#![cfg(target_os = "linux")]

mod support;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use support::{SLOW, exists, instrument, note_on};

/// The stand-in for Lumen: starts a host that hangs in `process`, prints the
/// host's id and then waits to be killed.
#[test]
#[ignore = "helper: run as a child by the test below"]
fn stand_in_for_lumen() {
    let mut host = instrument("process_hang@0");
    println!("host {}", host.id());
    let _ = host.process(
        32,
        vec![],
        vec![note_on(0, 60)],
        vec![],
        Duration::from_millis(100),
    );
    std::thread::sleep(SLOW * 6);
}

#[test]
fn killing_lumen_takes_a_hung_host_with_it() {
    let mut lumen = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stand_in_for_lumen", "--ignored", "--nocapture"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(lumen.stdout.take().unwrap()).lines();
    let host: u32 = loop {
        let line = lines.next().expect("the helper prints the host").unwrap();
        if let Some(id) = line.strip_prefix("host ") {
            break id.trim().parse().unwrap();
        }
    };
    // Give the helper time to send the block that hangs the host.
    std::thread::sleep(Duration::from_millis(400));
    assert!(exists(host), "the host is running (and hung)");

    lumen.kill().unwrap();
    lumen.wait().unwrap();
    let start = Instant::now();
    while exists(host) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "host {host} outlived Lumen"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
