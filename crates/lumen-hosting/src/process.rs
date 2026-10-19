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

enum Command {
    /// A fresh link, at a rate and a longest block, for a new engine or a
    /// new device block size, and with these auxiliary buses on when given
    /// (otherwise the ones it had).
    Relink(f64, u32, Option<Aux>),
    /// Set a parameter (normalized): the plugin's controller is told and the
    /// processor hears it at the start of its next block.
    Set(u32, f64),
    /// Ask the plugin for its text for a value.
    Text(u32, f64),
    /// Ask the plugin for its text for each of these values of one parameter
    /// (the steps of a list): every one is answered, none is dropped for a
    /// later one.
    Texts(u32, Vec<f64>),
    /// Ask the plugin to read text the user typed for a parameter.
    Typed(u32, String),
    /// Open the editor window where it was, or where the system puts it.
    OpenEditor(Option<EditorBounds>),
    CloseEditor,
    /// Hide the open editor window without closing it, or show it again.
    ShowEditor(bool),
    /// End the host, as the user chose for a plugin whose window stopped
    /// answering.
    End,
    /// Read the plugin's state now.
    Capture,
    /// Read every parameter's value now.
    Refresh,
    /// Load a state into the plugin (a preset, or the project's state after
    /// an undo): the component's, the controller's when given, then these
    /// parameter values over them.
    Load {
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: Vec<(u32, f64)>,
    },
    /// Take these keys from the plugin's window (none: give them back).
    Keys(Vec<u8>),
    Quit,
}

/// Drops the commands a later one makes pointless: of several values set for
/// one parameter, only the last matters (a drag sends many); of several asks
/// for one parameter's text, the last; of several sets of keys, the last.
/// Order is otherwise kept.
fn coalesce(commands: Vec<Command>) -> Vec<Command> {
    let mut keep = vec![true; commands.len()];
    for (i, c) in commands.iter().enumerate() {
        let later = commands[i + 1..].iter();
        let superseded = match c {
            Command::Set(id, _) => later
                .into_iter()
                .any(|d| matches!(d, Command::Set(other, _) if other == id)),
            Command::Text(id, _) => later
                .into_iter()
                .any(|d| matches!(d, Command::Text(other, _) if other == id)),
            Command::Keys(_) => later.into_iter().any(|d| matches!(d, Command::Keys(_))),
            Command::Refresh => later.into_iter().any(|d| matches!(d, Command::Refresh)),
            Command::ShowEditor(_) => later
                .into_iter()
                .any(|d| matches!(d, Command::ShowEditor(_) | Command::CloseEditor)),
            _ => false,
        };
        keep[i] = !superseded;
    }
    commands
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect()
}

/// How often a running host is asked for the edits made inside it.
const EDIT_POLL: Duration = Duration::from_millis(25);
/// How long an ask for edits or text may take; a miss is only a miss (the
/// audio side finds a host that has stopped answering).
const QUICK: Duration = Duration::from_millis(300);
/// How often an open editor window is asked where it is.
const EDITOR_POLL: Duration = Duration::from_millis(100);
/// How long a request about the editor window may take. The host answers
/// "not responding" after its own wait (five seconds:
/// `lumen_plugin_host::server::EDITOR_WAIT`), so this only guards a host that
/// is gone.
const EDITOR_WAIT: Duration = Duration::from_secs(8);
/// How long reading a plugin's state may take before the answer is a failure
/// and the project is saved with the state it has (M6 slice 4g).
pub const CAPTURE: Duration = Duration::from_secs(2);

/// A permit to load: only a few plugins load at once, as the scanner's.
pub(crate) struct Gate {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Gate {
    pub(crate) fn new(permits: usize) -> Arc<Self> {
        Arc::new(Self {
            free: Mutex::new(permits),
            freed: Condvar::new(),
        })
    }

    fn take(&self) {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(|e| e.into_inner());
        }
        *free -= 1;
    }

    fn give(&self) {
        *self.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.freed.notify_one();
    }
}

