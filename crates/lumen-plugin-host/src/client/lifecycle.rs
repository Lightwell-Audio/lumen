//! The host process's life: starting it with its pipes, and how it ends.

use crate::protocol::read_response;
use std::fmt;
use std::io::Write;
use std::path::Path;
#[cfg(windows)]
use std::process::Child;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::log::host_line;
use super::{HostError, HostLog, HostProcess, NoticeSink, Notices};

/// Hosts are put in a job that ends them with Lumen: Windows has no request
/// like Linux's parent-death signal, and a host hung in a plugin would
/// otherwise outlive a killed Lumen. The job is never closed by Lumen; the
/// kernel closes it when Lumen's process ends.
#[cfg(windows)]
pub(crate) fn end_with_lumen(child: &Child) -> std::io::Result<()> {
    use std::sync::OnceLock;
    static JOB: OnceLock<Result<crate::shm::sys::Job, String>> = OnceLock::new();
    match JOB.get_or_init(|| crate::shm::sys::Job::new().map_err(|e| e.to_string())) {
        Ok(job) => job.assign(child),
        Err(why) => Err(std::io::Error::other(why.clone())),
    }
}

/// How a host process that has stopped ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitKind {
    /// Quit, or its stream closed, with a success code.
    Clean,
    /// Exited with a failure code (a refused message, an I/O error).
    Failed(i32),
    /// Killed by a signal or an exception: a crash.
    Crashed(String),
    /// Killed by Lumen after a missed deadline.
    Killed,
}

impl fmt::Display for ExitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitKind::Clean => f.write_str("it quit"),
            ExitKind::Failed(code) => write!(f, "it exited with code {code}"),
            ExitKind::Killed => f.write_str("it was ended after it stopped answering"),
            ExitKind::Crashed(how) => match crash_name(how) {
                Some(name) => write!(f, "it crashed: {name}, {how}"),
                None => write!(f, "it crashed: {how}"),
            },
        }
    }
}

/// What a Windows exception code means, for the codes plugins most often
/// die of.
fn crash_name(how: &str) -> Option<&'static str> {
    let code = how.strip_prefix("exception ")?.to_ascii_lowercase();
    Some(match code.as_str() {
        "0xc0000005" => "access violation",
        "0xc000001d" => "illegal instruction",
        "0xc0000094" => "integer divide by zero",
        "0xc00000fd" => "stack overflow",
        "0xc0000135" => "a library it needs was not found",
        "0xc0000142" => "a library it needs could not start",
        "0xc0000374" => "heap corruption",
        "0xc0000409" => "the plugin stopped itself (a failed check)",
        "0xe06d7363" => "an uncaught C++ exception",
        _ => return None,
    })
}

impl ExitKind {
    pub(crate) fn of(status: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                return ExitKind::Crashed(format!("signal {signal}"));
            }
        }
        match status.code() {
            Some(0) => ExitKind::Clean,
            // Windows reports a crash as an exception code in the exit code.
            #[cfg(windows)]
            Some(code) if (code as u32) >= 0xC000_0000 => {
                ExitKind::Crashed(format!("exception {:#010x}", code as u32))
            }
            Some(code) => ExitKind::Failed(code),
            None => ExitKind::Failed(-1),
        }
    }
}

impl HostProcess {
    /// What the host process has said about itself.
    pub fn log(&self) -> std::sync::Arc<HostLog> {
        self.log.clone()
    }

