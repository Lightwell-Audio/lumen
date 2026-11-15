mod capture;
mod editor;
mod hear;
mod outputs;
mod parameters;
mod presets;
mod pretend;
mod update;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use self::outputs::{output_mask, sidechain_mask};
use self::parameters::{Load, Parameters};
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

pub const HOSTED_KIND: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Loading,
    Ready,
    Failed(String),
    Missing,
    Safe,
}

struct Tracked {
    track: Id,
    class: String,
    instrument: bool,
    state: State,
    status: Option<Arc<Status>>,
    log: Option<Arc<HostLog>>,
    latency: u32,
    tail: u32,
    rate: u32,
    frames: u32,
    bypass: bool,
    params: Parameters,
    editor: EditorState,
    linked_at: Option<std::time::Instant>,
    editor_hidden: bool,
    relinking: bool,
    inputs: Vec<BusInfo>,
    outputs: Vec<BusInfo>,
    aux: Aux,
    failures: Vec<std::time::Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EditorState {
    #[default]
    Closed,
    Opening,
    Open,
    NotResponding,
    Refused(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PresetNews {
    Ready {
        device: Id,
        name: String,
        component: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    Refused {
        device: Id,
        name: String,
        why: String,
    },
}

pub const MOST_CHOICES: i32 = 64;

const PRESET_RETRIES: u8 = 2;
const PRESET_RETRY_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

const REST: std::time::Duration = std::time::Duration::from_millis(600);

const SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub device: Id,
    pub id: u32,
    pub text: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEdits {
    pub device: Id,
    pub edits: Vec<(Edit, bool)>,
}

const EXIT_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

const CHECKPOINT: std::time::Duration = std::time::Duration::from_secs(30);

const FAILURES_THAT_COUNT: usize = 3;
const FAILURE_WINDOW: std::time::Duration = std::time::Duration::from_secs(120);

fn note_failure(failures: &mut Vec<std::time::Instant>, state: &State) {
    if matches!(state, State::Failed(_)) {
        return;
    }
    failures.retain(|at| at.elapsed() < FAILURE_WINDOW);
    failures.push(std::time::Instant::now());
}

const QUIET: std::time::Duration = std::time::Duration::from_millis(400);

const RELEASE_AGAIN: std::time::Duration = std::time::Duration::from_millis(250);

const PROGRAM_WINDOW: std::time::Duration = std::time::Duration::from_millis(1500);

const GESTURE_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub struct PluginCost {
    pub device: Id,
    pub name: String,
    pub place: String,
    pub cost: Cost,
    pub late: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub midi: bool,
    pub again_in_ms: Option<u64>,
}

pub struct Hosting {
    manager: Option<Manager>,
    host: Option<PathBuf>,
    faults: Option<crate::plugins::Faults>,
    tracked: HashMap<u64, Tracked>,
    instruments: HashMap<u64, InstrumentSpec>,
    waiting: Vec<(u64, Guard)>,
    rate: u32,
    frames: u32,
    places: Vec<(String, EditorBounds)>,
    inside_changes: bool,
    texts_forgotten: u64,
    keys: Vec<(u8, bool)>,
    presets: Vec<PresetNews>,
    checkpoint_every: std::time::Duration,
    safe: bool,
    allowed: std::collections::HashSet<u64>,
    overridden: crate::document::Overrides,
}

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

const TEST_EDITOR_WAIT_MS: &str = "600";

pub fn promised_frames(block: u32) -> u32 {
    let most = lumen_devices::MOST_FRAMES as u32;
    if block == 0 { most } else { block.min(most) }
}

pub struct OfflinePlugin {
    pub device: Id,
    pub name: String,
    pub instrument: bool,
    pub start: Start,
}

struct Wanted<'a> {
    track: &'a Track,
    device: &'a Device,
    class: &'a str,
    instrument: bool,
}

fn wanted(project: &Project) -> Vec<Wanted<'_>> {
    let mut list = Vec::new();
    for track in all_tracks(project).filter(|t| t.frozen.is_none()) {
        let mut instrument_seen = false;
        for device in &track.devices {
            let lumen_project::model::PluginRef::Vst3 { class_id, .. } = &device.plugin else {
                continue;
            };
            let instrument = device.kind() == DeviceKind::Instrument;
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
            texts_forgotten: 0,
            keys: Vec::new(),
            presets: Vec::new(),
            checkpoint_every: CHECKPOINT,
            safe: false,
            allowed: Default::default(),
            overridden: Default::default(),
        }
    }

    pub fn set_overridden(&mut self, overridden: &crate::document::Overrides) {
        if self.overridden != *overridden {
            self.overridden = overridden.clone();
        }
    }

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

    pub fn class(&self, device: Id) -> Option<&str> {
        self.tracked.get(&device.0).map(|t| t.class.as_str())
    }

    pub fn state(&self, device: Id) -> Option<&State> {
        self.tracked.get(&device.0).map(|t| &t.state)
    }

    pub fn promised(&self, device: Id) -> Option<u32> {
        self.tracked
            .get(&device.0)
            .map(|t| t.frames)
            .filter(|&f| f != 0)
    }

    pub fn latency(&self, device: Id) -> Option<u32> {
        self.tracked.get(&device.0).map(|t| t.latency)
    }

    pub fn set_safe(&mut self, safe: bool) {
        self.safe = safe;
        if !safe {
            self.allowed.clear();
        }
    }

    pub fn allow(&mut self, device: Id) {
        self.allowed.insert(device.0);
    }

    pub fn loading_any(&self) -> bool {
        self.tracked.values().any(|t| t.state == State::Loading)
    }

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

    pub fn host_count(&self) -> usize {
        self.manager.as_ref().map_or(0, Manager::len)
    }

    pub fn keeps_failing(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| {
            t.failures
                .iter()
                .filter(|at| at.elapsed() < FAILURE_WINDOW)
                .count()
                >= FAILURES_THAT_COUNT
        })
    }

    pub fn last_good_age(&self, device: Id) -> Option<std::time::Duration> {
        self.tracked
            .get(&device.0)?
            .params
            .captured_when
            .map(|at| at.elapsed())
    }

    pub fn set_checkpoint_every(&mut self, every: std::time::Duration) {
        self.checkpoint_every = every;
    }

    pub fn reload(&mut self, device: Id) {
        self.restart_failed(device, false);
    }

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
                Err(e) => {
                    t.state = State::Failed(format!("The plugin host could not be started: {e}"));
                }
            }
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(manager) = self.manager.as_mut() {
            manager.shutdown(EXIT_GRACE);
        }
        self.tracked.clear();
        self.instruments.clear();
        self.waiting.clear();
    }

    fn start(&mut self, w: &Wanted<'_>, catalog: &Catalog, rate: u32, frames: u32) -> State {
        if self.safe && !self.allowed.contains(&w.device.id.0) {
            return State::Safe;
        }
        if self.manager.is_none() {
            return State::Missing;
        }
        let start = match self.make_start(w, catalog, rate, frames) {
            Ok(start) => start,
            Err(state) => return state,
        };
        let Some(manager) = self.manager.as_mut() else {
            return State::Missing;
        };
        match manager.want(w.device.id.0, start) {
            Ok(()) => State::Loading,
            Err(e) => State::Failed(format!("The plugin host could not be started: {e}")),
        }
    }

    fn make_start(
        &self,
        w: &Wanted<'_>,
        catalog: &Catalog,
        rate: u32,
        frames: u32,
    ) -> Result<Start, State> {
        let Some(host) = self.host.as_ref() else {
            return Err(State::Missing);
        };
        let Some(module) = catalog
            .find(&format!("vst3:{}", w.class.to_ascii_uppercase()))
            .and_then(|d| d.module.clone())
        else {
            return Err(State::Missing);
        };
        let Some(class) = lumen_hosting::parse_class_id(w.class) else {
            return Err(State::Missing);
        };
        if rate == 0 {
            return Err(State::Missing);
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
        Ok(start)
    }

    pub fn offline_starts(
        &self,
        project: &Project,
        track: Id,
        effects: bool,
        catalog: &Catalog,
        rate: u32,
    ) -> Result<Vec<OfflinePlugin>, String> {
        let mut found = Vec::new();
        for w in wanted(project) {
            if w.track.id != track || !(effects || w.instrument) {
                continue;
            }
            let name = match &w.device.plugin {
                PluginRef::Vst3 { name, .. } | PluginRef::Builtin { name } => name.clone(),
            };
            if self.safe && !self.allowed.contains(&w.device.id.0) {
                return Err(format!("{name} is not loaded in safe mode"));
            }
            match self.make_start(&w, catalog, rate, promised_frames(0)) {
                Ok(start) => found.push(OfflinePlugin {
                    device: w.device.id,
                    name,
                    instrument: w.instrument,
                    start,
                }),
                Err(_) => return Err(format!("{name} is not installed")),
            }
        }
        Ok(found)
    }

    pub fn instrument(&mut self, track: Id) -> InstrumentSpec {
        self.instruments.remove(&track.0).unwrap_or_else(stand_in)
    }

    pub fn reclaim(&mut self, tracks: Vec<MidiTrack>) {
        for track in tracks {
            if let Some(spec) = track.instrument
                && spec.instrument.hosted()
            {
                self.instruments.insert(track.id, spec);
            }
        }
    }

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

pub fn stand_in() -> InstrumentSpec {
    InstrumentSpec {
        kind: HOSTED_KIND,
        preset: 0,
        values: Vec::new(),
        instrument: Box::new(HostedInstrument::placeholder()),
    }
}

#[cfg(test)]
mod tests;
