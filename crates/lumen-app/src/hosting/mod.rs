//! Plugin devices in the app (M6 slice 2c, ADR-0071): which devices of the
//! project want a host process, what became of each, and how a running host
//! reaches the engine. The processes and their threads are
//! `lumen_hosting::Manager`; this is the bridge between it, the project, the
//! catalog and the engine's handles. Nothing here blocks a frame: the loop
//! calls [`Hosting::update`] and the host threads' news arrives as it is sent.
//!
//! - An **effect** is named in the routing for as long as the project holds it
//!   (`Device::Hosted`), at its last known latency, so a loading, failed or
//!   missing effect plays the engine's fallback line and timing never shifts
//!   (the owner's D1 and D7). A running host is attached to the mixer by the
//!   device's id.
//! - An **instrument** is a placeholder in every MIDI track list; the list
//!   built after the host loads carries the real one, and the engine keeps
//!   it across later lists (ADR-0070).
//! - The latency and tail a host reports are stamped into the project when it
//!   is saved, as the view is (owner's C2, refined: not an edit, so opening a
//!   project and hearing it does not make it unsaved).

mod capture;
mod editor;
mod outputs;
mod parameters;
mod presets;
mod pretend;
mod update;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use self::outputs::{output_mask, sidechain_mask};
use lumen_devices::{Cost, Guard, HostedInstrument, Status};
use lumen_engine::{InstrumentSpec, MidiTrack, MixerHandle};
use lumen_hosting::{
    Aux, BusInfo, Captured, Edit, EditKind, EditorBounds, EditorEvent, Happened, HostLog,
    MAX_CHANNELS, Manager, ParamInfo, ProgramList, Start, UnitInfo,
};
use lumen_plugin_catalog::catalog::Catalog;
use lumen_project::model::{
    Device, DeviceKind, Id, PluginRef, Project, Reported, Track, TrackKind,
};
use lumen_project::routing::all_tracks;

/// Why a running plugin was given up on, with what its host recorded: the
/// first blocks and the slow ones took this long, which says a plugin that is
/// slow from a plugin that never answers.
fn gave_up_words(log: Option<&HostLog>) -> String {
    let timings: Vec<String> = log
        .map(|log| {
            log.lines()
                .into_iter()
                .filter(|l| l.starts_with("block "))
                .collect()
        })
        .unwrap_or_default();
    let base = "The plugin stopped answering, and its host was ended.";
    match timings.len() {
        0 => format!("{base} It never finished a block it was given. Reload to start it again."),
        _ => {
            let last: Vec<&str> = timings
                .iter()
                .rev()
                .take(3)
                .rev()
                .map(String::as_str)
                .collect();
            format!(
                "{base} Its blocks: {}. Reload to start it again.",
                last.join("; ")
            )
        }
    }
}

/// The engine's instrument kind for a plugin host: tracks keep it across
/// track lists while the kind stays the same (the synth is 1).
pub const HOSTED_KIND: u32 = 2;

/// What became of a plugin device's host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Its host is starting and loading the plugin.
    Loading,
    /// Running and attached.
    Ready,
    /// Crashed, hung or refused; the words say what and how to go on.
    Failed(String),
    /// The plugin is not installed (or plugins are off).
    Missing,
    /// Safe mode: no plugin is loaded, and this one has not been asked for. Its
    /// settings are kept as the project holds them.
    Safe,
}

struct Tracked {
    track: Id,
    class: String,
    instrument: bool,
    state: State,
    status: Option<Arc<Status>>,
    /// What the host said about itself (block timings, where it died).
    log: Option<Arc<HostLog>>,
    /// What the plugin reported, in frames at `rate`, and the rate.
    latency: u32,
    tail: u32,
    rate: u32,
    /// The longest block its plugin was promised (0 until linked).
    frames: u32,
    bypass: bool,
    params: Parameters,
    editor: EditorState,
    /// When its host last came up. What a plugin says about its own state
    /// changing in the first moments after is its loading, not the user.
    linked_at: Option<std::time::Instant>,
    /// Its window was hidden (asked for; it stays open): the user is on
    /// another track.
    editor_hidden: bool,
    /// Being set up again (a new rate or block size, or the plugin asked):
    /// the link it plays through now misses blocks until the new one comes,
    /// which is not the plugin failing.
    relinking: bool,
    /// The audio inputs the plugin declares, and the auxiliary buses its
    /// link has on (M6 slice 6c): a sidechain turns one on.
    inputs: Vec<BusInfo>,
    /// The audio outputs it declares (an instrument's other outputs).
    outputs: Vec<BusInfo>,
    aux: Aux,
    /// When its host failed, most recent last, over the last couple of
    /// minutes: a plugin that keeps stopping is told apart from one that
    /// stopped once. Nothing restarts it: the person does, from its card.
    failures: Vec<std::time::Instant>,
}

