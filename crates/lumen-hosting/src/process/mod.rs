use std::cell::Cell;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lumen_devices::MOST_FRAMES;
use lumen_plugin_host::client::{ExitKind, HostError, HostProcess};
use lumen_plugin_host::protocol::{
    Aux, BusInfo, EditorBounds, MAX_BUSES, ParamInfo, ProgramList, Response, Taken, UnitInfo,
};
use lumen_plugin_host::shm::{Layout, Wake};

use crate::link::{Counts, Link, Pair};

#[cfg(not(target_os = "macos"))]
pub const WAKE: Wake = Wake::EVENT;

#[cfg(target_os = "macos")]
pub const WAKE: Wake = Wake {
    host: lumen_plugin_host::shm::Waiting::Event,
    lumen: lumen_plugin_host::shm::Waiting::Spin,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    Load,
    State,
    Configure,
    Attach,
    Running,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub step: Step,
    pub error: HostError,
    pub detail: Option<String>,
}

impl Step {
    fn doing(self) -> &'static str {
        match self {
            Step::Start => "starting",
            Step::Load => "loading",
            Step::State => "restoring its saved state",
            Step::Configure => "setting up",
            Step::Attach => "opening its audio link",
            Step::Running => "running",
        }
    }
}

impl Failure {
    pub fn crashed(&self) -> bool {
        matches!(
            self.error,
            HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
        )
    }

    pub fn hung(&self) -> bool {
        matches!(self.error, HostError::Timeout)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.step {
            Step::Start => "start the plugin host",
            Step::Load => "load the plugin",
            Step::State => "restore the plugin's saved state",
            Step::Configure => "set the plugin up",
            Step::Attach => "open the audio link to the plugin",
            Step::Running => "keep running",
        };
        match &self.error {
            HostError::Ended(ExitKind::Crashed(how)) => {
                let cause = ExitKind::Crashed(how.clone()).to_string();
                let cause = cause.strip_prefix("it crashed: ").unwrap_or(&cause);
                match &self.detail {
                    Some(detail) => {
                        write!(f, "Crashed while {}: {cause}, {detail}.", self.step.doing())
                    }
                    None => write!(f, "Crashed while {}: {cause}.", self.step.doing()),
                }
            }
            HostError::Timeout => write!(f, "The plugin did not answer while trying to {what}."),
            HostError::Ended(how) => {
                write!(f, "The plugin host ended while trying to {what}: {how}.")
            }
            HostError::Protocol(why) => write!(f, "The plugin host broke the protocol: {why}."),
            HostError::Failed { message, .. } => {
                write!(f, "The plugin could not {what}: {message}")
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    pub load: Duration,
    pub state: Duration,
    pub configure: Duration,
    pub attach: Duration,
    pub quit: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            load: Duration::from_secs(20),
            state: Duration::from_secs(10),
            configure: Duration::from_secs(5),
            attach: Duration::from_secs(2),
            quit: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Start {
    pub host: PathBuf,
    pub module: PathBuf,
    pub class: [u8; 16],
    pub instrument: bool,
    pub state: Vec<u8>,
    pub controller_state: Vec<u8>,
    pub values: Vec<(u32, f64)>,
    pub rate: f64,
    pub frames: u32,
    pub env: Vec<(String, String)>,
    pub deadlines: Deadlines,
    pub wake: Wake,
    pub aux: Aux,
}

impl Start {
    pub fn new(
        host: PathBuf,
        module: PathBuf,
        class: [u8; 16],
        instrument: bool,
        rate: f64,
    ) -> Self {
        Self {
            host,
            module,
            class,
            instrument,
            state: Vec::new(),
            controller_state: Vec::new(),
            values: Vec::new(),
            rate,
            frames: MOST_FRAMES as u32,
            env: Vec::new(),
            deadlines: Deadlines::default(),
            wake: WAKE,
            aux: Aux::default(),
        }
    }
}

#[derive(Debug)]
pub enum Happened {
    Linked(Box<Linked>),
    Failed(Failure),
    Edits(Taken),
    Editor(EditorEvent),
    Text {
        id: u32,
        value: f64,
        text: String,
    },
    Typed {
        id: u32,
        text: String,
        value: Option<f64>,
    },
    State(Result<Captured, String>),
    Values(Vec<ParamInfo>),
    Loaded(Result<Vec<(u32, f64)>, String>),
    Key {
        code: u8,
        down: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub component: Vec<u8>,
    pub controller: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    Opened(EditorBounds),
    Moved(EditorBounds),
    Closed(Option<EditorBounds>),
    NotResponding,
    Responding,
    Refused(String),
}

pub struct Linked {
    pub link: Link,
    pub name: String,
    pub vendor: String,
    pub latency: u32,
    pub tail: u32,
    pub frames: u32,
    pub params: Vec<ParamInfo>,
    pub params_total: u32,
    pub units: Vec<UnitInfo>,
    pub programs: Vec<ProgramList>,
    pub buses: (Vec<BusInfo>, Vec<BusInfo>),
    pub aux: Aux,
    pub pid: u32,
    pub log: Arc<lumen_plugin_host::client::HostLog>,
}

impl fmt::Debug for Linked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Linked")
            .field("name", &self.name)
            .field("latency", &self.latency)
            .field("tail", &self.tail)
            .finish_non_exhaustive()
    }
}

mod command;
mod gate;
mod handle;
mod run;

#[cfg(test)]
mod tests;

pub use command::CAPTURE;
use command::{Command, EDIT_POLL, EDITOR_POLL, EDITOR_WAIT, QUICK, coalesce};
pub(crate) use gate::Gate;
use gate::catching;
pub use handle::{Process, Tag};
use run::Run;
