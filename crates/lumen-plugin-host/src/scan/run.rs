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
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::client::ExitKind;
use crate::scan::report::{MAX_REPLY, ScanReply, ScanReport};

/// How much of a child's error output is kept to explain a failure. The
/// rest is read and dropped so the child never blocks on a full pipe.
const STDERR_KEPT: usize = 2048;
/// How long to wait for the output threads once the child has ended.
const DRAIN: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(5);

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
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return ScanOutcome::Unstarted(format!("{}: {e}", exe.display())),
    };
    #[cfg(windows)]
    if let Err(e) = crate::client::end_with_lumen(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return ScanOutcome::Unstarted(format!("could not group the scan process: {e}"));
    }

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    // One byte past the limit tells "exactly full" from "too much".
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout)
            .take(MAX_REPLY as u64 + 1)
            .read_to_end(&mut bytes);
        // Keep reading so a module that writes on cannot stall on the pipe.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        let _ = out_tx.send(bytes);
    });
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = (&mut stderr)
            .take(STDERR_KEPT as u64)
            .read_to_end(&mut kept);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = err_tx.send(kept);
    });

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

    let reply = out_rx.recv_timeout(DRAIN).unwrap_or_default();
    let detail = String::from_utf8_lossy(&err_rx.recv_timeout(DRAIN).unwrap_or_default())
        .trim()
        .to_string();
    classify(status, &reply, detail)
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