/// What Lumen knows of a plugin's own window (M6 slice 4f).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EditorState {
    #[default]
    Closed,
    /// Asked for, not shown yet.
    Opening,
    Open,
    /// The plugin is stuck in its window code. Never ended on its own.
    NotResponding,
    /// The window cannot be had, and why (the plugin has none, or this
    /// platform cannot show one).
    Refused(String),
}

/// What Lumen knows of a plugin's parameters while its host runs (M6 slice 4).
#[derive(Default)]
struct Parameters {
    /// The plugin's parameters, capped, as they were when it loaded, with
    /// their values in the saved state.
    list: Vec<ParamInfo>,
    /// The plugin's own count, which may be more than the list.
    total: u32,
    units: Vec<UnitInfo>,
    /// The value each parameter has in the plugin as far as Lumen knows: the
    /// saved state's, then what Lumen set and what the plugin did inside.
    values: BTreeMap<u32, f64>,
    /// The value a parameter had before the project first held one for it:
    /// what an undone change goes back to.
    base: BTreeMap<u32, f64>,
    /// The values the project held when the host was started.
    started_with: BTreeMap<u32, f64>,
    /// The plugin's own text for a value, last asked for, by parameter.
    texts: HashMap<u32, (f64, String)>,
    /// The plugin's own word for each step of a stepped parameter, by
    /// parameter (a chooser's list), as far as it has said them.
    choices: HashMap<u32, Vec<String>>,
    /// The parameters whose steps have been asked about.
    choices_asked: std::collections::HashSet<u32>,
    /// What the plugin made of text the user typed, waiting for the document:
    /// the parameter, the text, and the value or `None` when it did not
    /// understand.
    typed: Vec<(u32, String, Option<f64>)>,
    /// Edits the plugin made inside itself, waiting for the document, each
    /// with whether it came inside a gesture (between a begin and an end).
    edits: Vec<(Edit, bool)>,
    /// The parameter whose gesture is open, since when. A gesture that never
    /// ends is let go of after [`GESTURE_LIMIT`].
    open: Option<(u32, std::time::Instant)>,
    /// Controller, bend and pressure events no parameter took, in all.
    ignored: u32,
    /// Values Lumen has sent the plugin to match the project (a plugin's own
    /// edits need none).
    sent: u32,
    /// Whether the host has linked before: a fresh link of the same process
    /// keeps what is known.
    linked_before: bool,
    /// Whether the plugin's state is ahead of what the project saved.
    dirty: bool,
    /// How many times it has changed, and when last: a state read after the
    /// last change is the plugin's state now.
    edit_count: u64,
    last_edit: Option<std::time::Instant>,
    /// The plugin's state as last read, and the edit count it was read at. A
    /// read is never replaced by a failed one: this is the last good state, the
    /// one a plugin that fails starts again from.
    captured: Option<Captured>,
    /// When it was read, and the plugin's values when it was asked for.
    captured_when: Option<std::time::Instant>,
    captured_values: BTreeMap<u32, f64>,
    /// The values as they were when the read in flight was asked for.
    capturing_values: Option<BTreeMap<u32, f64>>,
    /// When the plugin was last read for a checkpoint (or loaded).
    last_checkpoint: Option<std::time::Instant>,
    captured_at: u64,
    /// A read in flight, started at this edit count.
    capturing: Option<u64>,
    /// The last read failed (hung, crashed, refused); not tried again until
    /// the plugin changes again.
    capture_failed: bool,
    /// The values the project holds for the device, as last seen.
    held: BTreeMap<u32, f64>,
    /// The programs the plugin offers (M6 slice 5).
    programs: Vec<ProgramList>,
    /// The revision of the sound the plugin has, as the project counts them
    /// (`Device::state_rev`): another revision in the project means a state
    /// was loaded, or an undo or redo went across one, and the plugin is given
    /// the project's state.
    state_rev: u32,
    /// States sent to the plugin, answers not yet in, oldest first.
    loads: std::collections::VecDeque<Load>,
    /// A preset the plugin refused, to be given to it again when the time
    /// comes: a plugin still loading its content may refuse a state it takes
    /// a moment later.
    retry: Option<(std::time::Instant, Load)>,
    /// What the plugin does inside itself shortly after a state is loaded is
    /// the loading, not the user: its edits are not taken until this time.
    quiet_until: Option<std::time::Instant>,
    /// A program the user chose, by name, and when: the edits the plugin makes
    /// in answer are one undo step with the choice.
    program: Option<(String, std::time::Instant)>,
    /// The choice has not been sent to the plugin yet: what the plugin sets in
    /// answer to it is wanted, unlike its answer to any later value of ours.
    program_fresh: bool,
}

