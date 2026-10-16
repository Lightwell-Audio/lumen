//! The stand-in's end of the pipe to an editor host process: one command, one
//! answer, with a deadline on every wait.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

pub struct Host {
    child: Child,
    stdin: ChildStdin,
    answers: Receiver<String>,
    events: Arc<Mutex<Vec<(Instant, String)>>>,
    next: u32,
}

impl Host {
    /// Starts a host process for `plugin` and waits until it says it is ready.
    pub fn spawn(
        plugin: &Path,
        class: &str,
        owner: Option<isize>,
        topmost: bool,
        faults: &str,
    ) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = Command::new(exe);
        command
            .args(["host", "--plugin"])
            .arg(plugin)
            .args(["--class", class])
            .env(lumen_fault_variable(), faults)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some(owner) = owner {
            command.args(["--owner", &owner.to_string()]);
        }
        if topmost {
            command.arg("--topmost");
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("start the host: {e}"))?;
        // The stand-in owns the foreground; let the host take it for its window.
        // SAFETY: a plain call naming a process we just started.
        unsafe { AllowSetForegroundWindow(child.id()) };
        let stdin = child.stdin.take().ok_or("no pipe to the host")?;
        let stdout = child.stdout.take().ok_or("no pipe from the host")?;
        let (tx, answers) = mpsc::channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&events);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(event) = line.strip_prefix("event ") {
                    seen.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((Instant::now(), event.to_string()));
                } else if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let host = Self {
            child,
            stdin,
            answers,
            events,
            next: 1,
        };
        let began = Instant::now();
        while !host.saw("ready") {
            if began.elapsed() > Duration::from_secs(20) {
                return Err("the host never became ready".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(host)
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn saw(&self, prefix: &str) -> bool {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|(_, e)| e.starts_with(prefix))
    }

    pub fn events(&self) -> Vec<(Instant, String)> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Sends a command and returns the answer without its id, or why there
    /// was none within `wait`.
    pub fn send(&mut self, command: &str, wait: Duration) -> Result<String, String> {
        let id = self.next;
        self.next += 1;
        writeln!(self.stdin, "{id} {command}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("write: {e}"))?;
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .answers
                .recv_timeout(left)
                .map_err(|_| "no answer in time".to_string())?;
            let (got, rest) = line.split_once(' ').unwrap_or((&line, ""));
            // An answer to an earlier command that gave up is dropped.
            if got.parse::<u32>().is_ok_and(|n| n == id) {
                return Ok(rest.to_string());
            }
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.kill();
    }
}

fn lumen_fault_variable() -> &'static str {
    "LUMEN_TEST_PLUGIN_FAULT"
}

/// `key=value` from an answer line.
pub fn field<'a>(answer: &'a str, key: &str) -> Option<&'a str> {
    answer
        .split_whitespace()
        .find_map(|part| part.strip_prefix(key)?.strip_prefix('='))
}

/// `AxB` from an answer field.
pub fn pair(text: &str) -> Option<(i32, i32)> {
    let (a, b) = text.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}
