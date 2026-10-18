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

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use lumen_devices::{Cost, Guard, HostedInstrument, Status};
use lumen_engine::{InstrumentSpec, MidiTrack, MixerHandle};
use lumen_hosting::{
    Captured, Edit, EditKind, EditorBounds, EditorEvent, Happened, HostLog, Manager, ParamInfo,
    ProgramList, Start, UnitInfo,
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

    /// Shows `device` in `state` whatever its host is doing: for tests and
    /// screenshots of the cards' faces. Only a device already tracked
    /// (after a frame with the project open) changes.
    pub fn pretend(&mut self, device: Id, state: State) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.state = state;
        }
    }

    /// The plugin's parameters as listed when it loaded (empty until it has).
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.list.as_slice())
    }

    /// The plugin's own count of parameters, which may be more than listed.
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn parameter_total(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.total)
    }

    /// The groups the plugin's parameters sit in.
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn units(&self, device: Id) -> &[UnitInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.units.as_slice())
    }

    /// A parameter's value as far as Lumen knows (normalized).
    pub fn value(&self, device: Id, id: u32) -> Option<f64> {
        self.tracked.get(&device.0)?.params.values.get(&id).copied()
    }

    /// Asks the plugin for its own text for `value` of a parameter; the answer
    /// is [`Hosting::text`] a moment later.
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn ask_text(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_text(device.0, id, value);
        }
    }

    /// What the device's plugin window is doing.
    pub fn editor(&self, device: Id) -> EditorState {
        self.tracked
            .get(&device.0)
            .map(|t| t.editor.clone())
            .unwrap_or_default()
    }

    /// Opens the device's plugin window (at `place` if the plugin's was
    /// remembered). The news comes back as [`Hosting::editor`] changing.
    pub fn open_editor(&mut self, device: Id, place: Option<EditorBounds>) {
        let Some(t) = self.tracked.get_mut(&device.0) else {
            return;
        };
        if !matches!(t.state, State::Ready) || !matches!(t.editor, EditorState::Closed) {
            return;
        }
        t.editor = EditorState::Opening;
        if let Some(manager) = self.manager.as_ref() {
            manager.open_editor(device.0, place);
        }
    }

    /// Shows the plugin windows of `devices` and hides every other that is
    /// open (the user is on a track that has them or not), so a window is
    /// only on screen with its own track. Only a change is sent: a window
    /// still being made is dealt with once it is up. Returns the devices
    /// whose windows were hidden by this call.
    pub fn show_editors_of(&mut self, devices: &[Id]) -> Vec<Id> {
        let mut hidden_now = Vec::new();
        for (&id, t) in &mut self.tracked {
            if !matches!(t.editor, EditorState::Open | EditorState::NotResponding) {
                continue;
            }
            let hide = !devices.contains(&Id(id));
            if t.editor_hidden == hide {
                continue;
            }
            t.editor_hidden = hide;
            if let Some(manager) = self.manager.as_ref() {
                manager.show_editor(id, !hide);
            }
            if hide {
                hidden_now.push(Id(id));
            }
        }
        hidden_now
    }

    /// Sends a parameter value straight to the device's plugin, as its own
    /// window would deliver it, without an edit of the project. For tests.
    pub fn poke_param(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.set_param(device.0, id, value);
        }
    }

    /// Whether the device's plugin window is hidden while open.
    pub fn editor_hidden(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| t.editor_hidden)
    }

    /// Closes the device's plugin window.
    pub fn close_editor(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.close_editor(device.0);
        }
    }

    /// Ends the host of a plugin whose window stopped answering. The device
    /// then reads as failed, with Reload.
    pub fn end_plugin(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.end(device.0);
        }
    }

    /// Where plugin windows were last seen since the last call, by plugin
    /// class, for the app to remember.
    pub fn take_editor_places(&mut self) -> Vec<(String, EditorBounds)> {
        std::mem::take(&mut self.places)
    }

    /// Has every plugin's host take `keys` (virtual-key codes) from its
    /// window when pressed alone, and give the rest to the plugin; empty
    /// gives them all back. Cheap when nothing changes.
    pub fn capture_keys(&mut self, keys: Vec<u8>) {
        if let Some(manager) = self.manager.as_mut() {
            manager.capture_keys(keys);
        }
    }

    /// The keys pressed (`true`) and let go of in plugins' windows since the
    /// last call, oldest first.
    pub fn take_keys(&mut self) -> Vec<(u8, bool)> {
        std::mem::take(&mut self.keys)
    }

    /// Asks the plugin to read `text` the user typed for a parameter; the
    /// answer is in [`Hosting::take_typed`] a moment later.
    pub fn ask_value(&self, device: Id, id: u32, text: String) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_value(device.0, id, text);
        }
    }

    /// What plugins made of the text typed for them since the last call, in
    /// the order they answered.
    pub fn take_typed(&mut self) -> Vec<Typed> {
        let mut out = Vec::new();
        let mut devices: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(_, t)| !t.params.typed.is_empty())
            .map(|(&id, _)| id)
            .collect();
        devices.sort_unstable();
        for id in devices {
            if let Some(t) = self.tracked.get_mut(&id) {
                for (parameter, text, value) in std::mem::take(&mut t.params.typed) {
                    out.push(Typed {
                        device: Id(id),
                        id: parameter,
                        text,
                        value,
                    });
                }
            }
        }
        out
    }

    /// The plugin's text for the value it was last asked about, and that value.
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn text(&self, device: Id, id: u32) -> Option<&(f64, String)> {
        self.tracked.get(&device.0)?.params.texts.get(&id)
    }

    /// Events no parameter of the plugin took (it maps no controller to one).
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn ignored_events(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.ignored)
    }

    /// The edits plugins made inside themselves since the last call, oldest
    /// first, for the document to record. Their values are already known here,
    /// so the document's matching change sends nothing back.
    pub fn take_edits(&mut self) -> Vec<DeviceEdits> {
        let mut out: Vec<DeviceEdits> = self
            .tracked
            .iter_mut()
            .filter(|(_, t)| !t.params.edits.is_empty())
            .map(|(&id, t)| DeviceEdits {
                device: Id(id),
                edits: std::mem::take(&mut t.params.edits),
            })
            .collect();
        out.sort_by_key(|e| e.device);
        out
    }

    /// Whether a plugin has a gesture open (a control held in its window): the
    /// document keeps the undo step open until it ends.
    pub fn gesture_open(&self) -> bool {
        self.tracked.values().any(|t| t.params.gesture_is_open())
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

    /// The programs a plugin offers, as it listed them when it loaded.
    pub fn programs(&self, device: Id) -> &[ProgramList] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.programs.as_slice())
    }

    /// The user chose a program: for a moment the edits the plugin makes in
    /// answer (the values the program holds) are one undo step with the
    /// choice, named for it.
    pub fn begin_program(&mut self, device: Id, name: String) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.program = Some((name, std::time::Instant::now()));
            t.params.program_fresh = true;
        }
    }

    /// The program being chosen, while the edits it causes still count as
    /// part of the choice.
    pub fn program_loading(&self, device: Id) -> Option<&str> {
        let (name, since) = self.tracked.get(&device.0)?.params.program.as_ref()?;
        (since.elapsed() < PROGRAM_WINDOW).then_some(name.as_str())
    }

    /// Gives a running plugin a preset's state to try. The project is edited
    /// only when the plugin has taken it ([`PresetNews::Ready`]), so a state
    /// the plugin refuses changes nothing. False when the plugin is not running
    /// or is already taking one.
    pub fn try_preset(
        &mut self,
        device: Id,
        name: String,
        component: Vec<u8>,
        controller: Option<Vec<u8>>,
    ) -> bool {
        let (Some(manager), Some(t)) = (self.manager.as_ref(), self.tracked.get_mut(&device.0))
        else {
            return false;
        };
        if t.state != State::Ready
            || t.params.retry.is_some()
            || t.params.loads.iter().any(|l| l.preset.is_some())
        {
            return false;
        }
        manager.load_state(device.0, component.clone(), controller.clone(), Vec::new());
        t.params.loads.push_back(Load {
            state: Captured {
                component,
                controller,
            },
            preset: Some(name),
            tries: 0,
        });
        t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
        true
    }

    /// When a refused preset is given to its plugin again, if one waits.
    pub fn next_retry(&self) -> Option<std::time::Duration> {
        self.tracked
            .values()
            .filter_map(|t| t.params.retry.as_ref())
            .map(|(at, _)| at.saturating_duration_since(std::time::Instant::now()))
            .min()
    }

    /// Presets plugins took or refused since the last call.
    pub fn take_presets(&mut self) -> Vec<PresetNews> {
        std::mem::take(&mut self.presets)
    }

    /// The project now holds the sound the plugin has (a preset it took was
    /// put into the project as revision `rev`): the plugin need not be given
    /// it again.
    pub fn synced(&mut self, device: Id, rev: u32) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.state_rev = rev;
        }
    }

    /// Whether the plugin's state has moved on from what the project saved
    /// (a parameter changed, from Lumen or inside the plugin).
    #[allow(
        dead_code,
        reason = "the card's strip and the parameter panel use it (slice 4e)"
    )]
    pub fn dirty(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| t.params.dirty)
    }

    /// Gives a tracked device a parameter list, as if its plugin had listed
    /// it: for screenshots of the card's strip. `(title, value, text)`; ids
    /// count from 1.
    pub fn pretend_parameters(&mut self, device: Id, list: &[(&str, f64, &str)]) {
        let Some(t) = self.tracked.get_mut(&device.0) else {
            return;
        };
        for (i, (title, value, text)) in list.iter().enumerate() {
            let id = i as u32 + 1;
            t.params.list.push(ParamInfo {
                id,
                title: (*title).to_owned(),
                units: String::new(),
                steps: 0,
                default: 0.5,
                flags: 1,
                unit: 0,
                value: *value,
            });
            t.params.values.insert(id, *value);
            t.params.texts.insert(id, (*value, (*text).to_owned()));
        }
        t.params.total = list.len() as u32;
    }

    /// Gives a tracked device a program list and said failures, for screenshots
    /// of the preset menu and the failed card.
    pub fn pretend_programs(&mut self, device: Id, name: &str, programs: &[&str]) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.params.programs.push(ProgramList {
                id: t.params.programs.len() as i32,
                name: name.to_owned(),
                unit: 0,
                parameter: Some(90),
                programs: programs.iter().map(|p| (*p).to_owned()).collect(),
            });
        }
    }

    /// Says a tracked device failed `count` times just now and that its last
    /// good state was read `age` ago, for screenshots of the failed card.
    pub fn pretend_failures(&mut self, device: Id, count: usize, age: Option<std::time::Duration>) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.failures = vec![std::time::Instant::now(); count];
            t.params.captured_when = age.and_then(|age| std::time::Instant::now().checked_sub(age));
        }
    }

    /// Sets what a tracked device's plugin window is doing, for screenshots.
    pub fn pretend_editor(&mut self, device: Id, state: EditorState) {
        if let Some(t) = self.tracked.get_mut(&device.0) {
            t.editor = state;
        }
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
            if manager.reload_from(device.0, state, list).unwrap_or(false) {
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
        }
    }

    /// Ends every host; when Lumen closes.
    pub fn shutdown(&mut self) {
        if let Some(manager) = self.manager.as_mut() {
            manager.shutdown();
        }
        self.tracked.clear();
        self.instruments.clear();
        self.waiting.clear();
    }

    /// Keeps the hosts in step with the project and the engine. Call every
    /// frame with the device's rate and block size (0 for unknown).
    /// `latencies` receives each effect's latency for the routing.
    pub fn update(
        &mut self,
        project: &Project,
        catalog: &Catalog,
        (rate, block): (u32, u32),
        mixer: &mut MixerHandle,
        latencies: &mut std::collections::BTreeMap<Id, u32>,
    ) -> Changed {
        let mut changed = Changed::default();
        let frames = promised_frames(block);
        let list = wanted(project);

        // Devices that left the project, or changed which plugin they hold,
        // or an instrument that moved to another track: the engine keeps an
        // instrument by its track, so it starts again where it now plays
        // (an effect is attached by its own id and goes with its device).
        let gone: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(id, t)| {
                !list.iter().any(|w| {
                    w.device.id.0 == **id
                        && w.class == t.class
                        && (!t.instrument || w.track.id == t.track)
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            let Some(t) = self.tracked.remove(&id) else {
                continue;
            };
            if let Some(manager) = self.manager.as_mut() {
                manager.forget(id);
            }
            if t.instrument {
                self.instruments.remove(&t.track.0);
                changed.midi = true;
            } else {
                mixer.detach_host(id);
            }
            self.waiting.retain(|(w, _)| *w != id);
        }

        // New devices, and missing ones whose plugin has turned up.
        for w in &list {
            let id = w.device.id.0;
            let known = self.tracked.get(&id).map(|t| t.state.clone());
            if !matches!(known, None | Some(State::Missing | State::Safe)) {
                continue;
            }
            let state = self.start(w, catalog, rate, frames);
            match self.tracked.get_mut(&id) {
                Some(t) => t.state = state,
                None => {
                    self.tracked.insert(
                        id,
                        Tracked {
                            track: w.track.id,
                            class: w.class.to_owned(),
                            instrument: w.instrument,
                            state,
                            status: None,
                            log: None,
                            latency: w.device.reported.map_or(0, |r| r.latency_at(rate.max(1))),
                            tail: w.device.reported.map_or(0, |r| r.tail),
                            rate,
                            frames: 0,
                            bypass: false,
                            params: Parameters {
                                started_with: w.device.params.clone(),
                                state_rev: w.device.state_rev,
                                ..Parameters::default()
                            },
                            editor: EditorState::Closed,
                            linked_at: None,
                            editor_hidden: false,
                            relinking: false,
                            failures: Vec::new(),
                        },
                    );
                }
            }
        }

        // A new rate or block size: every host configures again for it, so
        // a plugin is always promised the device's block (ADR-0075).
        if rate != 0 && (rate != self.rate || frames != self.frames) {
            if self.rate != 0
                && let Some(manager) = self.manager.as_mut()
            {
                manager.relink_all(f64::from(rate), frames);
                for t in self.tracked.values_mut() {
                    t.relinking = t.state == State::Ready;
                }
            }
            self.rate = rate;
            self.frames = frames;
        }

        // What the hosts said.
        let events = self.manager.as_mut().map(Manager::poll).unwrap_or_default();
        for event in events {
            let id = event.device;
            let Some(t) = self.tracked.get_mut(&id) else {
                continue;
            };
            match event.happened {
                Happened::Linked(linked) => {
                    let linked = *linked;
                    let guard = Guard::new(Box::new(linked.link), linked.latency);
                    t.status = Some(guard.status());
                    t.log = Some(linked.log.clone());
                    t.latency = linked.latency;
                    t.tail = linked.tail;
                    t.rate = self.rate;
                    t.frames = linked.frames;
                    t.state = State::Ready;
                    t.linked_at = Some(std::time::Instant::now());
                    t.bypass = false;
                    t.relinking = false;
                    if !t.params.linked_before {
                        t.params.linked_before = true;
                        // A fresh host has no window; a new link of the same
                        // host (the buffer size changed) keeps its own.
                        t.editor = EditorState::Closed;
                        t.editor_hidden = false;
                        t.params.base = linked.params.iter().map(|p| (p.id, p.value)).collect();
                        t.params.values = t.params.base.clone();
                        for (&id, &v) in &t.params.started_with {
                            t.params.values.insert(id, v);
                            // A value recovered from before a failure is the
                            // plugin's now, not the project's: it is not
                            // something to put back.
                            if !t.params.held.contains_key(&id) {
                                t.params.base.insert(id, v);
                            }
                        }
                        t.params.last_checkpoint = Some(std::time::Instant::now());
                        t.params.list = linked.params;
                        t.params.total = linked.params_total;
                        t.params.units = linked.units;
                        t.params.programs = linked.programs;
                    }
                    if t.instrument {
                        self.instruments.insert(
                            t.track.0,
                            InstrumentSpec {
                                kind: HOSTED_KIND,
                                preset: 0,
                                instrument: Box::new(HostedInstrument::new(guard)),
                            },
                        );
                        changed.midi = true;
                    } else if let Err(guard) = mixer.attach_host(id, guard) {
                        self.waiting.push((id, guard));
                    }
                }
                Happened::Failed(why) => {
                    t.editor = EditorState::Closed;
                    t.editor_hidden = false;
                    t.relinking = false;
                    t.state = State::Failed(why.to_string());
                    note_failure(&mut t.failures);
                }
                Happened::Key { code, down } => self.keys.push((code, down)),
                Happened::Text { id, value, text } => {
                    t.params.texts.insert(id, (value, text));
                }
                Happened::Editor(news) => match news {
                    EditorEvent::Opened(b) | EditorEvent::Moved(b) => {
                        if t.editor != EditorState::NotResponding
                            || matches!(news, EditorEvent::Opened(_))
                        {
                            t.editor = EditorState::Open;
                        }
                        if matches!(news, EditorEvent::Opened(_)) {
                            // A window comes up shown.
                            t.editor_hidden = false;
                        }
                        self.places.push((t.class.clone(), b));
                    }
                    EditorEvent::Closed(place) => {
                        t.editor = EditorState::Closed;
                        t.editor_hidden = false;
                        if let Some(b) = place {
                            self.places.push((t.class.clone(), b));
                        }
                    }
                    EditorEvent::NotResponding => t.editor = EditorState::NotResponding,
                    EditorEvent::Responding => {
                        if t.editor == EditorState::NotResponding {
                            t.editor = EditorState::Open;
                        }
                    }
                    EditorEvent::Refused(why) => t.editor = EditorState::Refused(why),
                },
                Happened::Typed { id, text, value } => {
                    t.params.typed.push((id, text, value));
                }
                Happened::State(state) => {
                    let p = &mut t.params;
                    let at = p.capturing.take().unwrap_or(0);
                    match state {
                        Ok(bytes) => {
                            p.captured = Some(bytes);
                            p.captured_when = Some(std::time::Instant::now());
                            p.captured_values = p.capturing_values.take().unwrap_or_default();
                            p.captured_at = at;
                            p.capture_failed = false;
                            if at == p.edit_count {
                                p.dirty = false;
                            }
                        }
                        Err(_) => {
                            // The state read before stays the last good one.
                            p.capturing_values = None;
                            p.capture_failed = true;
                        }
                    }
                }
                Happened::Values(all) => {
                    // What the plugin holds now: a value that moved without a
                    // word (a program chosen in its window, a patch) is an edit
                    // of the project, as if the plugin had said so.
                    let p = &mut t.params;
                    for (id, value) in all {
                        let moves = p
                            .list
                            .iter()
                            .find(|i| i.id == id)
                            .is_none_or(|i| !i.read_only() && !i.bypass() && !i.hidden());
                        let Some(&old) = p.values.get(&id) else {
                            continue;
                        };
                        if moves && (old - value).abs() > 1e-6 {
                            p.base.entry(id).or_insert(old);
                            p.values.insert(id, value);
                            p.touch();
                            p.edits.push((
                                Edit {
                                    id,
                                    value,
                                    kind: EditKind::Perform,
                                },
                                false,
                            ));
                        }
                    }
                }
                Happened::Loaded(answer) => {
                    let p = &mut t.params;
                    p.quiet_until = Some(std::time::Instant::now() + QUIET);
                    let Some(load) = p.loads.pop_front() else {
                        continue;
                    };
                    match answer {
                        Ok(values) => {
                            // The plugin's values are the state's own now: the
                            // baseline an undone change goes back to.
                            let values: BTreeMap<u32, f64> = values.into_iter().collect();
                            p.base = values.clone();
                            p.values = values;
                            p.captured = Some(load.state.clone());
                            p.captured_when = Some(std::time::Instant::now());
                            p.captured_values = p.values.clone();
                            p.captured_at = p.edit_count;
                            p.dirty = false;
                            p.capture_failed = false;
                            if let Some(manager) = self.manager.as_mut() {
                                manager.keep_state(id, load.state.clone());
                            }
                            if let Some(name) = load.preset {
                                self.presets.push(PresetNews::Ready {
                                    device: Id(id),
                                    name,
                                    component: load.state.component,
                                    controller: load.state.controller,
                                });
                            }
                        }
                        Err(why) => {
                            if load.preset.is_some() && load.tries < PRESET_RETRIES {
                                // Try again in a moment before giving up.
                                p.retry = Some((
                                    std::time::Instant::now() + PRESET_RETRY_WAIT,
                                    Load {
                                        tries: load.tries + 1,
                                        ..load
                                    },
                                ));
                            } else if let Some(name) = load.preset {
                                self.presets.push(PresetNews::Refused {
                                    device: Id(id),
                                    name,
                                    why,
                                });
                            }
                        }
                    }
                }
                Happened::Edits(taken) => {
                    t.params.ignored = t.params.ignored.saturating_add(taken.ignored);
                    let quiet = t
                        .params
                        .quiet_until
                        .is_some_and(|at| std::time::Instant::now() < at);
                    for edit in taken.list {
                        if quiet && edit.kind != EditKind::Output {
                            continue;
                        }
                        let p = &mut t.params;
                        match edit.kind {
                            EditKind::Begin => {
                                if let Some(&now) = p.values.get(&edit.id) {
                                    p.base.entry(edit.id).or_insert(now);
                                }
                                p.touch();
                                p.open = Some((edit.id, std::time::Instant::now()));
                                p.edits.push((edit, true));
                            }
                            EditKind::Perform => {
                                let before = p.values.insert(edit.id, edit.value);
                                if let Some(before) = before {
                                    p.base.entry(edit.id).or_insert(before);
                                }
                                p.touch();
                                let inside = p.open.is_some_and(|(id, _)| id == edit.id);
                                if inside {
                                    p.open = Some((edit.id, std::time::Instant::now()));
                                }
                                p.edits.push((edit, inside));
                            }
                            EditKind::End => {
                                p.open = None;
                                p.edits.push((edit, true));
                            }
                            // What the processor reports is the plugin's to
                            // show, not an edit of the project.
                            EditKind::Output => {
                                p.values.insert(edit.id, edit.value);
                            }
                            // The plugin changed inside, where no parameter
                            // shows it: its state is ahead of the project's
                            // (read once it rests), and the project has
                            // unsaved changes. A plugin says as much while it
                            // loads, which is not the user's change.
                            EditKind::Dirty => {
                                if t.linked_at.is_some_and(|at| at.elapsed() >= SETTLE) {
                                    p.touch();
                                    self.inside_changes = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Effects the mixer's queue had no room for last time.
        let mut still = Vec::new();
        for (id, guard) in std::mem::take(&mut self.waiting) {
            if let Err(guard) = mixer.attach_host(id, guard) {
                still.push((id, guard));
            }
        }
        self.waiting = still;

        // What the engine learned about the running ones.
        for w in &list {
            let id = w.device.id.0;
            let Some(t) = self.tracked.get_mut(&id) else {
                continue;
            };
            let Some(status) = t.status.clone() else {
                continue;
            };
            if t.bypass != w.device.bypass {
                status.set_bypass(w.device.bypass);
                t.bypass = w.device.bypass;
            }
            if let Some(latency) = status.take_news() {
                t.latency = latency;
            }
            if status.take_restart()
                && let Some(manager) = self.manager.as_mut()
            {
                manager.relink(id, f64::from(self.rate), self.frames);
                t.relinking = true;
            }
            if status.failed() && t.state == State::Ready && !t.relinking {
                t.state = State::Failed(gave_up_words(t.log.as_deref()));
                note_failure(&mut t.failures);
            }
            // The sound the project holds is the plugin's: after a preset was
            // loaded, or an undo or redo went across one, the plugin is given
            // the project's state. Its own values come back with the answer.
            if t.state == State::Ready
                && t.params.state_rev != w.device.state_rev
                && w.device.state.is_empty()
                && !t.params.loads.iter().any(|l| l.preset.is_some())
                && let Some(manager) = self.manager.as_mut()
            {
                // The project holds no state (a plugin as it was added): a
                // plugin cannot be told to forget, so it is started again from
                // nothing, with the values the project holds over it.
                let values: Vec<(u32, f64)> =
                    w.device.params.iter().map(|(&k, &v)| (k, v)).collect();
                let nothing = Captured {
                    component: Vec::new(),
                    controller: None,
                };
                if manager
                    .reload_from(id, Some(nothing), values)
                    .unwrap_or(false)
                {
                    t.state = State::Loading;
                    t.status = None;
                    t.log = None;
                    t.params = Parameters {
                        started_with: w.device.params.clone(),
                        held: w.device.params.clone(),
                        state_rev: w.device.state_rev,
                        ..Parameters::default()
                    };
                    continue;
                }
            }
            if t.state == State::Ready
                && t.params.loads.is_empty()
                && t.params
                    .retry
                    .as_ref()
                    .is_some_and(|(at, _)| std::time::Instant::now() >= *at)
                && let Some(manager) = self.manager.as_ref()
                && let Some((_, load)) = t.params.retry.take()
            {
                manager.load_state(
                    id,
                    load.state.component.clone(),
                    load.state.controller.clone(),
                    Vec::new(),
                );
                t.params.loads.push_back(load);
                t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
            }
            if t.state == State::Ready
                && t.params.state_rev != w.device.state_rev
                && t.params.retry.is_none()
                && !t.params.loads.iter().any(|l| l.preset.is_some())
                && let Some(manager) = self.manager.as_ref()
            {
                let state = Captured {
                    component: w.device.state.clone(),
                    controller: Some(w.device.controller_state.clone()).filter(|c| !c.is_empty()),
                };
                manager.load_state(
                    id,
                    state.component.clone(),
                    state.controller.clone(),
                    Vec::new(),
                );
                t.params.loads.push_back(Load {
                    state,
                    preset: None,
                    tries: 0,
                });
                t.params.state_rev = w.device.state_rev;
                t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
            }
            // The values the project holds are the plugin's: what an edit,
            // an undo or a redo changed is sent to the host (not while a state
            // is on its way: its values are not known yet).
            if t.state == State::Ready
                && t.params.loads.is_empty()
                && let Some(manager) = self.manager.as_ref()
            {
                let mut send = t.params.differences(&w.device.params);
                // A program parameter first: choosing a program sets other
                // parameters as it goes, and the values the project holds
                // for those must be set after, not overwritten by it.
                let programs: Vec<u32> = t
                    .params
                    .list
                    .iter()
                    .filter(|i| i.program_change())
                    .map(|i| i.id)
                    .collect();
                send.sort_by_key(|(id, _)| !programs.contains(id));
                // What the plugin answers a value of ours with (the values a
                // program holds, a linked parameter) is not the user's edit:
                // unless the user chose a program, whose answer is wanted.
                let choosing = t.params.program_fresh;
                if !send.is_empty() {
                    t.params.program_fresh = false;
                    if !choosing {
                        t.params.quiet_until = Some(std::time::Instant::now() + QUIET);
                    }
                }
                for (id, value) in send {
                    manager.set_param(w.device.id.0, id, value);
                    t.params.values.insert(id, value);
                    t.params.touch();
                }
            }
            if t.params.held != w.device.params {
                t.params.held = w.device.params.clone();
            }
            // A plugin that has changed is read once it rests, so the next
            // save holds what it is now.
            if t.state == State::Ready
                && t.params.dirty
                && t.params.loads.is_empty()
                && t.params.capturing.is_none()
                && !t.params.capture_failed
                && let Some(since) = t.params.last_edit
                && let Some(manager) = self.manager.as_ref()
            {
                let waited = since.elapsed();
                if waited >= REST {
                    manager.capture(id);
                    // And every value, for what moved without a word.
                    manager.refresh(id);
                    t.params.asked();
                    t.params.last_checkpoint = Some(std::time::Instant::now());
                } else {
                    let left = (REST - waited).as_millis() as u64 + 1;
                    changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
                }
            }
        }

        // Checkpoints: every running plugin is read now and then, whether or not
        // it said it changed (a plugin keeps most of what the user sets without
        // a word), so a plugin that fails, or a crash of Lumen's own, costs the
        // time since the last one and not the session.
        if let Some(manager) = self.manager.as_ref() {
            let every = self.checkpoint_every;
            let busy = self.tracked.values().any(|t| t.params.gesture_is_open());
            for (&id, t) in &mut self.tracked {
                if t.state != State::Ready {
                    continue;
                }
                let Some(last) = t.params.last_checkpoint else {
                    continue;
                };
                // Staggered by device, so plugins are not all read at once.
                let due = last + every + every / 8 * (id % 4) as u32;
                let now = std::time::Instant::now();
                if now < due {
                    let left = (due - now).as_millis() as u64 + 1;
                    changed.again_in_ms = Some(changed.again_in_ms.map_or(left, |m| m.min(left)));
                } else if t.params.capturing.is_none() && t.params.loads.is_empty() && !busy {
                    manager.capture(id);
                    t.params.asked();
                    t.params.last_checkpoint = Some(now);
                }
            }
        }

        // The routing's picture of each effect's latency.
        latencies.clear();
        for w in list.iter().filter(|w| !w.instrument) {
            if let Some(t) = self.tracked.get(&w.device.id.0) {
                latencies.insert(w.device.id, t.latency);
            }
        }
        changed
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

    /// Asks every running plugin for its state now, because a save is about
    /// to be written. Every plugin, not only those that reported a change: a
    /// plugin keeps most of what the user sets (a patch, a preset, its own
    /// pages) without telling the host, so nothing says it has changed. True
    /// when a read is in flight, to be waited for ([`Hosting::captures_pending`]).
    pub fn request_captures(&mut self) -> bool {
        let Some(manager) = self.manager.as_ref() else {
            return false;
        };
        let mut any = false;
        for (&id, t) in &mut self.tracked {
            if t.state != State::Ready {
                continue;
            }
            if t.params.capturing.is_none() {
                manager.capture(id);
                t.params.asked();
            }
            any = true;
        }
        any
    }

    /// Whether a plugin has said, since the last call, that something changed
    /// inside it that no parameter shows (a patch, a preset). The project then
    /// has unsaved changes, though no edit.
    pub fn take_inside_changes(&mut self) -> bool {
        std::mem::take(&mut self.inside_changes)
    }

    /// Asks one running plugin for its state now (a preset to be saved). Not
    /// asked again while a read is in flight. False when the plugin is not
    /// running.
    pub fn capture_one(&mut self, device: Id) -> bool {
        let Some(manager) = self.manager.as_ref() else {
            return false;
        };
        let Some(t) = self
            .tracked
            .get_mut(&device.0)
            .filter(|t| t.state == State::Ready)
        else {
            return false;
        };
        if t.params.capturing.is_none() && !t.params.fresh() {
            manager.capture(device.0);
            t.params.asked();
        }
        true
    }

    /// The plugin's state as it is now: the last read, when nothing has changed
    /// in the plugin since. None while it has changed or a read is in flight.
    pub fn fresh_state(&self, device: Id) -> Option<Captured> {
        let p = &self.tracked.get(&device.0)?.params;
        (p.capturing.is_none() && p.fresh())
            .then(|| p.captured.clone())
            .flatten()
    }

    /// Whether the last read of the plugin's state failed.
    pub fn capture_failed(&self, device: Id) -> bool {
        self.tracked
            .get(&device.0)
            .is_some_and(|t| t.params.capture_failed)
    }

    /// Whether a plugin's state is still being read.
    pub fn captures_pending(&self) -> bool {
        self.tracked
            .values()
            .any(|t| t.state == State::Ready && t.params.capturing.is_some())
    }

    /// How many plugins have changed and could not be read, so a save holds
    /// the state they were loaded with and the parameter values the project
    /// keeps, but nothing else that changed inside them.
    pub fn unread(&self) -> usize {
        self.tracked
            .values()
            .filter(|t| t.state == State::Ready && t.params.dirty && t.params.capture_failed)
            .count()
    }

    /// Stamps what the running plugins reported into `project`, to be saved.
    pub fn stamp(&self, project: &mut Project) {
        for track in project
            .tracks
            .iter_mut()
            .chain(std::iter::once(&mut project.master))
        {
            if !track.devices.iter().any(|d| {
                self.tracked
                    .get(&d.id.0)
                    .is_some_and(|t| t.state == State::Ready)
            }) {
                continue;
            }
            let track = Arc::make_mut(track);
            for device in &mut track.devices {
                if let Some(t) = self.tracked.get(&device.id.0)
                    && t.state == State::Ready
                    && t.rate != 0
                {
                    device.reported = Some(Reported {
                        latency: t.latency,
                        tail: t.tail,
                        rate: t.rate,
                    });
                    // A state read since the plugin last changed is what it
                    // is now, with every value in it: it replaces the saved
                    // state and the values the project held over it.
                    if let Some(captured) = &t.params.captured {
                        device.state = captured.component.clone();
                        if let Some(controller) = &captured.controller {
                            device.controller_state = controller.clone();
                        }
                        // A state read since the plugin last changed is what it
                        // is now, with every value in it. An older one (a
                        // checkpoint, or a read that failed since) has the
                        // values that moved after it set over it.
                        device.params = if t.params.fresh() {
                            BTreeMap::new()
                        } else {
                            t.params.catch_up(&device.params)
                        };
                    }
                }
            }
        }
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
mod tests {
    use super::*;
    use crate::document::{Document, Slot};

    #[test]
    fn plugins_are_promised_the_devices_block_at_most_the_engines() {
        assert_eq!(super::promised_frames(128), 128);
        assert_eq!(super::promised_frames(64), 64);
        assert_eq!(super::promised_frames(1024), 256, "the engine splits");
        assert_eq!(super::promised_frames(0), 256, "no stream yet");
    }
    use lumen_project::model::PluginRef;

    fn known(values: &[(u32, f64)], base: &[(u32, f64)]) -> Parameters {
        Parameters {
            values: values.iter().copied().collect(),
            base: base.iter().copied().collect(),
            ..Parameters::default()
        }
    }

    fn held(values: &[(u32, f64)]) -> BTreeMap<u32, f64> {
        values.iter().copied().collect()
    }

    #[test]
    fn a_value_the_project_holds_and_the_plugin_lacks_is_sent() {
        let p = known(&[(1, 0.5), (2, 0.5)], &[(1, 0.5), (2, 0.5)]);
        let mut send = p.differences(&held(&[(1, 0.9), (2, 0.5)]));
        send.sort_by_key(|&(id, _)| id);
        assert_eq!(send, [(1, 0.9)], "only what differs");
        assert!(p.differences(&held(&[(1, 0.5)])).is_empty());
    }

    #[test]
    fn a_value_the_project_let_go_of_goes_back_to_what_it_was() {
        // The project held 0.9 for parameter 1, over a saved 0.5; an undo
        // takes the entry away: the plugin must go back to 0.5.
        let p = known(&[(1, 0.9)], &[(1, 0.5)]);
        assert_eq!(p.differences(&held(&[])), [(1, 0.5)]);
        // Already back: nothing to send.
        let p = known(&[(1, 0.5)], &[(1, 0.5)]);
        assert!(p.differences(&held(&[])).is_empty());
    }

    #[test]
    fn a_value_set_inside_the_plugin_is_not_sent_back() {
        // The plugin reported 0.7 itself (known), then the document recorded
        // it: the project holds the same value, so nothing goes back.
        let p = known(&[(4, 0.7)], &[(4, 0.2)]);
        assert!(p.differences(&held(&[(4, 0.7)])).is_empty());
    }

    const CLASS: &str = "00112233445566778899AABBCCDDEEFF";

    fn plugin(kind: DeviceKind) -> PluginRef {
        PluginRef::Vst3 {
            class_id: CLASS.into(),
            name: "Plug".into(),
            vendor: "Vendor".into(),
            kind,
        }
    }

    fn project_with(kind: DeviceKind, track: TrackKind) -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let t = doc.add_track(track, None);
        let device = doc.add_device(t, plugin(kind), Slot::End).unwrap();
        (doc, t, device)
    }

    fn mixer_handle() -> MixerHandle {
        let (_clips, players) = lumen_engine::clip_player();
        let (_midi, player) = lumen_engine::midi_player();
        lumen_engine::mixer(lumen_engine::Layered(players, player)).0
    }

    #[test]
    fn a_plugin_that_cannot_be_found_is_missing_and_an_effect_keeps_its_saved_time() {
        let (mut doc, _, effect) = project_with(DeviceKind::Effect, TrackKind::Audio);
        // The plugin reported 512 frames at 48 kHz when the project was saved.
        doc.apply("report", None, |p| {
            let track = Arc::make_mut(&mut p.tracks[0]);
            track.devices[0].reported = Some(Reported {
                latency: 512,
                tail: 0,
                rate: 48_000,
            });
        });
        let mut hosting = Hosting::off();
        let mut mixer = mixer_handle();
        let mut latencies = Default::default();
        let catalog = Catalog::build(&Default::default(), Vec::new());
        hosting.update(
            doc.project(),
            &catalog,
            (96_000, 128),
            &mut mixer,
            &mut latencies,
        );
        assert_eq!(hosting.state(effect), Some(&State::Missing));
        assert_eq!(
            latencies.get(&effect),
            Some(&1_024),
            "scaled to the new rate"
        );
    }

    #[test]
    fn only_one_instrument_on_a_midi_track_is_wanted() {
        let (doc, _, _) = project_with(DeviceKind::Instrument, TrackKind::Midi);
        assert_eq!(wanted(doc.project()).len(), 1);
        let (doc, _, _) = project_with(DeviceKind::Effect, TrackKind::Audio);
        let w = wanted(doc.project());
        assert_eq!(w.len(), 1);
        assert!(!w[0].instrument);
    }

    #[test]
    fn a_device_that_leaves_the_project_is_forgotten() {
        let (mut doc, _, device) = project_with(DeviceKind::Effect, TrackKind::Audio);
        let mut hosting = Hosting::off();
        let mut mixer = mixer_handle();
        let mut latencies = Default::default();
        let catalog = Catalog::build(&Default::default(), Vec::new());
        hosting.update(
            doc.project(),
            &catalog,
            (48_000, 128),
            &mut mixer,
            &mut latencies,
        );
        assert!(hosting.state(device).is_some());
        doc.undo();
        hosting.update(
            doc.project(),
            &catalog,
            (48_000, 128),
            &mut mixer,
            &mut latencies,
        );
        assert!(hosting.state(device).is_none());
        assert!(latencies.is_empty());
    }
}