/// A state sent to a plugin whose answer is awaited.
struct Load {
    state: Captured,
    /// `Some` for a preset the user chose: the project is edited once the
    /// plugin has taken it (and not if it refuses). `None` for the project's
    /// own state sent again after an undo or redo.
    preset: Option<String>,
    /// How many times the plugin has refused it already.
    tries: u8,
}

/// What became of a preset the user chose to load.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetNews {
    /// The plugin took it: the project can hold it now.
    Ready {
        device: Id,
        name: String,
        component: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    /// The plugin refused it, and has the sound it had.
    Refused {
        device: Id,
        name: String,
        why: String,
    },
}

impl Parameters {
    /// The plugin has changed: its state read before is out of date.
    fn touch(&mut self) {
        self.dirty = true;
        self.edit_count += 1;
        self.last_edit = Some(std::time::Instant::now());
        self.capture_failed = false;
    }

    /// Whether the state read last is the plugin's state now.
    fn fresh(&self) -> bool {
        self.captured.is_some() && self.captured_at == self.edit_count
    }

    /// Whether Lumen may set the parameter: not an output the plugin reports
    /// and not its bypass.
    fn settable(&self, id: u32) -> bool {
        self.list
            .iter()
            .find(|i| i.id == id)
            .is_none_or(|i| !i.read_only() && !i.bypass())
    }

    /// The values a plugin started again from its last good state needs set
    /// over it: what the project holds, and every value that moved since that
    /// state was read (or since the plugin loaded, when none was), by the
    /// plugin's own doing or Lumen's.
    fn catch_up(&self, held: &BTreeMap<u32, f64>) -> BTreeMap<u32, f64> {
        let then = if self.captured.is_some() {
            &self.captured_values
        } else {
            &self.base
        };
        let mut all = held.clone();
        for (&id, &value) in &self.values {
            if self.settable(id) && !all.contains_key(&id) && then.get(&id) != Some(&value) {
                all.insert(id, value);
            }
        }
        all
    }

    /// Whether the user holds a control in the plugin's window, or has just
    /// chosen a program: not a moment to read the plugin.
    fn gesture_is_open(&self) -> bool {
        self.open
            .is_some_and(|(_, since)| since.elapsed() < GESTURE_LIMIT)
            || self
                .program
                .as_ref()
                .is_some_and(|(_, since)| since.elapsed() < PROGRAM_WINDOW)
    }

    /// Notes that a read is asked for now.
    fn asked(&mut self) {
        self.capturing = Some(self.edit_count);
        self.capturing_values = Some(self.values.clone());
    }
}

/// The most steps a parameter may have to be shown as a list of choices; a
/// parameter with more is a knob, and its steps are not asked for.
pub const MOST_CHOICES: i32 = 64;

/// How many more times a preset a plugin refused is given to it, and how long
/// after each refusal (a plugin that is still loading its content may take
/// what it refused a moment before).
const PRESET_RETRIES: u8 = 2;
const PRESET_RETRY_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// How long a plugin is left alone after its last change before its state
/// is read (a drag is many changes; the state is read when it rests).
const REST: std::time::Duration = std::time::Duration::from_millis(600);

/// How long after a plugin's host comes up that what it says about its own
/// state changing is taken as its loading.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

/// What a plugin made of text the user typed for one of its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub device: Id,
    pub id: u32,
    pub text: String,
    /// The normalized value, or `None` when the plugin did not understand.
    pub value: Option<f64>,
}

