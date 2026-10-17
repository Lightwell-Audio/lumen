//! Shared by the integration tests: the built test plugin and host program,
//! and the waiting every test does.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use lumen_core::midi::{Event, Msg};
use lumen_devices::Hosted;
use lumen_hosting::{Happened, Linked, Manager, Start};

pub const FAULT: &str = lumen_test_plugin::faults::ENV;
pub const RATE: f64 = 48_000.0;

/// The test plugin's module and the host program, built once.
pub fn built() -> &'static (PathBuf, PathBuf) {
    static BUILT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    BUILT.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = Command::new(cargo);
        build.args([
            "build",
            "-p",
            "lumen-test-plugin",
            "-p",
            "lumen-plugin-host",
        ]);
        if !cfg!(debug_assertions) {
            build.arg("--release");
        }
        assert!(build.status().expect("run cargo").success(), "build");
        // target/<profile>/deps/<this test> -> target/<profile>
        let exe = std::env::current_exe().expect("test binary");
        let profile = exe.parent().and_then(Path::parent).expect("profile dir");
        let library = if cfg!(windows) {
            "lumen_test_plugin.dll"
        } else if cfg!(target_os = "macos") {
            "liblumen_test_plugin.dylib"
        } else {
            "liblumen_test_plugin.so"
        };
        let host = if cfg!(windows) {
            "lumen-plugin-host.exe"
        } else {
            "lumen-plugin-host"
        };
        let (library, host) = (profile.join(library), profile.join(host));
        assert!(
            library.is_file() && host.is_file(),
            "artifacts were not built"
        );
        (library, host)
    })
}

pub fn instrument_class() -> [u8; 16] {
    lumen_test_plugin::INSTRUMENT.map(|c| c as u8)
}

pub fn effect_class() -> [u8; 16] {
    lumen_test_plugin::EFFECT.map(|c| c as u8)
}

/// The test instrument, with `fault` switched on in its host.
pub fn instrument(fault: &str) -> Start {
    let mut start = Start::new(
        built().1.clone(),
        built().0.clone(),
        instrument_class(),
        true,
        RATE,
    );
    start.env.push((FAULT.into(), fault.into()));
    start
}

/// The test effect, with `fault` switched on in its host.
pub fn effect(fault: &str) -> Start {
    let mut start = Start::new(
        built().1.clone(),
        built().0.clone(),
        effect_class(),
        false,
        RATE,
    );
    start.env.push((FAULT.into(), fault.into()));
    start
}

/// Polls `manager` until `pick` accepts an event, or fails after `limit`.
pub fn wait_for<T>(
    manager: &mut Manager,
    limit: Duration,
    mut pick: impl FnMut(u64, Happened) -> Result<T, Happened>,
) -> T {
    let end = Instant::now() + limit;
    loop {
        for event in manager.poll() {
            if let Ok(found) = pick(event.device, event.happened) {
                return found;
            }
        }
        assert!(Instant::now() < end, "nothing happened within {limit:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The next link `device` gets.
pub fn linked(manager: &mut Manager, device: u64) -> Box<Linked> {
    wait_for(
        manager,
        Duration::from_secs(30),
        |id, happened| match happened {
            Happened::Linked(l) if id == device => Ok(l),
            Happened::Failed(f) if id == device => panic!("device {device} failed: {f}"),
            other => Err(other),
        },
    )
}

/// The first link each of `devices` gets.
pub fn linked_all(manager: &mut Manager, devices: &[u64]) -> Vec<Linked> {
    let mut found: Vec<Option<Linked>> = devices.iter().map(|_| None).collect();
    let end = Instant::now() + Duration::from_secs(60);
    while found.iter().any(Option::is_none) {
        for event in manager.poll() {
            let at = devices
                .iter()
                .position(|&d| d == event.device)
                .expect("a device");
            match event.happened {
                Happened::Linked(l) => found[at] = Some(*l),
                Happened::Failed(f) => panic!("device {} failed: {f}", event.device),
                Happened::Edits(_)
                | Happened::Editor(_)
                | Happened::Text { .. }
                | Happened::Typed { .. }
                | Happened::State(_)
                | Happened::Key { .. } => {}
            }
        }
        assert!(Instant::now() < end, "devices did not all link");
        std::thread::sleep(Duration::from_millis(5));
    }
    found.into_iter().flatten().collect()
}

/// How the next thing to happen to `device` went wrong.
pub fn failure(manager: &mut Manager, device: u64) -> lumen_hosting::Failure {
    wait_for(
        manager,
        Duration::from_secs(30),
        |id, happened| match happened {
            Happened::Failed(f) if id == device => Ok(f),
            Happened::Linked(l) if id == device => panic!("device {device} linked: {l:?}"),
            other => Err(other),
        },
    )
}

pub fn note_on(offset: u32, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_on(0, key, 127),
    }
}

pub fn note_off(offset: u32, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_off(0, key),
    }
}

/// One block through a host: `(left, right, reply)`.
pub fn play(
    host: &mut dyn Hosted,
    frames: usize,
    input: &[&[f32]],
    events: &[Event],
) -> (Vec<f32>, Vec<f32>, Option<lumen_devices::Reply>) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    if host.submit(frames, input, events).is_err() {
        return (left, right, None);
    }
    let reply = host.collect(
        Instant::now() + Duration::from_secs(5),
        &mut left,
        &mut right,
    );
    (left, right, Some(reply))
}

/// Whether a process with this id is still running.
pub fn exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .expect("run tasklist");
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }
    #[cfg(target_os = "linux")]
    {
        // A zombie still has an entry; it has stopped running.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat.contains(") Z "),
            Err(_) => false,
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Waits until `pid` has gone, up to `limit`.
pub fn gone_within(pid: u32, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if !exists(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !exists(pid)
}
