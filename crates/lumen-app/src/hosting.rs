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
use lumen_hosting::{Edit, EditKind, Happened, HostLog, Manager, ParamInfo, Start, UnitInfo};
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
}

/// The edits one device's plugin made inside itself, for the document.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEdits {
    pub device: Id,
    /// In the order the plugin made them; `true` when inside a gesture.
    pub edits: Vec<(Edit, bool)>,
}

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
}

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
    pub fn parameter_total(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.total)
    }

    /// The groups the plugin's parameters sit in.
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
    pub fn ask_text(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_text(device.0, id, value);
        }
    }

    /// The plugin's text for the value it was last asked about, and that value.
    pub fn text(&self, device: Id, id: u32) -> Option<&(f64, String)> {
        self.tracked.get(&device.0)?.params.texts.get(&id)
    }

    /// Events no parameter of the plugin took (it maps no controller to one).
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
        self.tracked.values().any(|t| {
            t.params
                .open
                .is_some_and(|(_, since)| since.elapsed() < GESTURE_LIMIT)
        })
    }

    /// Whether the plugin's state has moved on from what the project saved
    /// (a parameter changed, from Lumen or inside the plugin).
    pub fn dirty(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| t.params.dirty)
    }

    /// Starts a new host for a failed device (the card's Reload).
    pub fn reload(&mut self, device: Id) {
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        if let Some(t) = self.tracked.get_mut(&device.0)
            && matches!(t.state, State::Failed(_))
            && manager.reload(device.0).unwrap_or(false)
        {
            t.state = State::Loading;
            t.status = None;
            t.log = None;
            // A new process starts from the saved state.
            t.params = Parameters {
                started_with: t.params.started_with.clone(),
                ..Parameters::default()
            };
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
            if !matches!(known, None | Some(State::Missing)) {
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
                                ..Parameters::default()
                            },
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
                    t.bypass = false;
                    if !t.params.linked_before {
                        t.params.linked_before = true;
                        t.params.base = linked.params.iter().map(|p| (p.id, p.value)).collect();
                        t.params.values = t.params.base.clone();
                        for (&id, &v) in &t.params.started_with {
                            t.params.values.insert(id, v);
                        }
                        t.params.list = linked.params;
                        t.params.total = linked.params_total;
                        t.params.units = linked.units;
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
                    t.state = State::Failed(why.to_string());
                }
                Happened::Text { id, value, text } => {
                    t.params.texts.insert(id, (value, text));
                }
                Happened::Edits(taken) => {
                    t.params.ignored = t.params.ignored.saturating_add(taken.ignored);
                    for edit in taken.list {
                        let p = &mut t.params;
                        match edit.kind {
                            EditKind::Begin => {
                                if let Some(&now) = p.values.get(&edit.id) {
                                    p.base.entry(edit.id).or_insert(now);
                                }
                                p.dirty = true;
                                p.open = Some((edit.id, std::time::Instant::now()));
                                p.edits.push((edit, true));
                            }
                            EditKind::Perform => {
                                let before = p.values.insert(edit.id, edit.value);
                                if let Some(before) = before {
                                    p.base.entry(edit.id).or_insert(before);
                                }
                                p.dirty = true;
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
            }
            if status.failed() && t.state == State::Ready {
                t.state = State::Failed(gave_up_words(t.log.as_deref()));
            }
            // The values the project holds are the plugin's: what an edit,
            // an undo or a redo changed is sent to the host.
            if t.state == State::Ready
                && let Some(manager) = self.manager.as_ref()
            {
                for (id, value) in t.params.differences(&w.device.params) {
                    manager.set_param(w.device.id.0, id, value);
                    t.params.values.insert(id, value);
                    t.params.dirty = true;
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
        start.values = w.device.params.iter().map(|(&id, &v)| (id, v)).collect();
        start.frames = frames;
        if let Some(faults) = self.faults
            && let Some(stem) = module.file_stem()
            && let Some(fault) = faults(&stem.to_string_lossy())
        {
            start
                .env
                .push(("LUMEN_TEST_PLUGIN_FAULT".into(), fault.into()));
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

    /// Stamps what the running plugins reported into `project`, to be saved.
    pub fn stamp(&self, project: &mut Project) {
        for track in &mut project.tracks {
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