/// The edits one device's plugin made inside itself, for the document.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEdits {
    pub device: Id,
    /// In the order the plugin made them; `true` when inside a gesture.
    pub edits: Vec<(Edit, bool)>,
}

/// The longest closing Lumen waits for plugin hosts to quit.
const EXIT_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// How often a running plugin is read for a checkpoint.
const CHECKPOINT: std::time::Duration = std::time::Duration::from_secs(30);

/// A plugin that fails this many times within [`FAILURE_WINDOW`] keeps failing.
const FAILURES_THAT_COUNT: usize = 3;
const FAILURE_WINDOW: std::time::Duration = std::time::Duration::from_secs(120);

/// Notes a failure now, forgetting the ones too old to count. A second word
/// about the same failure (the engine gave up on a host that then reported its
/// end) is not another failure.
fn note_failure(failures: &mut Vec<std::time::Instant>) {
    let now = std::time::Instant::now();
    failures.retain(|at| at.elapsed() < FAILURE_WINDOW);
    if failures
        .last()
        .is_none_or(|at| now.duration_since(*at) > std::time::Duration::from_secs(1))
    {
        failures.push(now);
    }
}

/// How long what a plugin does inside itself after a state was loaded into it
/// is taken as the loading.
const QUIET: std::time::Duration = std::time::Duration::from_millis(400);

/// How long after a program was chosen the plugin's answering edits belong to
/// the choice.
const PROGRAM_WINDOW: std::time::Duration = std::time::Duration::from_millis(1500);

/// How long a plugin's gesture may stay open with nothing in it before it is
/// taken as ended (a plugin that forgets its end must not hold undo steps
/// open for ever).
const GESTURE_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a running plugin's blocks cost over a stretch of time, for the
/// audio panel's budget bar (ADR-0075).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginCost {
    pub device: Id,
    /// The plugin's name.
    pub name: String,
    /// Where it is: its track, and its slot there ("Bass · effect 2").
    pub place: String,
    pub cost: Cost,
    /// Blocks it missed since its host started.
    pub late: u32,
}

/// What a frame's update changed outside the hosting itself.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    /// A running instrument is ready for the next MIDI track list.
    pub midi: bool,
    /// Something is waiting out a delay (a plugin's state is to be read once
    /// it has rested): ask for a frame this many milliseconds from now.
    pub again_in_ms: Option<u64>,
}

pub struct Hosting {
    manager: Option<Manager>,
    host: Option<PathBuf>,
    /// Tests give a module's host a fault chosen by the module's name.
    faults: Option<crate::plugins::Faults>,
    tracked: HashMap<u64, Tracked>,
    /// Real instruments waiting for the next track list, by track.
    instruments: HashMap<u64, InstrumentSpec>,
    /// Effects the mixer's queue had no room for.
    waiting: Vec<(u64, Guard)>,
    rate: u32,
    /// The longest block plugins are promised: the device's (ADR-0075).
    frames: u32,
    /// Where editor windows were last seen, by plugin class, waiting for the
    /// app to remember them.
    places: Vec<(String, EditorBounds)>,
    /// A plugin changed inside, waiting for the app to say so.
    inside_changes: bool,
    /// Keys pressed and let go of in plugins' windows (virtual-key codes,
    /// `true` for down), oldest first, waiting for the computer keyboard.
    keys: Vec<(u8, bool)>,
    /// Presets the plugins took or refused, waiting for the document.
    presets: Vec<PresetNews>,
    /// How often a running plugin is read for a checkpoint.
    checkpoint_every: std::time::Duration,
    /// Safe mode: no plugin is loaded, except those asked for one at a time.
    safe: bool,
    allowed: std::collections::HashSet<u64>,
}

/// The host's stand-in for an editor window, as a test asks for it in the
/// plugin's fault list (`headless_editor`, `headless_hang=MS`,
/// `headless_close=MS`, `headless_press=CODE`): the value of
/// `LUMEN_HOST_HEADLESS_EDITOR`.
fn headless_setting(fault: &str) -> Option<String> {
    let parts: Vec<String> = fault
        .split(',')
        .map(str::trim)
        .filter_map(|entry| match entry.split_once('=') {
            None if entry == "headless_editor" => Some("on".to_owned()),
            Some(("headless_hang", ms)) => Some(format!("hang={ms}")),
            Some(("headless_close", ms)) => Some(format!("close={ms}")),
            Some(("headless_press", code)) => Some(format!("press={code}")),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(","))
}

/// How long a stand-in window may keep its host's main thread before the host
/// says it is not responding, in tests: 600 ms rather than the system's five
/// seconds (`LUMEN_HOST_EDITOR_WAIT_MS`).
const TEST_EDITOR_WAIT_MS: &str = "600";

/// The longest block to promise plugins for a device block of `block`
/// frames (0 when no stream is open): the device's, at most what the engine
/// ever hands a plugin.
pub fn promised_frames(block: u32) -> u32 {
    let most = lumen_devices::MOST_FRAMES as u32;
    if block == 0 { most } else { block.min(most) }
}

/// What a device needs of the project to get a host.
struct Wanted<'a> {
    track: &'a Track,
    device: &'a Device,
    class: &'a str,
    instrument: bool,
}

