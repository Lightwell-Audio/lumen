//! One plugin device's host process and the thread that owns it (M6 slice 2c).
//!
//! The thread starts the process, loads the plugin, restores its state,
//! configures it and opens the audio link, with a deadline on every step; then
//! it waits for requests (a fresh link for a new engine, quit) and, between
//! them, looks at the process every 10 ms and sets the flag the audio thread
//! reads, so the callback never makes a system call to ask whether a host has
//! ended. A host that hangs blocks only this thread.

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

/// How the audio loop and the host wait for each other. The setting
/// ADR-0069's runs support (inside the budget to 100 µs of plugin work per
/// track); one constant, to re-measure in MT-VST-2.
pub const WAKE: Wake = Wake::EVENT;

/// The step a failure happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    Load,
    State,
    Configure,
    Attach,
    /// The process ended while the project played.
    Running,
}

/// Why a device has no working host.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub step: Step,
    pub error: HostError,
    /// Where the plugin died, when the host could say ("in Name.dll+0x1f2a,
    /// reading address 0x0"): it tells a plugin's fault from the host's.
    pub detail: Option<String>,
}

impl Step {
    /// What the step is, for "crashed while ...".
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
    /// Whether the plugin took its host down (a crash) rather than refusing.
    pub fn crashed(&self) -> bool {
        matches!(
            self.error,
            HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
        )
    }

    /// Whether it stopped answering.
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
            // The cause first and short: it has to fit on a card.
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

/// How long each step may take before the host is given up on.
#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    pub load: Duration,
    pub state: Duration,
    pub configure: Duration,
    pub attach: Duration,
    /// To quit when asked, before the host is killed.
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

/// What to start for one device.
#[derive(Debug, Clone)]
pub struct Start {
    /// The `lumen-plugin-host` program.
    pub host: PathBuf,
    /// The plugin's module, and its class in it.
    pub module: PathBuf,
    pub class: [u8; 16],
    /// An instrument takes no audio in; an effect takes a stereo pair.
    pub instrument: bool,
    /// The saved component state, restored at load; empty for none.
    pub state: Vec<u8>,
    /// The saved edit-controller state, restored after the component's; empty
    /// for none. A controller that will not take it leaves the plugin as it
    /// is: its own view settings are not worth a plugin that does not start.
    pub controller_state: Vec<u8>,
    /// Parameter values set after the state is restored, as `(id,
    /// normalized value)`: what the project holds over the saved state.
    pub values: Vec<(u32, f64)>,
    pub rate: f64,
    /// The longest block the plugin is promised: the device's block, at most
    /// the engine's [`MOST_FRAMES`]. Some plugins size their work by it on
    /// every call, so it is the device's, not a generous ceiling (ADR-0075).
    pub frames: u32,
    /// Added to the host's environment (the test plugin's faults).
    pub env: Vec<(String, String)>,
    pub deadlines: Deadlines,
    /// How the two sides wait for each other: [`WAKE`] unless a test asks
    /// for another.
    pub wake: Wake,
    /// The auxiliary buses to turn on (extra outputs, a sidechain input);
    /// none unless the project asks.
    pub aux: Aux,
}

impl Start {
    /// A plugin with no saved state, [`Deadlines::default`] and [`WAKE`].
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

/// What the thread tells the app.
#[derive(Debug)]
pub enum Happened {
    /// The host is running and has a link for the engine to play it through.
    Linked(Box<Linked>),
    /// The device has no host and will not get one without a reload.
    Failed(Failure),
    /// The plugin's parameters changed from inside (its own window, or the
    /// processor reporting a value), or events it has no parameter for were
    /// dropped. Sent only when there is something to say.
    Edits(Taken),
    /// Something happened to the plugin's editor window.
    Editor(EditorEvent),
    /// The plugin's own text for a value that was asked for.
    Text { id: u32, value: f64, text: String },
    /// What the plugin made of text the user typed for a parameter: its
    /// normalized value, or `None` when it did not understand.
    Typed {
        id: u32,
        text: String,
        value: Option<f64>,
    },
    /// The plugin's saved state, as asked for ([`Process::capture`]): the
    /// bytes, or why they could not be read in time.
    State(Result<Captured, String>),
    /// Every parameter's value now, as asked for ([`Process::refresh`]): what
    /// a plugin changed inside itself without saying which (a patch loaded in
    /// its window, a program change).
    Values(Vec<(u32, f64)>),
    /// A state loaded into the plugin ([`Process::load_state`]): the value of
    /// every parameter now, or why the plugin would not take it (it keeps the
    /// sound it had).
    Loaded(Result<Vec<(u32, f64)>, String>),
    /// A key Lumen asked for ([`Process::capture_keys`]) went down or up in
    /// the plugin's window (a virtual-key code).
    Key { code: u8, down: bool },
}

/// A plugin's state as read: the component's, and the edit controller's when
/// it keeps any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub component: Vec<u8>,
    pub controller: Option<Vec<u8>>,
}

/// What happened to a plugin's editor window (M6 slice 4f).
#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    /// The window is open, here.
    Opened(EditorBounds),
    /// The window was moved or resized by the user or the plugin.
    Moved(EditorBounds),
    /// The window is gone: Lumen closed it or the user did. Where it was last.
    Closed(Option<EditorBounds>),
    /// The window stopped answering: the plugin is stuck in its own window
    /// code. The user decides whether to end the plugin.
    NotResponding,
    /// The window answers again.
    Responding,
    /// The editor could not be opened, and why.
    Refused(String),
}

pub struct Linked {
    pub link: Link,
    pub name: String,
    pub vendor: String,
    /// What the plugin reports, in frames at the rate it was configured at.
    pub latency: u32,
    pub tail: u32,
    /// The longest block the plugin was promised.
    pub frames: u32,
    /// The plugin's parameters as they were when it loaded (capped), the
    /// plugin's own count, and the groups they sit in.
    pub params: Vec<ParamInfo>,
    pub params_total: u32,
    pub units: Vec<UnitInfo>,
    /// The programs the plugin offers, with the parameter that selects each
    /// list's program.
    pub programs: Vec<ProgramList>,
    /// The audio buses the plugin declares, inputs then outputs, and which
    /// auxiliary ones are on.
    pub buses: (Vec<BusInfo>, Vec<BusInfo>),
    pub aux: Aux,
    pub pid: u32,
    /// What the host says about itself: how long its first blocks took, and
    /// where it died if it does. For the card when the plugin is given up on.
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