    /// Starts `exe` as a host, with `env` added to its environment.
    pub fn spawn(exe: &Path, env: &[(&str, &str)]) -> std::io::Result<Self> {
        // The host shares its stderr with the plugin, so only lines marked with
        // a nonce this process chose are the host's: a plugin that prints
        // "lumen-host: ..." cannot pass itself off as one.
        let nonce = crate::process::new_nonce();
        let mut command = Command::new(exe);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::process::started_by_this_process(&mut command);
        for (key, value) in env {
            command.env(key, value);
        }
        // Last, so a caller cannot replace the nonce with one this side would
        // not recognise.
        command.env(crate::process::NONCE_ENV, &nonce);
        let mut child = command.spawn()?;
        #[cfg(windows)]
        if let Err(e) = end_with_lumen(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let log = std::sync::Arc::new(HostLog::default());
        let stderr = child.stderr.take().expect("piped stderr");
        let reader_log = log.clone();
        let mark = crate::process::mark(&nonce);
        // Always drained, so a plugin that prints a lot never blocks on a
        // full pipe; only the host's own lines are kept.
        // Not joined with the others: a helper process a plugin started can keep
        // the pipe open after the host is gone, and the thread simply ends with it.
        std::thread::Builder::new()
            .name("host-stderr".into())
            .spawn(move || {
                use std::io::{BufRead, Read};
                let mut lines = std::io::BufReader::new(stderr);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    // Bounded: a line without an end is cut, not grown.
                    match lines.by_ref().take(4096).read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let text = String::from_utf8_lossy(&buf);
                            if let Some(rest) = host_line(&text, &mark) {
                                reader_log.push(rest);
                            }
                        }
                    }
                }
            })?;

        let (tx, incoming) = mpsc::channel();
        let notices: Notices = Default::default();
        let sink = notices.clone();
        let reader = std::thread::Builder::new()
            .name("host-reader".into())
            .spawn(move || {
                loop {
                    let item = read_response(&mut stdout);
                    if let Ok(response) = &item
                        && response.is_notice()
                    {
                        if let Some(sink) = &*sink.lock().unwrap_or_else(|e| e.into_inner()) {
                            sink(response.clone());
                        }
                        continue;
                    }
                    let done = item.is_err();
                    if tx.send(item).is_err() || done {
                        break;
                    }
                }
            })?;

        let (writer, bytes) = mpsc::channel::<Vec<u8>>();
        let write_thread = std::thread::Builder::new()
            .name("host-writer".into())
            .spawn(move || {
                // A write that blocks (the host stopped reading) blocks only
                // this thread; the caller's deadline still holds.
                while let Ok(frame) = bytes.recv() {
                    if stdin
                        .write_all(&frame)
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;

        Ok(Self {
            log,
            child,
            writer: Some(writer),
            incoming,
            threads: vec![reader, write_thread],
            generation: 0,
            next_seq: 0,
            stale: 0,
            owed: 0,
            notices,
            killed: false,
        })
    }

    /// Sends the host's news (keys pressed in its plugin's window) to `sink`,
    /// from the thread that reads the host's answers.
    pub fn on_notice(&self, sink: NoticeSink) {
        *self.notices.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Responses dropped as stale so far.
    pub fn stale(&self) -> u32 {
        self.stale
    }

    /// Whether the host has ended, without saying how. Allocation-free, so
    /// the audio loop may ask it after a missed deadline; [`Self::ended`]
    /// words a crash and so allocates (found by the MT-VST-0 crash runs).
    pub fn has_ended(&mut self) -> bool {
        self.killed || matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// How the host ended, if it has. Allocates when it crashed: not for the
    /// audio loop, which asks [`Self::has_ended`].
    pub fn ended(&mut self) -> Option<ExitKind> {
        if self.killed {
            return Some(ExitKind::Killed);
        }
        self.child.try_wait().ok().flatten().map(ExitKind::of)
    }

    /// Waits up to `limit` for the host to end.
    pub fn wait_ended(&mut self, limit: Duration) -> Option<ExitKind> {
        let start = Instant::now();
        loop {
            if let Some(how) = self.ended() {
                return Some(how);
            }
            if start.elapsed() >= limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Ends the host now. The caller's answer to a missed deadline.
    pub fn kill(&mut self) {
        if !self.killed {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.killed = true;
        }
    }

    /// The error for a host that stopped answering: say how it ended if it
    /// did, and otherwise that it timed out.
    pub(super) fn lost(&mut self) -> HostError {
        match self.wait_ended(Duration::from_millis(500)) {
            Some(how) => HostError::Ended(how),
            None => HostError::Timeout,
        }
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        // No host outlives the handle that started it.
        self.kill();
        self.writer = None;
        // The pipes are closed now; the threads end. A writer stuck in a
        // blocked write ends when the killed child's pipe breaks. A reader
        // does not, if a helper process the plugin started inherited the
        // pipe: it is let go after a moment rather than waited for, as the
        // stderr reader is.
        join_within(self.threads.drain(..), THREADS_END_WITHIN);
    }
}

/// How long dropping a host waits for its pipe threads to end.
const THREADS_END_WITHIN: Duration = Duration::from_millis(500);

/// Joins the threads that end within `within` in all; the rest are let go
/// (they end by themselves when what they wait on does).
pub(super) fn join_within(threads: impl Iterator<Item = JoinHandle<()>>, within: Duration) {
    let end = Instant::now() + within;
    for thread in threads {
        while !thread.is_finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
}
