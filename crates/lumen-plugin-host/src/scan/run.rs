use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::client::ExitKind;
use crate::scan::report::{MAX_REPLY, ScanReply, ScanReport};

const STDERR_KEPT: usize = 2048;
const DRAIN: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(5);
const CHECK_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Scanned(ScanReport),
    Failed(String),
    Crashed { how: String, detail: String },
    TimedOut,
    Cancelled,
    Malformed(String),
    Unstarted(String),
}

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
    crate::process::started_by_this_process(&mut command);
    for (key, value) in env {
        command.env(key, value);
    }
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

    let stdout = match collect(
        child.stdout.take().expect("piped stdout"),
        MAX_REPLY + 1,
        "output",
    ) {
        Ok(read) => read,
        Err(error) => {
            let outcome = reader_failed("output", error);
            return stopped(&mut child, outcome);
        }
    };
    let stderr = collect(
        child.stderr.take().expect("piped stderr"),
        STDERR_KEPT,
        "errors",
    )
    .ok();

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

    let reply = stdout.finish();
    let detail = match stderr {
        Some(stderr) => String::from_utf8_lossy(&stderr.finish()).trim().to_string(),
        None => String::new(),
    };
    classify(status, &reply, detail)
}

struct Collected {
    bytes: Arc<Mutex<Vec<u8>>>,
    ended: mpsc::Receiver<()>,
}

impl Collected {
    fn finish(&self) -> Vec<u8> {
        let _ = self.ended.recv_timeout(DRAIN);
        self.bytes.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn collect(mut pipe: impl Read + Send + 'static, keep: usize, what: &str) -> io::Result<Collected> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (ended_tx, ended) = mpsc::channel();
    let shared = Arc::clone(&bytes);
    std::thread::Builder::new()
        .name(format!("lumen-scan-{what}"))
        .spawn(move || {
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
        })?;
    Ok(Collected { bytes, ended })
}

fn reader_failed(what: &str, error: io::Error) -> ScanOutcome {
    ScanOutcome::Unstarted(format!(
        "could not start the thread reading the scan's {what}: {error}"
    ))
}

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

fn stopped(child: &mut std::process::Child, outcome: ScanOutcome) -> ScanOutcome {
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

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
        let collected = collect(held, 64, "output").expect("a reader");
        assert_eq!(collected.finish(), b"reply");
        drop(hold);
    }

    #[test]
    fn only_the_first_bytes_are_kept() {
        let read = collect(std::io::Cursor::new(vec![7u8; 100_000]), 10, "output");
        assert_eq!(read.expect("a reader").finish(), vec![7u8; 10]);
    }

    #[test]
    fn a_reader_thread_that_cannot_start_is_a_scan_that_could_not_start() {
        let outcome = reader_failed(
            "output",
            std::io::Error::from(std::io::ErrorKind::OutOfMemory),
        );
        match outcome {
            ScanOutcome::Unstarted(why) => {
                assert!(why.contains("thread"), "{why}");
                assert!(why.contains("output"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }
}
