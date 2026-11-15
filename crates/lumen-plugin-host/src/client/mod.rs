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
    Timeout,
    Ended(ExitKind),
    Protocol(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configured {
    pub latency: u32,
    pub tail: u32,
    pub inputs: u8,
    pub outputs: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub status: i32,
    pub latency: u32,
    pub restart: u32,
    pub repaired: u32,
    pub output: Vec<Vec<f32>>,
}

type Incoming = Result<Response, FrameError>;

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
    stale: u32,
    owed: u32,
    notices: Notices,
    killed: bool,
}