fn wanted(project: &Project) -> Vec<Wanted<'_>> {
    let mut list = Vec::new();
    for track in all_tracks(project) {
        let mut instrument_seen = false;
        for device in &track.devices {
            let lumen_project::model::PluginRef::Vst3 { class_id, .. } = &device.plugin else {
                continue;
            };
            let instrument = device.kind() == DeviceKind::Instrument;
            // An instrument plays on a MIDI track, one to a track; anything
            // else was never offered (edit time refuses it).
            if instrument && (track.kind != TrackKind::Midi || instrument_seen) {
                continue;
            }
            instrument_seen |= instrument;
            list.push(Wanted {
                track,
                device,
                class: class_id,
                instrument,
            });
        }
    }
    list
}

impl Hosting {
    /// No plugin hosting at all (`--plugins off`; most tests): devices are
    /// `Missing`, and effects still keep their time.
    pub fn off() -> Self {
        Self {
            manager: None,
            host: None,
            faults: None,
            tracked: HashMap::new(),
            instruments: HashMap::new(),
            waiting: Vec::new(),
            rate: 0,
            frames: promised_frames(0),
            places: Vec::new(),
            inside_changes: false,
            keys: Vec::new(),
            presets: Vec::new(),
            checkpoint_every: CHECKPOINT,
            safe: false,
            allowed: Default::default(),
        }
    }

