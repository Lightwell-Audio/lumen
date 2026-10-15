//! Lumen's side: start a host process and talk to it, with a deadline on
//! everything. Nothing here can block forever on a plugin: writes go through
//! a thread of their own, reads arrive on a channel, and a host that does not
//! answer in time is reported, then killed by the caller.
//!
//! This is the control path (load, configure, state, parameters) and the
//! protocol check for the audio path. A response that arrives after its
//! deadline is *stale*: the next request drops it, counts it, and never lets
//! it stand for the block now being waited for.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::protocol::{
    ClassId, ClassInfo, Event, FailureKind, FrameError, ParamInfo, ParamQueue, Request, Response,
    read_response, write_request,
};

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

impl ExitKind {
    fn of(status: std::process::ExitStatus) -> Self {
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

#[derive(Debug, Clone, PartialEq)]
pub enum HostError {
    /// No answer by the deadline. The host is suspect: kill it.
    Timeout,
    /// The host process is gone.
    Ended(ExitKind),
    /// The host answered with something that does not fit.
    Protocol(String),
    /// The host refused or failed the request.
    Failed { kind: FailureKind, message: String },
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Timeout => write!(f, "the plugin host did not answer in time"),
            HostError::Ended(how) => write!(f, "the plugin host ended: {how:?}"),
            HostError::Protocol(what) => write!(f, "the plugin host broke the protocol: {what}"),
            HostError::Failed { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostError {}

/// What a processed block gave back.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
    pub output: Vec<Vec<f32>>,
}

type Incoming = Result<Response, FrameError>;

pub struct HostProcess {
    child: Child,
    writer: Option<Sender<Vec<u8>>>,
    incoming: Receiver<Incoming>,
    threads: Vec<JoinHandle<()>>,
    generation: u32,
    next_seq: u64,
    /// Responses that arrived for a block or configuration already given up on.
    stale: u32,
    killed: bool,
}

impl HostProcess {
    /// Starts `exe` as a host, with `env` added to its environment.
    pub fn spawn(exe: &Path, env: &[(&str, &str)]) -> std::io::Result<Self> {
        let mut command = Command::new(exe);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");

        let (tx, incoming) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name("host-reader".into())
            .spawn(move || {
                loop {
                    let item = read_response(&mut stdout);
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
            child,
            writer: Some(writer),
            incoming,
            threads: vec![reader, write_thread],
            generation: 0,
            next_seq: 0,
            stale: 0,
            killed: false,
        })
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

    /// How the host ended, if it has.
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
    fn lost(&mut self) -> HostError {
        match self.wait_ended(Duration::from_millis(500)) {
            Some(how) => HostError::Ended(how),
            None => HostError::Timeout,
        }
    }

    fn send(&mut self, request: &Request) -> Result<(), HostError> {
        let mut frame = Vec::new();
        write_request(&mut frame, request).map_err(|e| HostError::Protocol(e.to_string()))?;
        match &self.writer {
            Some(writer) if writer.send(frame).is_ok() => Ok(()),
            _ => Err(self.lost()),
        }
    }

    /// The next response that is not stale, within `deadline`.
    fn receive(
        &mut self,
        deadline: Instant,
        wanted: Option<(u32, u64)>,
    ) -> Result<Response, HostError> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let item = match self.incoming.recv_timeout(left) {
                Ok(item) => item,
                Err(RecvTimeoutError::Timeout) => return Err(HostError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(self.lost()),
            };
            let response = match item {
                Ok(response) => response,
                Err(FrameError::Closed | FrameError::Io(_)) => return Err(self.lost()),
                Err(FrameError::Wire(e)) => return Err(HostError::Protocol(e.to_string())),
            };
            match (&response, wanted) {
                // The answer to the block being waited for.
                (
                    Response::Processed {
                        generation, seq, ..
                    },
                    Some((g, s)),
                ) if *generation == g && *seq == s => {
                    return Ok(response);
                }
                // A block answered after its deadline, or from before a
                // reconfiguration: not the one waited for.
                (Response::Processed { .. }, _) => self.stale += 1,
                (Response::Failed { kind, message }, _) => {
                    return Err(HostError::Failed {
                        kind: *kind,
                        message: message.clone(),
                    });
                }
                (_, None) => return Ok(response),
                // A control response while waiting for a block: out of step.
                (_, Some(_)) => {
                    return Err(HostError::Protocol("unexpected response".into()));
                }
            }
        }
    }

    fn call(&mut self, request: &Request, within: Duration) -> Result<Response, HostError> {
        let deadline = Instant::now() + within;
        self.send(request)?;
        self.receive(deadline, None)
    }

    pub fn load(
        &mut self,
        path: &Path,
        class: Option<ClassId>,
        within: Duration,
    ) -> Result<ClassInfo, HostError> {
        let request = Request::Load {
            path: path.to_string_lossy().into_owned(),
            class,
        };
        match self.call(&request, within)? {
            Response::Loaded(info) => Ok(info),
            _ => Err(HostError::Protocol("expected Loaded".into())),
        }
    }

    /// Sets the format and starts a new generation: blocks and answers of
    /// the previous one are stale from here on. Returns `(latency, tail)`.
    pub fn configure(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
        inputs: u8,
        outputs: u8,
        within: Duration,
    ) -> Result<(u32, u32), HostError> {
        self.generation += 1;
        let request = Request::Configure {
            generation: self.generation,
            sample_rate,
            max_frames,
            inputs,
            outputs,
        };
        match self.call(&request, within)? {
            Response::Configured {
                generation,
                latency,
                tail,
            } if generation == self.generation => Ok((latency, tail)),
            _ => Err(HostError::Protocol("expected Configured".into())),
        }
    }

    /// Renders one block, or reports why it could not. On `Timeout` the
    /// block is given up on; if the host answers later, the answer is
    /// dropped as stale by the next call.
    pub fn process(
        &mut self,
        frames: u32,
        input: Vec<Vec<f32>>,
        events: Vec<Event>,
        params: Vec<ParamQueue>,
        within: Duration,
    ) -> Result<Block, HostError> {
        let deadline = Instant::now() + within;
        let seq = self.next_seq;
        self.next_seq += 1;
        let generation = self.generation;
        self.send(&Request::Process {
            generation,
            seq,
            frames,
            input,
            events,
            params,
        })?;
        match self.receive(deadline, Some((generation, seq)))? {
            Response::Processed {
                status,
                latency,
                restart,
                repaired,
                output,
                ..
            } => Ok(Block {
                status,
                latency,
                restart,
                repaired,
                output,
            }),
            _ => Err(HostError::Protocol("expected Processed".into())),
        }
    }

    pub fn state(&mut self, within: Duration) -> Result<Vec<u8>, HostError> {
        match self.call(&Request::GetState, within)? {
            Response::State(bytes) => Ok(bytes),
            _ => Err(HostError::Protocol("expected State".into())),
        }
    }

    pub fn set_state(&mut self, state: Vec<u8>, within: Duration) -> Result<(), HostError> {
        match self.call(&Request::SetState(state), within)? {
            Response::Done => Ok(()),
            _ => Err(HostError::Protocol("expected Done".into())),
        }
    }

    pub fn parameters(&mut self, within: Duration) -> Result<(u32, Vec<ParamInfo>), HostError> {
        match self.call(&Request::Parameters, within)? {
            Response::Parameters { total, list } => Ok((total, list)),
            _ => Err(HostError::Protocol("expected Parameters".into())),
        }
    }

    /// Asks the host to quit and waits for it; kills it if it does not.
    pub fn quit(&mut self, within: Duration) -> ExitKind {
        let _ = self.call(&Request::Quit, within);
        self.writer = None;
        match self.wait_ended(within) {
            Some(how) => how,
            None => {
                self.kill();
                ExitKind::Killed
            }
        }
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        // No host outlives the handle that started it.
        self.kill();
        self.writer = None;
        for thread in self.threads.drain(..) {
            // The pipes are closed now; the threads end. A writer stuck in a
            // blocked write ends when the killed child's pipe breaks.
            let _ = thread.join();
        }
    }
}
