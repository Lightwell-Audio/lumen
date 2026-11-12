//! Lumen's side: start a host process and talk to it, with a deadline on
//! everything. Nothing here can block forever on a plugin: writes go through
//! a thread of their own, reads arrive on a channel, and a host that does not
//! answer in time is reported, then killed by the caller.
//!
//! This is the control path (load, configure, state, parameters) and the
//! protocol check for the audio path. A response that arrives after its
//! deadline is *stale*: the host answers every request once and in order, so
//! the answers owed to requests given up on are the next ones to arrive, and
//! each is dropped and counted. A late answer never stands for the request now
//! being waited for (a late `Edits` once read as the answer to `Configure`).

use std::fmt;
use std::process::Child;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use crate::protocol::{FailureKind, FrameError, Response};

mod calls;
mod lifecycle;
mod log;
#[cfg(test)]
mod tests;

pub use lifecycle::ExitKind;
#[cfg(windows)]
pub(crate) use lifecycle::end_with_lumen;
pub use log::HostLog;

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
            HostError::Ended(how) => write!(f, "the plugin host ended: {how}"),
            HostError::Protocol(what) => write!(f, "the plugin host broke the protocol: {what}"),
            HostError::Failed { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostError {}

/// What a plugin answered to being set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configured {
    pub latency: u32,
    pub tail: u32,
    /// The channels a block carries each way.
    pub inputs: u8,
    pub outputs: u8,
}

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

/// Where the host's news goes ([`Response::is_notice`]): called on the
/// reader thread as each arrives.
pub type NoticeSink = Box<dyn Fn(Response) + Send>;
type Notices = std::sync::Arc<std::sync::Mutex<Option<NoticeSink>>>;

pub struct HostProcess {
    log: std::sync::Arc<HostLog>,
    child: Child,
    writer: Option<Sender<Vec<u8>>>,
    incoming: Receiver<Incoming>,
    threads: Vec<JoinHandle<()>>,
    generation: u32,
    next_seq: u64,
    /// Responses that arrived for a block or configuration already given up on.
    stale: u32,
    /// Answers still to come for requests given up on: the host answers in
    /// order, so they are the next ones to arrive.
    owed: u32,
    notices: Notices,
    killed: bool,
}
