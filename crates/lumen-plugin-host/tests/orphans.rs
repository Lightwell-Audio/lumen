#![cfg(any(target_os = "linux", target_os = "macos"))]

mod support;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use support::{SLOW, exists, instrument, note_on};

fn kill_process(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

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
    std::thread::sleep(Duration::from_millis(400));
    assert!(exists(host), "the host is running (and hung)");

    lumen.kill().unwrap();
    lumen.wait().unwrap();
    let start = Instant::now();
    while exists(host) {
        if start.elapsed() >= Duration::from_secs(5) {
            kill_process(host);
            panic!("host {host} outlived Lumen");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "helper: run as a child by the race test below"]
fn wrapper_exits_with_a_hung_host() {
    let mut host = instrument("process_hang@0");
    let id = host.id();
    println!("host {id}");
    let _ = host.process(
        32,
        vec![],
        vec![note_on(0, 60)],
        vec![],
        Duration::from_millis(100),
    );
    std::mem::forget(host);
}

#[test]
fn an_already_exited_parent_does_not_leave_a_hung_host() {
    let mut wrapper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "wrapper_exits_with_a_hung_host",
            "--ignored",
            "--nocapture",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(wrapper.stdout.take().unwrap()).lines();
    let host: u32 = loop {
        let line = lines.next().expect("the helper prints the host").unwrap();
        if let Some(id) = line.strip_prefix("host ") {
            break id.trim().parse().unwrap();
        }
    };
    wrapper.wait().unwrap();
    let start = Instant::now();
    while exists(host) {
        if start.elapsed() >= Duration::from_secs(5) {
            kill_process(host);
            panic!("host {host} outlived its already-exited parent");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "helper: run as a child by the race test below"]
fn child_exits_if_its_parent_is_already_gone() {
    lumen_plugin_host::process::die_with_parent();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "helper: creates a short-lived parent for the race test below"]
fn parent_exits_immediately_after_starting_watcher_child() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "child_exits_if_its_parent_is_already_gone",
        "--ignored",
        "--nocapture",
    ]);
    lumen_plugin_host::process::started_by_this_process(&mut command);
    let child = command.spawn().unwrap();
    println!("watcher child {}", child.id());
    let mut stdout = std::io::stdout();
    std::io::Write::flush(&mut stdout).unwrap();
    std::mem::forget(child);
}

#[test]
fn parent_exit_during_child_startup_is_detected() {
    let mut wrapper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "parent_exits_immediately_after_starting_watcher_child",
            "--ignored",
            "--nocapture",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(wrapper.stdout.take().unwrap()).lines();
    let child: u32 = loop {
        let line = lines
            .next()
            .expect("the wrapper prints the child id")
            .unwrap();
        if let Some(id) = line.strip_prefix("watcher child ") {
            break id.trim().parse().unwrap();
        }
    };
    wrapper.wait().unwrap();
    let start = Instant::now();
    while exists(child) {
        if start.elapsed() >= Duration::from_secs(5) {
            kill_process(child);
            panic!("watcher child {child} outlived its exited parent");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