    /// Hosts started from the program `host`. `wake` is called from a host
    /// thread when it has news (a repaint).
    pub fn new(
        host: PathBuf,
        faults: Option<crate::plugins::Faults>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            manager: Some(Manager::new(wake)),
            host: Some(host),
            faults,
            ..Self::off()
        }
    }

    /// The plugin's class id as Lumen keys it, for what is remembered per
    /// plugin.
    pub fn class(&self, device: Id) -> Option<&str> {
        self.tracked.get(&device.0).map(|t| t.class.as_str())
    }

    pub fn state(&self, device: Id) -> Option<&State> {
        self.tracked.get(&device.0).map(|t| &t.state)
    }

    /// The longest block the device's plugin was promised, once linked.
    pub fn promised(&self, device: Id) -> Option<u32> {
        self.tracked
            .get(&device.0)
            .map(|t| t.frames)
            .filter(|&f| f != 0)
    }

    /// Frames the plugin holds back, as it reported (at the current rate).
    pub fn latency(&self, device: Id) -> Option<u32> {
        self.tracked.get(&device.0).map(|t| t.latency)
    }

    /// Starts or ends safe mode. In safe mode no plugin binary is loaded or
    /// hosted: each device is [`State::Safe`], keeping its settings and (for an
    /// effect) its time. Ending it lets every device start, as it would have.
    pub fn set_safe(&mut self, safe: bool) {
        self.safe = safe;
        if !safe {
            self.allowed.clear();
        }
    }

    /// Asks for one device's plugin to be loaded although safe mode is on.
    pub fn allow(&mut self, device: Id) {
        self.allowed.insert(device.0);
    }

    /// Whether any plugin is being loaded now.
    pub fn loading_any(&self) -> bool {
        self.tracked.values().any(|t| t.state == State::Loading)
    }

    /// The plugin devices of `project` that are not running and are not
    /// loading, with how: "Bass · Name (failed)". Missing and safe-mode ones
    /// count; the callback benchmark will not measure a project that is not
    /// all there.
    pub fn not_running(&self, project: &Project) -> Vec<String> {
        let mut found = Vec::new();
        for track in all_tracks(project) {
            for device in &track.devices {
                let PluginRef::Vst3 { name, .. } = &device.plugin else {
                    continue;
                };
                let how = match self.tracked.get(&device.id.0).map(|t| &t.state) {
                    Some(State::Ready) | Some(State::Loading) => continue,
                    Some(State::Failed(_)) => "failed",
                    Some(State::Safe) => "not loaded in safe mode",
                    Some(State::Missing) | None => "not installed",
                };
                found.push(format!("{} · {name} ({how})", track.name));
            }
        }
        found
    }

    /// How many plugin hosts exist (running, loading or failed).
    pub fn host_count(&self) -> usize {
        self.manager.as_ref().map_or(0, Manager::len)
    }

    /// Whether the plugin has stopped three times in the last two minutes: it
    /// keeps stopping, and the card says so rather than offering the same
    /// Reload as for one that stopped once.
    pub fn keeps_failing(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| {
            t.failures
                .iter()
                .filter(|at| at.elapsed() < FAILURE_WINDOW)
                .count()
                >= FAILURES_THAT_COUNT
        })
    }

    /// How long ago the plugin's last good state was read, if one was.
    pub fn last_good_age(&self, device: Id) -> Option<std::time::Duration> {
        self.tracked
            .get(&device.0)?
            .params
            .captured_when
            .map(|at| at.elapsed())
    }

    /// Changes how often running plugins are read for a checkpoint (tests).
    pub fn set_checkpoint_every(&mut self, every: std::time::Duration) {
        self.checkpoint_every = every;
    }

    /// Starts a new host for a failed device (the card's Reload), from the last
    /// good state it was read at and the values that moved since.
    pub fn reload(&mut self, device: Id) {
        self.restart_failed(device, false);
    }

    /// Starts a new host for a failed device with no sound of its own (the
    /// card's "Reload without sound", for a plugin whose last good state is
    /// what keeps it failing): the plugin as it was added, with the values the
    /// project holds over it. The project's saved state is left alone until the
    /// next save writes what the plugin is then.
    pub fn reload_clean(&mut self, device: Id) {
        self.restart_failed(device, true);
    }

    fn restart_failed(&mut self, device: Id, clean: bool) {
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        if let Some(t) = self.tracked.get_mut(&device.0)
            && matches!(t.state, State::Failed(_))
        {
            let (state, values) = if clean {
                (
                    Some(Captured {
                        component: Vec::new(),
                        controller: None,
                    }),
                    t.params.held.clone(),
                )
            } else {
                (t.params.captured.clone(), t.params.catch_up(&t.params.held))
            };
            let list = values.iter().map(|(&id, &v)| (id, v)).collect();
            match manager.reload_from(device.0, state, list) {
                Ok(true) => {
                    t.state = State::Loading;
                    t.status = None;
                    t.log = None;
                    t.params = Parameters {
                        started_with: values,
                        held: t.params.held.clone(),
                        state_rev: t.params.state_rev,
                        ..Parameters::default()
                    };
                }
                Ok(false) => {}
                // The host could not be started at all (no thread to run it
                // on): the card says so, and Reload is still there.
                Err(e) => {
                    t.state = State::Failed(format!("The plugin host could not be started: {e}"));
                }
            }
        }
    }

    /// Ends every host; when Lumen closes. Waits for them no longer than
    /// [`EXIT_GRACE`]: a plugin still loading cannot hear it, and its host
    /// ends with Lumen's process.
    pub fn shutdown(&mut self) {
        if let Some(manager) = self.manager.as_mut() {
            manager.shutdown(EXIT_GRACE);
        }
        self.tracked.clear();
        self.instruments.clear();
        self.waiting.clear();
    }

    /// Starts a host for `w`, and says how that went so far.
    fn start(&mut self, w: &Wanted<'_>, catalog: &Catalog, rate: u32, frames: u32) -> State {
        // Safe mode loads nothing: not even to find out whether the plugin is
        // there. Only the ones asked for, one at a time, are started.
        if self.safe && !self.allowed.contains(&w.device.id.0) {
            return State::Safe;
        }
        let (Some(manager), Some(host)) = (self.manager.as_mut(), self.host.as_ref()) else {
            return State::Missing;
        };
        let Some(module) = catalog
            .find(&format!("vst3:{}", w.class.to_ascii_uppercase()))
            .and_then(|d| d.module.clone())
        else {
            return State::Missing;
        };
        let Some(class) = lumen_hosting::parse_class_id(w.class) else {
            return State::Missing;
        };
        if rate == 0 {
            // Not known yet (no stream): try again next frame.
            return State::Missing;
        }
        let mut start = Start::new(
            host.clone(),
            module.clone(),
            class,
            w.instrument,
            f64::from(rate),
        );
        start.state = w.device.state.clone();
        start.controller_state = w.device.controller_state.clone();
        start.values = w.device.params.iter().map(|(&id, &v)| (id, v)).collect();
        start.frames = frames;
        if let Some(faults) = self.faults
            && let Some(stem) = module.file_stem()
            && let Some(fault) = faults(&stem.to_string_lossy())
        {
            start
                .env
                .push(("LUMEN_TEST_PLUGIN_FAULT".into(), fault.into()));
            if let Some(headless) = headless_setting(fault) {
                start
                    .env
                    .push(("LUMEN_HOST_HEADLESS_EDITOR".into(), headless));
                start.env.push((
                    "LUMEN_HOST_EDITOR_WAIT_MS".into(),
                    TEST_EDITOR_WAIT_MS.into(),
                ));
            }
        }
        match manager.want(w.device.id.0, start) {
            Ok(()) => State::Loading,
            Err(e) => State::Failed(format!("The plugin host could not be started: {e}")),
        }
    }

    /// The instrument for a MIDI track's plugin: the running one, once, or a
    /// stand-in that plays nothing (the engine keeps the running one it
    /// already has across lists).
    pub fn instrument(&mut self, track: Id) -> InstrumentSpec {
        self.instruments.remove(&track.0).unwrap_or_else(stand_in)
    }

    /// Takes back the running instruments of a track list the engine had no
    /// room for, so the next list carries them.
    pub fn reclaim(&mut self, tracks: Vec<MidiTrack>) {
        for track in tracks {
            if let Some(spec) = track.instrument
                && spec.instrument.hosted()
            {
                self.instruments.insert(track.id, spec);
            }
        }
    }

    /// What every running plugin's blocks cost since the last call, in the
    /// project's order. Each call starts a fresh stretch.
    pub fn take_costs(&self, project: &Project) -> Vec<PluginCost> {
        let mut costs = Vec::new();
        for track in all_tracks(project) {
            let mut effects = 0;
            for device in &track.devices {
                let instrument = device.kind() == DeviceKind::Instrument;
                if !instrument {
                    effects += 1;
                }
                let Some(status) = self
                    .tracked
                    .get(&device.id.0)
                    .filter(|t| t.state == State::Ready)
                    .and_then(|t| t.status.as_ref())
                else {
                    continue;
                };
                let slot = if instrument {
                    "instrument".to_string()
                } else {
                    format!("effect {effects}")
                };
                let name = match &device.plugin {
                    PluginRef::Vst3 { name, .. } | PluginRef::Builtin { name } => name.clone(),
                };
                costs.push(PluginCost {
                    device: device.id,
                    name,
                    place: format!("{} · {slot}", track.name),
                    cost: status.take_cost(),
                    late: status.late_blocks(),
                });
            }
        }
        costs
    }
}

impl Parameters {
    /// The values to send so the plugin matches what the project holds: every
    /// value the project has that the plugin does not, and, for a parameter
    /// the project let go of (an undone change), the value it had before.
    fn differences(&self, held: &BTreeMap<u32, f64>) -> Vec<(u32, f64)> {
        let mut send: Vec<(u32, f64)> = held
            .iter()
            .filter(|&(id, v)| self.values.get(id) != Some(v))
            .map(|(&id, &v)| (id, v))
            .collect();
        for (&id, &base) in &self.base {
            if !held.contains_key(&id) && self.values.get(&id).is_some_and(|&v| v != base) {
                send.push((id, base));
            }
        }
        send
    }
}

/// An instrument slot for a plugin that is not attached: silent.
pub fn stand_in() -> InstrumentSpec {
    InstrumentSpec {
        kind: HOSTED_KIND,
        preset: 0,
        instrument: Box::new(HostedInstrument::placeholder()),
    }
}

#[cfg(test)]
mod tests;

/// TEMP TRACE (remove before merge): appends a line to the file named by
/// LUMEN_FLUSH_TRACE, when it is set.
pub(crate) fn trace_app(line: &str) {
    if let Some(path) = std::env::var_os("LUMEN_FLUSH_TRACE")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}
