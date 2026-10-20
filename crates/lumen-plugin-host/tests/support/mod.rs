//! Shared by the integration tests: the built test plugin and the host binary.
#![allow(dead_code, reason = "each test binary uses some of these helpers")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use lumen_plugin_host::client::HostProcess;
use lumen_plugin_host::protocol::{Event, EventKind};

pub const HOST: &str = env!("CARGO_BIN_EXE_lumen-plugin-host");
pub const FAULT: &str = lumen_test_plugin::faults::ENV;
pub const SLOW: Duration = Duration::from_secs(10);

/// The test plugin's shared library, built once for the whole test run.
pub fn plugin() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = Command::new(cargo);
        build.args(["build", "-p", "lumen-test-plugin"]);
        if !cfg!(debug_assertions) {
            build.arg("--release");
        }
        assert!(
            build.status().expect("run cargo").success(),
            "build the test plugin"
        );
        // target/<profile>/deps/<this test> -> target/<profile>
        let exe = std::env::current_exe().expect("test binary");
        let profile = exe.parent().and_then(Path::parent).expect("profile dir");
        let name = if cfg!(windows) {
            "lumen_test_plugin.dll"
        } else if cfg!(target_os = "macos") {
            "liblumen_test_plugin.dylib"
        } else {
            "liblumen_test_plugin.so"
        };
        let library = profile.join(name);
        assert!(library.is_file(), "{} was not built", library.display());
        #[cfg(target_os = "macos")]
        {
            let bundle = std::env::temp_dir().join(format!(
                "lumen-host-test-plugin-{}.vst3",
                std::process::id()
            ));
            let contents = bundle.join("Contents");
            let executable = contents.join("MacOS").join("lumen_test_plugin");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::copy(&library, &executable).unwrap();
            let info = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>lumen_test_plugin</string><key>CFBundleIdentifier</key><string>org.lumen.test-plugin</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>";
            std::fs::write(contents.join("Info.plist"), info).unwrap();
            bundle
        }
        #[cfg(not(target_os = "macos"))]
        {
            library
        }
    })
}

pub fn spawn(fault: &str) -> HostProcess {
    HostProcess::spawn(Path::new(HOST), &[(FAULT, fault)]).expect("start the host")
}

/// A host with the instrument (class 0) loaded and configured: 48 kHz, 512
/// frames, no input, stereo output.
pub fn instrument(fault: &str) -> HostProcess {
    let mut host = spawn(fault);
    host.load(plugin(), None, SLOW).expect("load");
    host.configure(48_000.0, 512, 0, 2, SLOW)
        .expect("configure");
    host
}

/// A host with the effect loaded and configured: stereo in and out.
pub fn effect(fault: &str) -> HostProcess {
    let mut host = spawn(fault);
    let id = effect_class();
    host.load(plugin(), Some(id), SLOW).expect("load");
    host.configure(48_000.0, 512, 2, 2, SLOW)
        .expect("configure");
    host
}

pub fn effect_class() -> [u8; 16] {
    let tuid = lumen_test_plugin::EFFECT;
    tuid.map(|c| c as u8)
}

pub fn note_on(offset: u32, pitch: i16) -> Event {
    Event {
        offset,
        kind: EventKind::NoteOn {
            channel: 0,
            pitch,
            velocity: 1.0,
        },
    }
}

pub fn note_off(offset: u32, pitch: i16) -> Event {
    Event {
        offset,
        kind: EventKind::NoteOff {
            channel: 0,
            pitch,
            velocity: 0.0,
        },
    }
}

/// Whether a process with this id exists (and has not been reaped).
pub fn exists(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // A zombie still has a /proc entry; it has stopped running.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat.contains(") Z "),
            Err(_) => false,
        }
    }
    #[cfg(target_os = "macos")]
    {
        // `kill(pid, 0)` reports whether the process still exists without
        // sending it a signal. A process adopted and reaped after its parent
        // exits stops matching before this returns false.
        // SAFETY: signal zero only queries whether this PID exists.
        unsafe {
            libc::kill(pid as libc::pid_t, 0) == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        false
    }
}
