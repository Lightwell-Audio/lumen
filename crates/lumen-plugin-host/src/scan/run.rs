//! Lumen's side of a scan: one child process for one module, a deadline,
//! and an answer that says how it went.
//!
//! Nothing here can block on the module. The child's output is read on
//! threads of their own, the wait polls, and a child that outlives its
//! deadline or a cancel is killed.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::client::ExitKind;
use crate::scan::report::{MAX_REPLY, ScanReply, ScanReport};

/// How much of a child's error output is kept to explain a failure. The
/// rest is read and dropped so the child never blocks on a full pipe.
const STDERR_KEPT: usize = 2048;
/// How long to wait for the output threads once the child has ended.
const DRAIN: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(5);
/// How long the check of the scan program may take: it loads nothing.
const CHECK_DEADLINE: Duration = Duration::from_secs(10);

/// How one module's scan ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    /// The module loaded and described itself.
    Scanned(ScanReport),
    /// The module could not be loaded or has no factory; the message is the
    /// loader's.
    Failed(String),
    /// The child died, by a signal or an exception, or exited with a failure
    /// code before it could answer. `how` names which; `detail` is the tail
    /// of what the module printed, if anything.
    Crashed { how: String, detail: String },
    /// The child did not answer by the deadline and was killed.
    TimedOut,
    /// The scan was cancelled and the child killed.
    Cancelled,
    /// The child ended cleanly but its reply does not fit.
    Malformed(String),
    /// The child could not be started (a missing or unusable host binary).
    Unstarted(String),
}

/// Scans `module` with the host binary at `exe`, giving it `deadline`.
/// `env` is added to the child's environment (tests choose a fault with it).
/// Setting `cancel` ends the scan within a few milliseconds.
pub fn run_scan(
    exe: &Path,
    module: &Path,
    deadline: Duration,
    env: &[(&str, &str)],
    cancel: &AtomicBool,
) -> ScanOutcome {
    let mut command = Command::new(exe);
    command
        .arg("scan")
        .arg(module)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    // Windows lets a child inherit a pipe end that another thread has made
    // inheritable and not yet closed. Spawning one child at a time keeps
    // scans that run side by side out of each other's pipes.
    let spawned = {
        static SPAWN: Mutex<()> = Mutex::new(());
        let _one_at_a_time = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        command.spawn()
    };
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return ScanOutcome::Unstarted(format!("{}: {e}", exe.display())),
    };
    #[cfg(windows)]
    if let Err(e) = crate::client::end_with_lumen(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return ScanOutcome::Unstarted(format!("could not group the scan process: {e}"));
    }

    // One byte past the limit tells "exactly full" from "too much".
    let stdout = collect(child.stdout.take().expect("piped stdout"), MAX_REPLY + 1);
    let stderr = collect(child.stderr.take().expect("piped stderr"), STDERR_KEPT);

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return stopped(&mut child, ScanOutcome::Unstarted(e.to_string())),
        }
        if cancel.load(Ordering::Relaxed) {
            return stopped(&mut child, ScanOutcome::Cancelled);
        }
        if started.elapsed() >= deadline {
            return stopped(&mut child, ScanOutcome::TimedOut);
        }
        std::thread::sleep(POLL);
    };

    // The child is gone, so everything it wrote is in the pipes. Wait briefly
    // for the end of each, but use what was read either way: something else
    // holding a pipe open (another process that inherited it, a helper the
    // module started) must not turn a finished answer into "no reply".
    let reply = stdout.finish();
    let detail = String::from_utf8_lossy(&stderr.finish()).trim().to_string();
    classify(status, &reply, detail)
}

/// A child's output read as it arrives, so that what has been written is
/// never lost to a pipe that stays open.
struct Collected {
    bytes: Arc<Mutex<Vec<u8>>>,
    ended: mpsc::Receiver<()>,
}

impl Collected {
    /// What was read, once the pipe ended or [`DRAIN`] passed.
    fn finish(&self) -> Vec<u8> {
        let _ = self.ended.recv_timeout(DRAIN);
        self.bytes.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Reads `pipe` on a thread of its own, keeping the first `keep` bytes and
/// reading on past them so a child that writes more cannot stall.
fn collect(mut pipe: impl Read + Send + 'static, keep: usize) -> Collected {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ended_tx, ended) = mpsc::channel();
    let shared = Arc::clone(&bytes);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut kept = shared.lock().unwrap_or_else(|e| e.into_inner());
            let room = keep.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..n.min(room)]);
        }
        let _ = ended_tx.send(());
    });
    Collected { bytes, ended }
}

/// Asks the scan program at `exe` a question it can only answer if it is a
/// current `lumen-plugin-host`: to scan a module that does not exist, which
/// it must refuse with a reply of its own. No plugin code runs. An older
/// build, or some other program, ends without that reply; every module it
/// was asked about would then look blocked, so the caller must not scan with
/// it. The error says what to do.
pub fn check_host(exe: &Path) -> Result<(), String> {
    let missing = exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("lumen-scan-check-no-such-module.vst3");
    let outcome = run_scan(exe, &missing, CHECK_DEADLINE, &[], &AtomicBool::new(false));
    match outcome {
        ScanOutcome::Failed(_) => Ok(()),
        ScanOutcome::Unstarted(why) => Err(why),
        other => Err(format!(
            "The plugin scanner at {} did not answer a test ({other:?}); it is probably \
             out of date. Rebuild it with: cargo build --release -p lumen-plugin-host",
            exe.display()
        )),
    }
}

/// Ends a child that is still running and returns `outcome`.
fn stopped(child: &mut std::process::Child, outcome: ScanOutcome) -> ScanOutcome {
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

/// Turns how the child ended and what it wrote into an outcome.
fn classify(status: std::process::ExitStatus, reply: &[u8], detail: String) -> ScanOutcome {
    match ExitKind::of(status) {
        ExitKind::Clean => {}
        ExitKind::Crashed(how) => return ScanOutcome::Crashed { how, detail },
        ExitKind::Failed(code) => {
            return ScanOutcome::Crashed {
                how: format!("exit code {code}"),
                detail,
            };
        }
        ExitKind::Killed => return ScanOutcome::Cancelled,
    }
    if reply.is_empty() {
        return ScanOutcome::Malformed("the scan process ended without a reply".into());
    }
    match ScanReply::decode(reply) {
        Ok(ScanReply::Report(report)) => ScanOutcome::Scanned(report),
        Ok(ScanReply::Failed(message)) => ScanOutcome::Failed(message),
        Err(e) => ScanOutcome::Malformed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yields its bytes, then waits for a signal that never comes, like a
    /// pipe another process still holds open.
    struct Held {
        bytes: Option<Vec<u8>>,
        never: mpsc::Receiver<()>,
    }

    impl Read for Held {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if let Some(bytes) = self.bytes.take() {
                out[..bytes.len()].copy_from_slice(&bytes);
                return Ok(bytes.len());
            }
            let _ = self.never.recv();
            Ok(0)
        }
    }

    #[test]
    fn what_was_written_survives_a_pipe_that_never_closes() {
        let (hold, never) = mpsc::channel::<()>();
        let held = Held {
            bytes: Some(b"reply".to_vec()),
            never,
        };
        let collected = collect(held, 64);
        assert_eq!(collected.finish(), b"reply");
        drop(hold);
    }

    #[test]
    fn only_the_first_bytes_are_kept() {
        let collected = collect(std::io::Cursor::new(vec![7u8; 100_000]), 10);
        assert_eq!(collected.finish(), vec![7u8; 10]);
    }
}