/// Which device, and which start of it, a message is from: the manager drops
/// what a replaced or removed device's old thread still says.
pub type Tag = (u64, u64);

/// The handle to a device's thread. Dropping it asks the host to quit and
/// does not wait.
pub struct Process {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Process {
    /// Starts the thread; its messages go to `out` tagged with `tag`.
    pub(crate) fn start(
        start: Start,
        gate: Arc<Gate>,
        tag: Tag,
        out: Sender<(Tag, Happened)>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let (commands, requests) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("plugin-host".into())
            .spawn(move || {
                Run {
                    aux: Cell::new(start.aux),
                    start,
                    gate,
                    tag,
                    out,
                    wake,
                    requests,
                    ended: Arc::new(AtomicBool::new(false)),
                    counts: Arc::new(Counts::default()),
                }
                .run();
            })?;
        Ok(Self {
            commands,
            thread: Some(thread),
        })
    }

    /// Asks for a fresh link at `rate`, promising the plugin blocks of at
    /// most `frames`.
    pub fn relink(&self, rate: f64, frames: u32) {
        let _ = self.commands.send(Command::Relink(rate, frames, None));
    }

    /// As [`Process::relink`], with these auxiliary buses on.
    pub fn relink_with(&self, rate: f64, frames: u32, aux: Aux) {
        let _ = self.commands.send(Command::Relink(rate, frames, Some(aux)));
    }

    /// Sets a parameter of the plugin (normalized, 0 to 1).
    pub fn set_param(&self, id: u32, value: f64) {
        let _ = self.commands.send(Command::Set(id, value));
    }

    /// Asks the host for the plugin's state; the answer comes as
    /// [`Happened::State`], within the state deadline or as a failure.
    pub fn capture(&self) {
        let _ = self.commands.send(Command::Capture);
    }

    /// Asks for every parameter's value; the answer comes as
    /// [`Happened::Values`].
    pub fn refresh(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    /// Loads a state into the plugin; the answer comes as
    /// [`Happened::Loaded`].
    pub fn load_state(&self, state: Vec<u8>, controller: Option<Vec<u8>>, values: Vec<(u32, f64)>) {
        let _ = self.commands.send(Command::Load {
            state,
            controller,
            values,
        });
    }

    /// Asks for the plugin's text for a value; the answer comes as
    /// [`Happened::Text`].
    pub fn param_text(&self, id: u32, value: f64) {
        let _ = self.commands.send(Command::Text(id, value));
    }

    /// Asks for the plugin's text for each of `values` of one parameter; the
    /// answers come as [`Happened::Text`], one each.
    pub fn param_texts(&self, id: u32, values: Vec<f64>) {
        let _ = self.commands.send(Command::Texts(id, values));
    }

    /// Asks the plugin to read `text` the user typed for a parameter; the
    /// answer comes as [`Happened::Typed`].
    pub fn param_value(&self, id: u32, text: String) {
        let _ = self.commands.send(Command::Typed(id, text));
    }

    /// Opens the plugin's editor window (at `bounds` if given); the answer
    /// comes as [`Happened::Editor`].
    pub fn open_editor(&self, bounds: Option<EditorBounds>) {
        let _ = self.commands.send(Command::OpenEditor(bounds));
    }

    /// Closes the editor window; the answer comes as [`Happened::Editor`].
    pub fn close_editor(&self) {
        let _ = self.commands.send(Command::CloseEditor);
    }

    /// Hides the editor window without closing it (the plugin keeps its view
    /// and its place), or shows it again.
    pub fn show_editor(&self, visible: bool) {
        let _ = self.commands.send(Command::ShowEditor(visible));
    }

    /// Ends the host (a plugin whose window stopped answering); the device
    /// then reads as failed, as any host that stopped.
    pub fn end(&self) {
        let _ = self.commands.send(Command::End);
    }

    /// Takes `keys` (virtual-key codes) from the plugin's window when pressed
    /// alone; each press and release comes as [`Happened::Key`]. Empty gives
    /// them back.
    pub fn capture_keys(&self, keys: Vec<u8>) {
        let _ = self.commands.send(Command::Keys(keys));
    }

    /// Asks the host to quit and waits for the thread, as long as it takes.
    pub fn join(mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Asks the host to quit, without waiting.
    pub fn ask_to_quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }

    /// Whether the thread has ended (its host with it).
    pub fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // The thread quits the host on its own and ends; nothing waits for
        // it here.
        let _ = self.commands.send(Command::Quit);
    }
}

struct Run {
    start: Start,
    gate: Arc<Gate>,
    tag: Tag,
    out: Sender<(Tag, Happened)>,
    wake: Arc<dyn Fn() + Send + Sync>,
    requests: Receiver<Command>,
    ended: Arc<AtomicBool>,
    counts: Arc<Counts>,
    /// The auxiliary buses to turn on at the next set-up.
    aux: Cell<Aux>,
}

/// Where a crashed host said it died. The host writes the line as it goes
/// down and Lumen reads it from the pipe a moment later, so look for it for
/// a short while.
fn crash_detail(host: &HostProcess, error: &HostError) -> Option<String> {
    if !matches!(
        error,
        HostError::Ended(lumen_plugin_host::client::ExitKind::Crashed(_))
    ) {
        return None;
    }
    let log = host.log();
    for _ in 0..20 {
        if let Some(line) = log.crash() {
            return Some(line);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

/// What the thread knows of the editor window between polls.
struct EditorTrack {
    /// Believed open (asked for and not seen closed).
    open: bool,
    /// Opened has been said.
    announced: bool,
    /// Its host did not answer the last time it was asked.
    hung: bool,
    last: Option<EditorBounds>,
    polled: Instant,
}

impl Default for EditorTrack {
    fn default() -> Self {
        Self {
            open: false,
            announced: false,
            hung: false,
            last: None,
            polled: Instant::now(),
        }
    }
}

/// Whether `error` means the host's window code is not answering (as against
/// a refusal or a host that is gone).
fn not_answering(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Timeout
            | HostError::Failed {
                kind: lumen_plugin_host::protocol::FailureKind::Unresponsive,
                ..
            }
    )
}

struct Up {
    host: HostProcess,
    name: String,
    vendor: String,
    generation: u32,
    params: Vec<ParamInfo>,
    params_total: u32,
    units: Vec<UnitInfo>,
    programs: Vec<ProgramList>,
}

impl Run {
    fn say(&self, what: Happened) {
        let _ = self.out.send((self.tag, what));
        (self.wake)();
    }

    fn run(self) {
        self.gate.take();
        let loaded = self.load();
        self.gate.give();
        let mut up = match loaded {
            Ok((up, linked)) => {
                self.say(Happened::Linked(Box::new(linked)));
                up
            }
            Err(failure) => {
                self.say(Happened::Failed(failure));
                return;
            }
        };
        let d = self.start.deadlines;
        let mut polled = Instant::now();
        let mut editor = EditorTrack::default();
        loop {
            let mut commands = match self.requests.recv_timeout(Duration::from_millis(10)) {
                Ok(first) => vec![first],
                Err(RecvTimeoutError::Timeout) => Vec::new(),
                Err(RecvTimeoutError::Disconnected) => vec![Command::Quit],
            };
            while let Ok(more) = self.requests.try_recv() {
                commands.push(more);
            }
            for command in coalesce(commands) {
                match command {
                    Command::Relink(rate, frames, aux) => {
                        if let Some(aux) = aux {
                            self.aux.set(aux);
                        }
                        match self.relink(&mut up, rate, frames) {
                            Ok(linked) => self.say(Happened::Linked(Box::new(linked))),
                            Err(failure) => {
                                self.ended.store(true, Ordering::Release);
                                up.host.kill();
                                self.say(Happened::Failed(failure));
                                return;
                            }
                        }
                    }
                    Command::Set(id, value) => {
                        let _ = up.host.set_param(id, value, QUICK);
                    }
                    Command::Text(id, value) => {
                        if let Ok(text) = up.host.param_text(id, value, QUICK) {
                            self.say(Happened::Text { id, value, text });
                        }
                    }
                    Command::Texts(id, values) => {
                        for value in values {
                            if let Ok(text) = up.host.param_text(id, value, QUICK) {
                                self.say(Happened::Text { id, value, text });
                            }
                        }
                    }
                    Command::Typed(id, text) => {
                        let value = up.host.param_value(id, &text, QUICK).ok();
                        self.say(Happened::Typed { id, text, value });
                    }
                    Command::OpenEditor(bounds) => {
                        if !editor.open {
                            self.open_editor(&mut up, &mut editor, bounds);
                        }
                    }
                    Command::CloseEditor => self.close_editor(&mut up, &mut editor),
                    Command::ShowEditor(visible) => {
                        if editor.open {
                            // A window that does not answer is found out by
                            // the poll; nothing is said here.
                            let _ = up.host.show_editor(visible, EDITOR_WAIT);
                        }
                    }
                    Command::End => {
                        self.ended.store(true, Ordering::Release);
                        up.host.kill();
                        self.say(Happened::Failed(Failure {
                            step: Step::Running,
                            error: HostError::Failed {
                                kind: lumen_plugin_host::protocol::FailureKind::Unresponsive,
                                message:
                                    "the plugin's window stopped answering and the plugin was ended"
                                        .into(),
                            },
                            detail: None,
                        }));
                        return;
                    }
                    Command::Capture => {
                        let state = up
                            .host
                            .state(CAPTURE)
                            .map(|component| Captured {
                                component,
                                // The controller's own state is a courtesy: a
                                // plugin that cannot give it still has a state.
                                controller: up
                                    .host
                                    .controller_state(CAPTURE)
                                    .ok()
                                    .filter(|c| !c.is_empty()),
                            })
                            .map_err(|e| e.to_string());
                        self.say(Happened::State(state));
                    }
                    Command::Refresh => {
                        if let Ok((_, list)) = up.host.parameters(CAPTURE) {
                            self.say(Happened::Values(
                                list.iter().map(|p| (p.id, p.value)).collect(),
                            ));
                        }
                    }
                    Command::Load {
                        state,
                        controller,
                        values,
                    } => {
                        let answer = self.load_state(&mut up, state, controller, &values);
                        self.say(Happened::Loaded(answer));
                    }
                    Command::Keys(keys) => {
                        let _ = up.host.capture_keys(keys, QUICK);
                    }
                    Command::Quit => {
                        up.host.quit(d.quit);
                        return;
                    }
                }
            }
            if polled.elapsed() >= EDIT_POLL && !up.host.has_ended() {
                polled = Instant::now();
                if let Ok(taken) = up.host.take_edits(QUICK)
                    && (!taken.list.is_empty() || taken.dropped > 0 || taken.ignored > 0)
                {
                    self.say(Happened::Edits(taken));
                }
            }
            if editor.open && editor.polled.elapsed() >= EDITOR_POLL && !up.host.has_ended() {
                editor.polled = Instant::now();
                self.watch_editor(&mut up, &mut editor);
            }
            if up.host.has_ended() {
                self.ended.store(true, Ordering::Release);
                let how = up
                    .host
                    .ended()
                    .unwrap_or(lumen_plugin_host::client::ExitKind::Killed);
                let error = HostError::Ended(how);
                let detail = crash_detail(&up.host, &error);
                self.say(Happened::Failed(Failure {
                    step: Step::Running,
                    error,
                    detail,
                }));
                return;
            }
        }
    }

    fn open_editor(&self, up: &mut Up, editor: &mut EditorTrack, bounds: Option<EditorBounds>) {
        match up.host.open_editor(bounds, EDITOR_WAIT) {
            Ok(status) => {
                editor.open = status.open;
                editor.hung = false;
                editor.last = status.bounds;
                if let Some(b) = status.bounds.filter(|_| status.open) {
                    editor.announced = true;
                    self.say(Happened::Editor(EditorEvent::Opened(b)));
                }
            }
            Err(error) if not_answering(&error) => {
                // The window is being made by a plugin that has stopped
                // answering: it is open as far as anyone can say, and the
                // poll finds out when it answers.
                editor.open = true;
                editor.hung = true;
                editor.announced = false;
                self.say(Happened::Editor(EditorEvent::NotResponding));
            }
            Err(error) => self.say(Happened::Editor(EditorEvent::Refused(error.to_string()))),
        }
    }

    fn close_editor(&self, up: &mut Up, editor: &mut EditorTrack) {
        if !editor.open {
            return;
        }
        match up.host.close_editor(EDITOR_WAIT) {
            Ok(status) => {
                editor.open = false;
                editor.hung = false;
                self.say(Happened::Editor(EditorEvent::Closed(
                    status.bounds.or(editor.last),
                )));
            }
            Err(error) if not_answering(&error) => {
                if !editor.hung {
                    editor.hung = true;
                    self.say(Happened::Editor(EditorEvent::NotResponding));
                }
            }
            Err(_) => {}
        }
    }

    /// Asks an open editor where it is, and says what changed.
    fn watch_editor(&self, up: &mut Up, editor: &mut EditorTrack) {
        match up.host.editor_status(EDITOR_WAIT) {
            Ok(status) => {
                if editor.hung {
                    editor.hung = false;
                    self.say(Happened::Editor(EditorEvent::Responding));
                }
                if !status.open {
                    editor.open = false;
                    self.say(Happened::Editor(EditorEvent::Closed(
                        status.bounds.or(editor.last),
                    )));
                    return;
                }
                let Some(b) = status.bounds else {
                    return;
                };
                if !editor.announced {
                    editor.announced = true;
                    editor.last = Some(b);
                    self.say(Happened::Editor(EditorEvent::Opened(b)));
                } else if editor.last != Some(b) {
                    editor.last = Some(b);
                    self.say(Happened::Editor(EditorEvent::Moved(b)));
                }
            }
            Err(error) if not_answering(&error) => {
                if !editor.hung {
                    editor.hung = true;
                    self.say(Happened::Editor(EditorEvent::NotResponding));
                }
            }
            Err(_) => {}
        }
    }

    /// Loads a state into the running plugin and reads every parameter's
    /// value afterwards. A plugin that refuses the state keeps its own.
    fn load_state(
        &self,
        up: &mut Up,
        state: Vec<u8>,
        controller: Option<Vec<u8>>,
        values: &[(u32, f64)],
    ) -> Result<Vec<(u32, f64)>, String> {
        let d = self.start.deadlines;
        up.host
            .set_state(state, d.state)
            .map_err(|e| e.to_string())?;
        if let Some(controller) = controller {
            // The controller's view of its own state is not worth failing for.
            let _ = up.host.set_controller_state(controller, d.state);
        }
        for &(id, value) in values {
            up.host
                .set_param(id, value, d.state)
                .map_err(|e| e.to_string())?;
        }
        let (_, list) = up.host.parameters(d.state).map_err(|e| e.to_string())?;
        Ok(list.iter().map(|p| (p.id, p.value)).collect())
    }

    fn fail(&self, host: &mut HostProcess, step: Step, error: HostError) -> Failure {
        self.ended.store(true, Ordering::Release);
        host.kill();
        let detail = crash_detail(host, &error);
        Failure {
            step,
            error,
            detail,
        }
    }

    fn load(&self) -> Result<(Up, Linked), Failure> {
        let s = &self.start;
        let env: Vec<(&str, &str)> = s
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut host = HostProcess::spawn(&s.host, &env).map_err(|e| Failure {
            step: Step::Start,
            error: HostError::Failed {
                kind: lumen_plugin_host::protocol::FailureKind::Load,
                message: e.to_string(),
            },
            detail: None,
        })?;
        // Keys pressed in the plugin's window go to the app as they come, not
        // at this thread's next look.
        let (out, wake, tag) = (self.out.clone(), self.wake.clone(), self.tag);
        host.on_notice(Box::new(move |news| {
            if let Response::Key { code, down } = news {
                let _ = out.send((tag, Happened::Key { code, down }));
                wake();
            }
        }));
        let class = host
            .load(&s.module, Some(s.class), s.deadlines.load)
            .map_err(|e| self.fail(&mut host, Step::Load, e))?;
        if !s.state.is_empty() {
            host.set_state(s.state.clone(), s.deadlines.state)
                .map_err(|e| self.fail(&mut host, Step::State, e))?;
        }
        if !s.controller_state.is_empty() {
            let _ = host.set_controller_state(s.controller_state.clone(), s.deadlines.state);
        }
        // The parameters are for the interface: a plugin whose controller
        // will not list them still plays. They are listed before the project's
        // own values are set, so their values are what the saved state holds:
        // the baseline an undone change goes back to.
        let (params_total, params) = host
            .parameters(s.deadlines.state)
            .unwrap_or((0, Vec::new()));
        let units = host.units(s.deadlines.state).unwrap_or_default();
        let programs = host.programs(s.deadlines.state).unwrap_or_default();
        for &(id, value) in &s.values {
            host.set_param(id, value, s.deadlines.state)
                .map_err(|e| self.fail(&mut host, Step::State, e))?;
        }
        let mut up = Up {
            host,
            name: class.name,
            vendor: class.vendor,
            generation: 0,
            params,
            params_total,
            units,
            programs,
        };
        let linked = self.configure_and_link(&mut up, s.rate, s.frames)?;
        Ok((up, linked))
    }

    /// Configures at `rate` (a new generation) and opens a link. The plugin
    /// is promised blocks of at most `frames` (the device's), never the
    /// protocol's limit: some plugins work in proportion to it. The link has
    /// room for the engine's longest block; a longer block than promised is
    /// given to the plugin in pieces by the host.
    fn configure_and_link(&self, up: &mut Up, rate: f64, frames: u32) -> Result<Linked, Failure> {
        let s = &self.start;
        let inputs = if s.instrument { 0 } else { 2 };
        let frames = frames.clamp(1, MOST_FRAMES as u32);
        let aux = self.aux.get();
        // The buses the plugin declares; one that will not list them is
        // played on its main buses.
        let buses = up.host.buses(s.deadlines.configure).unwrap_or_default();
        let configured = up
            .host
            .configure_buses(rate, frames, (inputs, 2), aux, s.deadlines.configure)
            .map_err(|e| self.fail(&mut up.host, Step::Configure, e))?;
        let (latency, tail) = (configured.latency, configured.tail);
        up.generation = up.host.generation();
        let block = up
            .host
            .attach(
                Layout::new(MOST_FRAMES as u32, configured.inputs, configured.outputs),
                s.wake,
                s.deadlines.attach,
            )
            .map_err(|e| self.fail(&mut up.host, Step::Attach, e))?;
        let pairs = output_pairs(&buses.1, aux.outputs, configured.outputs);
        Ok(Linked {
            link: Link::new(
                block,
                up.generation,
                tail,
                pairs,
                self.ended.clone(),
                self.counts.clone(),
            ),
            buses,
            aux,
            name: up.name.clone(),
            vendor: up.vendor.clone(),
            latency,
            tail,
            frames,
            params: up.params.clone(),
            params_total: up.params_total,
            units: up.units.clone(),
            programs: up.programs.clone(),
            pid: up.host.id(),
            log: up.host.log(),
        })
    }

    fn relink(&self, up: &mut Up, rate: f64, frames: u32) -> Result<Linked, Failure> {
        self.configure_and_link(up, rate, frames)
    }
}

/// Where the output buses that are on sit in a block: the main bus first (2
/// channels, as Lumen sets it up), then each auxiliary bus of `mask` in bus
/// order. A list that does not add up to the `total` the host carries is
/// played on its main bus alone.
fn output_pairs(buses: &[BusInfo], mask: u32, total: u8) -> Vec<Pair> {
    let mut pairs = vec![Pair {
        first: 0,
        channels: 2,
    }];
    let mut first = 2;
    for bus in (1..buses.len().min(MAX_BUSES)).filter(|bus| mask & (1 << bus) != 0) {
        let channels = usize::from(buses[bus].channels).clamp(1, 2);
        pairs.push(Pair { first, channels });
        first += channels;
    }
    if first == usize::from(total) {
        pairs
    } else {
        vec![Pair {
            first: 0,
            channels: usize::from(total).clamp(1, 2),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_is_worded_short_with_its_cause_first() {
        let failure = Failure {
            step: Step::Load,
            error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
            detail: None,
        };
        assert_eq!(
            failure.to_string(),
            "Crashed while loading: access violation, exception 0xc0000005."
        );
        let located = Failure {
            step: Step::Running,
            error: HostError::Ended(ExitKind::Crashed("exception 0xc0000005".into())),
            detail: Some("in Plug.dll+0x1f2a, reading address 0x0".into()),
        };
        assert_eq!(
            located.to_string(),
            "Crashed while running: access violation, exception 0xc0000005, in Plug.dll+0x1f2a, reading address 0x0."
        );
        let timeout = Failure {
            step: Step::Configure,
            error: HostError::Timeout,
            detail: None,
        };
        assert!(timeout.to_string().contains("did not answer"));
    }

    use std::sync::atomic::AtomicUsize;

    fn describe(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .map(|c| match c {
                Command::Set(id, v) => format!("set {id}={v}"),
                Command::Text(id, v) => format!("text {id}={v}"),
                Command::Texts(id, v) => format!("texts {id} x{}", v.len()),
                Command::Typed(id, t) => format!("typed {id}={t}"),
                Command::OpenEditor(_) => "open editor".into(),
                Command::CloseEditor => "close editor".into(),
                Command::ShowEditor(_) => "show or hide editor".into(),
                Command::End => "end".into(),
                Command::Relink(..) => "relink".into(),
                Command::Capture => "capture".into(),
                Command::Load { .. } => "load".into(),
                Command::Refresh => "refresh".into(),
                Command::Keys(keys) => format!("keys {}", keys.len()),
                Command::Quit => "quit".into(),
            })
            .collect()
    }

    #[test]
    fn a_drag_of_one_parameter_is_one_value_and_the_order_of_the_rest_is_kept() {
        let kept = coalesce(vec![
            Command::Set(1, 0.1),
            Command::Set(2, 0.5),
            Command::Text(1, 0.1),
            Command::Set(1, 0.2),
            Command::Relink(48_000.0, 128, None),
            Command::Text(1, 0.2),
            Command::Set(1, 0.3),
            Command::Keys(vec![1, 2]),
            Command::Keys(vec![]),
            Command::Quit,
        ]);
        assert_eq!(
            describe(&kept),
            [
                "set 2=0.5",
                "relink",
                "text 1=0.2",
                "set 1=0.3",
                "keys 0",
                "quit"
            ]
        );
    }

    #[test]
    fn the_gate_lets_only_its_permits_through_at_once() {
        let gate = Gate::new(2);
        let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (gate, inside, most) = (gate.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    gate.take();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    inside.fetch_sub(1, Ordering::SeqCst);
                    gate.give();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn failures_say_what_was_tried_and_how_it_went() {
        let hung = Failure {
            step: Step::Load,
            error: HostError::Timeout,
            detail: None,
        };
        assert!(hung.hung() && !hung.crashed());
        assert!(hung.to_string().contains("did not answer"));
        assert!(hung.to_string().contains("load the plugin"));
    }
}
