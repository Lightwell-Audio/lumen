use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::Color32;
use lumen_devices::analyzer;
use lumen_devices::dynamics::{compressor, limiter};
use lumen_devices::params::{self, ParamInfo as Info};
use lumen_devices::{Description, MeterReading, Reduction, eq, utility};
use lumen_engine::{MixerHandle, Routing, Slot};
use lumen_hosting::{FLAG_CAN_AUTOMATE, ParamInfo};
use lumen_project::model::{Device, Id, PluginRef, Project, Track};
use lumen_project::routing::all_tracks;

use crate::theme;

pub const MOST_TAIL_SECONDS: u32 = 10;

pub fn tail_frames(project: &Project, rate: u32) -> u64 {
    let sounding = lumen_project::edit::freeze::played(project);
    let bpm = slowest_bpm(&sounding);
    let master = sounding.master.id;
    all_tracks(&sounding)
        .map(|track| path_tail(&sounding, track, master, bpm, rate, 0))
        .max()
        .unwrap_or(0)
        .min(cap(rate))
}

pub fn track_tail_frames(project: &Project, track: Id, rate: u32) -> u64 {
    let sounding = lumen_project::edit::freeze::played(project);
    let bpm = slowest_bpm(&sounding);
    all_tracks(&sounding)
        .find(|known| known.id == track)
        .map_or(0, |track| chain_tail(track, bpm, rate))
}

fn path_tail(project: &Project, track: &Track, master: Id, bpm: f32, rate: u32, depth: u32) -> u64 {
    let mut frames = chain_tail(track, bpm, rate);
    let mut parent = track.parent;
    let mut guard = 0;
    while let Some(id) = parent {
        guard += 1;
        if guard > 8 {
            break;
        }
        let Some(next) = all_tracks(project).find(|known| known.id == id) else {
            break;
        };
        frames = frames.saturating_add(chain_tail(next, bpm, rate));
        parent = next.parent;
    }
    if depth < 8 {
        let returns = track
            .sends
            .iter()
            .filter_map(|send| all_tracks(project).find(|known| known.id == send.to))
            .map(|ret| path_tail(project, ret, master, bpm, rate, depth + 1))
            .max()
            .unwrap_or(0);
        frames = frames.saturating_add(returns);
    }
    if track.id != master
        && let Some(top) = all_tracks(project).find(|known| known.id == master)
    {
        frames = frames.saturating_add(chain_tail(top, bpm, rate));
    }
    frames.min(cap(rate))
}

fn cap(rate: u32) -> u64 {
    u64::from(MOST_TAIL_SECONDS) * u64::from(rate)
}

fn chain_tail(track: &lumen_project::model::Track, bpm: f32, rate: u32) -> u64 {
    let mut frames = 0u64;
    for device in &track.devices {
        match &device.plugin {
            PluginRef::Builtin { name } => {
                let values = values(device);
                frames +=
                    u64::from(lumen_devices::effect_tail_at(name, &values, bpm, rate).unwrap_or(0));
            }
            PluginRef::Vst3 { .. } => frames += cap(rate),
        }
    }
    frames.min(cap(rate))
}

fn slowest_bpm(project: &Project) -> f32 {
    lumen_core::time::TempoMap::new(project.tempo.clone())
        .ok()
        .and_then(|map| {
            map.points()
                .iter()
                .map(|point| point.bpm as f32)
                .fold(None, |least: Option<f32>, bpm| {
                    Some(least.map_or(bpm, |least| least.min(bpm)))
                })
        })
        .unwrap_or(120.0)
}

#[derive(Clone, Copy)]
pub struct Identity {
    pub name: &'static str,
    pub kind: &'static str,
    pub subtitle: &'static str,
    pub colour: Color32,
    pub timing: &'static Description,
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.kind == other.kind && self.colour == other.colour
    }
}

impl Eq for Identity {}

const IDENTITIES: &[(&str, Identity)] = &[
    (
        utility::NAME,
        Identity {
            name: "Prism",
            kind: "Utility",
            subtitle: "Utility \u{00B7} gain, width, mono",
            colour: theme::personality::UTILITY,
            timing: &utility::DESCRIPTION,
        },
    ),
    (
        eq::NAME,
        Identity {
            name: "Iris",
            kind: "Parametric EQ",
            subtitle: "Parametric EQ",
            colour: theme::personality::EQ,
            timing: &eq::DESCRIPTION,
        },
    ),
    (
        lumen_devices::delay::NAME,
        Identity {
            name: "Parallax",
            kind: "Stereo delay",
            subtitle: "Stereo delay",
            colour: theme::personality::DELAY,
            timing: &lumen_devices::delay::DESCRIPTION,
        },
    ),
    (
        lumen_devices::reverb::NAME,
        Identity {
            name: "Afterglow",
            kind: "Reverb",
            subtitle: "Reverb",
            colour: theme::personality::REVERB,
            timing: &lumen_devices::reverb::DESCRIPTION,
        },
    ),
    (
        compressor::NAME,
        Identity {
            name: "Gravity",
            kind: "Compressor",
            subtitle: "Compressor",
            colour: theme::personality::GRAVITY,
            timing: &compressor::DESCRIPTION,
        },
    ),
    (
        limiter::NAME,
        Identity {
            name: "Horizon",
            kind: "Limiter",
            subtitle: "Limiter",
            colour: theme::personality::HORIZON,
            timing: &limiter::DESCRIPTION,
        },
    ),
    #[cfg(test)]
    (
        TEST_DEVICE,
        Identity {
            name: "Test timer",
            kind: "Test",
            subtitle: "Test",
            colour: theme::personality::UTILITY,
            timing: &TEST_TIMING,
        },
    ),
];

const INSTRUMENTS: &[(&str, Identity)] = &[(
    lumen_project::model::SYNTH,
    Identity {
        name: "Flare",
        kind: "Polysynth",
        subtitle: "Polysynth",
        colour: theme::personality::SYNTH,
        timing: &lumen_devices::synth::params::DESCRIPTION,
    },
)];

#[cfg(test)]
pub(crate) const TEST_DEVICE: &str = "test_timed";

#[cfg(test)]
fn test_latency(values: lumen_devices::Values<'_>) -> u32 {
    let up = params::normalised_of(utility::TABLE, utility::GAIN, values);
    240 + (up * 240.0) as u32
}

#[cfg(test)]
static TEST_TIMING: Description = Description {
    name: TEST_DEVICE,
    table: utility::TABLE,
    latency: test_latency,
    tail: |_| 0,
};

pub fn identity(name: &str) -> Option<Identity> {
    IDENTITIES
        .iter()
        .chain(INSTRUMENTS)
        .find(|(known, _)| *known == name)
        .map(|(_, identity)| *identity)
}

pub fn is_builtin_effect(plugin: &PluginRef) -> bool {
    matches!(plugin, PluginRef::Builtin { name } if has_effect(name))
}

fn has_effect(name: &str) -> bool {
    if lumen_devices::effect_description(name).is_some() {
        return true;
    }
    #[cfg(test)]
    if name == TEST_DEVICE {
        return true;
    }
    false
}

pub fn is_builtin(device: &Device) -> bool {
    is_builtin_effect(&device.plugin)
}

pub fn has_table(device: &Device) -> bool {
    identity(name_of(device)).is_some()
}

pub fn takes_sidechain(name: &str) -> Option<bool> {
    identity(name).map(|_| lumen_devices::takes_sidechain(name))
}

pub fn table(name: &str) -> Option<&'static [Info]> {
    identity(name).map(|identity| identity.timing.table)
}

pub fn defaults(name: &str) -> Vec<(u32, f32)> {
    table(name)
        .map(|table| {
            table
                .iter()
                .map(|info| (info.id, info.normalised(info.default)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn values(device: &Device) -> Vec<(u32, f32)> {
    let Some(table) = table(name_of(device)) else {
        return Vec::new();
    };
    table
        .iter()
        .map(|info| {
            let value = device
                .params
                .get(&info.id)
                .map_or_else(|| info.normalised(info.default), |&x| x as f32);
            (info.id, value)
        })
        .collect()
}

pub fn name_of(device: &Device) -> &str {
    match &device.plugin {
        PluginRef::Builtin { name } => name,
        PluginRef::Vst3 { .. } => "",
    }
}

pub fn routing_values(project: &Project) -> Vec<(u64, Vec<(u32, f32)>)> {
    let sounding = lumen_project::edit::freeze::played(project);
    all_tracks(&sounding)
        .flat_map(|track| track.devices.iter())
        .filter(|d| is_builtin(d) && !d.is_instrument())
        .map(|d| (d.id.0, values(d)))
        .collect()
}

pub fn latencies(project: &Project) -> BTreeMap<Id, u32> {
    sounding_latencies(&lumen_project::edit::freeze::played(project))
}

fn sounding_latencies(sounding: &Project) -> BTreeMap<Id, u32> {
    all_tracks(sounding)
        .flat_map(|track| track.devices.iter())
        .filter(|d| is_builtin(d) && !d.is_instrument())
        .filter_map(|d| {
            let identity = identity(name_of(d))?;
            Some((d.id, (identity.timing.latency)(&values(d))))
        })
        .collect()
}

fn revision(project: &Project) -> u64 {
    fn put(hash: &mut u64, word: u64) {
        for byte in word.to_le_bytes() {
            *hash ^= u64::from(byte);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for track in all_tracks(project) {
        put(&mut hash, track.id.0);
        put(&mut hash, u64::from(track.frozen.is_some()));
        for device in &track.devices {
            if !has_table(device) {
                continue;
            }
            put(&mut hash, device.id.0);
            put(&mut hash, u64::from(device.bypass));
            for byte in name_of(device).bytes() {
                put(&mut hash, u64::from(byte));
            }
            for (&id, &value) in &device.params {
                put(&mut hash, u64::from(id));
                put(&mut hash, value.to_bits());
            }
        }
    }
    hash
}

fn describe(device: &Device) -> Vec<ParamInfo> {
    let Some(table) = table(name_of(device)) else {
        return Vec::new();
    };
    table
        .iter()
        .map(|info| {
            let value = device
                .params
                .get(&info.id)
                .copied()
                .unwrap_or_else(|| f64::from(info.normalised(info.default)));
            ParamInfo {
                id: info.id,
                title: info.name.to_owned(),
                units: info.unit.to_owned(),
                steps: info.steps,
                default: f64::from(info.normalised(info.default)),
                flags: if lumen_devices::automatable(name_of(device), info.id) {
                    FLAG_CAN_AUTOMATE
                } else {
                    0
                },
                unit: 0,
                value,
            }
        })
        .collect()
}

pub fn text(device: &Device, param: u32, value: f64) -> Option<String> {
    let table = table(name_of(device))?;
    Some(params::find(table, param)?.text_of(value as f32))
}

pub fn value_of(device: &Device, param: u32, text: &str) -> Option<f64> {
    let table = table(name_of(device))?;
    params::find(table, param)?.value_of(text).map(f64::from)
}

pub fn choices(device: &Device, param: u32) -> Vec<String> {
    let Some(table) = table(name_of(device)) else {
        return Vec::new();
    };
    params::find(table, param)
        .map(|info| info.choices.iter().map(|c| (*c).to_owned()).collect())
        .unwrap_or_default()
}

#[derive(Default)]
pub struct Builtins {
    attached: BTreeMap<Id, String>,
    latency: BTreeMap<Id, u32>,
    params: BTreeMap<Id, Vec<ParamInfo>>,
    meters: BTreeMap<Id, Reduction>,
    readings: BTreeMap<Id, MeterReading>,
    pretend: BTreeMap<Id, MeterReading>,
    waiting: Vec<(Id, Box<Slot>)>,
    spectrums: BTreeMap<Id, analyzer::Reader>,
    refused: BTreeSet<Id>,
    revision: u64,
    note: Option<String>,
}

impl Builtins {
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.params.get(&device).map_or(&[], Vec::as_slice)
    }

    pub fn latencies(&self) -> &BTreeMap<Id, u32> {
        &self.latency
    }

    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    pub fn refused(&self, device: Id) -> bool {
        self.refused.contains(&device)
    }

    pub fn reading(&self, device: Id) -> Option<MeterReading> {
        self.readings
            .get(&device)
            .or_else(|| self.pretend.get(&device))
            .copied()
    }

    pub fn read_meters(&mut self) {
        for (&device, meter) in &self.meters {
            self.readings.insert(device, meter.take());
        }
        for (&device, &reading) in &self.pretend {
            self.readings.insert(device, reading);
        }
    }

    pub fn pretend_reduction(&mut self, device: Id, reading: MeterReading) {
        self.pretend.insert(device, reading);
        self.readings.insert(device, reading);
    }

    pub fn has_meter(&self, device: Id) -> bool {
        self.meters.contains_key(&device)
    }

    pub fn update(&mut self, project: &Project, mixer: &mut MixerHandle) -> bool {
        let revision = revision(project);
        let mut changed = false;
        if revision != self.revision {
            self.revision = revision;
            self.follow(project, mixer);
            changed = true;
        }
        self.take_refused(mixer);
        self.attach_waiting(mixer);
        changed
    }

    fn follow(&mut self, project: &Project, mixer: &mut MixerHandle) {
        let sounding = lumen_project::edit::freeze::played(project);
        let project: &Project = &sounding;
        let mut wanted: Vec<(Id, String)> = Vec::new();
        for track in all_tracks(project) {
            for device in &track.devices {
                if has_table(device) {
                    wanted.push((device.id, name_of(device).to_owned()));
                }
            }
        }
        self.forget_gone(&wanted, mixer);
        for track in all_tracks(project) {
            for device in &track.devices {
                if !is_builtin(device) {
                    continue;
                }
                let id = device.id;
                if self.attached.contains_key(&id) {
                    continue;
                }
                let name = name_of(device);
                let mut tap = None;
                if lumen_devices::watches_spectrum(name) {
                    let (writer, reader) = analyzer::pair();
                    self.spectrums.insert(id, reader);
                    tap = Some(writer);
                }
                let Some((effect, meter)) = lumen_devices::effect_built(name, tap) else {
                    continue;
                };
                self.attached.insert(id, name.to_owned());
                if let Some(meter) = meter {
                    self.meters.insert(id, meter);
                }
                self.refused.remove(&id);
                self.waiting.push((id, Slot::new(effect)));
            }
        }
        self.latency = sounding_latencies(project);
        self.refused
            .retain(|id| wanted.iter().any(|(known, _)| known == id));
        self.params
            .retain(|id, _| wanted.iter().any(|(known, _)| known == id));
        for track in all_tracks(project) {
            for device in &track.devices {
                if !has_table(device) {
                    continue;
                }
                let fresh = describe(device);
                if self.params.get(&device.id) != Some(&fresh) {
                    self.params.insert(device.id, fresh);
                }
            }
        }
    }

    fn forget_gone(&mut self, wanted: &[(Id, String)], mixer: &mut MixerHandle) {
        let gone: Vec<Id> = self
            .attached
            .iter()
            .filter(|(id, name)| {
                !wanted.iter().any(|(wanted, wanted_name)| {
                    wanted == *id && wanted_name.as_str() == name.as_str()
                })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.attached.remove(&id);
            self.params.remove(&id);
            self.latency.remove(&id);
            self.meters.remove(&id);
            self.spectrums.remove(&id);
            mixer.detach_builtin(id.0);
        }
    }

    fn take_refused(&mut self, mixer: &mut MixerHandle) {
        for id in mixer.take_refused_builtins() {
            let id = Id(id);
            let Some(name) = self.attached.remove(&id) else {
                continue;
            };
            self.refused.insert(id);
            let device = identity(&name).map_or("A built-in device", |i| i.name);
            self.note = Some(format!(
                "{device} could not start: the mixer has room for {} built-in devices at once. \
                 It plays its dry signal until there is room, and the rest of the project is heard.",
                lumen_engine::MAX_BUILTINS
            ));
        }
    }

    fn attach_waiting(&mut self, mixer: &mut MixerHandle) {
        let mut still = Vec::new();
        for (id, slot) in std::mem::take(&mut self.waiting) {
            if let Err(slot) = mixer.attach_builtin(id.0, slot) {
                still.push((id, slot));
            }
        }
        self.waiting = still;
    }

    pub(crate) fn attached(&self, device: Id) -> bool {
        self.attached.contains_key(&device)
    }

    pub(crate) fn spectrum(&mut self, device: Id) -> Option<&mut analyzer::Reader> {
        self.spectrums.get_mut(&device)
    }

    pub(crate) fn spectrum_ids(&self) -> Vec<Id> {
        self.spectrums.keys().copied().collect()
    }
}

pub fn attach_all(
    project: &Project,
    routing: &Routing,
    mixer: &mut MixerHandle,
    mut drain: impl FnMut(),
) -> Result<(), String> {
    let named: BTreeSet<u64> = routing
        .nodes
        .iter()
        .flat_map(|node| node.devices.iter())
        .filter_map(|device| match *device {
            lumen_engine::Device::Builtin { device, .. } => Some(device),
            _ => None,
        })
        .collect();
    let sounding = lumen_project::edit::freeze::played(project);
    for track in all_tracks(&sounding) {
        for device in &track.devices {
            if !is_builtin(device) || !named.contains(&device.id.0) {
                continue;
            }
            let Some(effect) = lumen_devices::effect(name_of(device)) else {
                continue;
            };
            let mut slot = Slot::new(effect);
            let mut rounds = 0;
            while let Err(back) = mixer.attach_builtin(device.id.0, slot) {
                slot = back;
                rounds += 1;
                if rounds > 64 {
                    return Err(
                        "The mixer would not take the project's built-in devices".to_owned()
                    );
                }
                drain();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::{Color, Track, TrackKind};

    fn device(name: &str) -> Device {
        Device::new(
            Id(5),
            PluginRef::Builtin {
                name: name.to_owned(),
            },
            false,
            Vec::new(),
        )
    }

    #[test]
    fn a_fresh_utility_is_saved_with_every_parameter_at_its_default() {
        let values = defaults(utility::NAME);
        assert_eq!(values.len(), utility::TABLE.len());
        for (id, value) in &values {
            let info = params::find(utility::TABLE, *id).unwrap();
            assert!((value - info.normalised(info.default)).abs() < 1e-6);
        }
        let gain = values
            .iter()
            .find(|(id, _)| *id == utility::GAIN)
            .unwrap()
            .1;
        assert!((gain - 60.0 / 84.0).abs() < 1e-6, "{gain}");
    }

    #[test]
    fn the_personality_is_a_name_and_what_the_device_is() {
        let prism = identity(utility::NAME).expect("Prism");
        assert_eq!((prism.name, prism.kind), ("Prism", "Utility"));
        assert!(identity("nothing").is_none());
        assert!(
            !is_builtin(&device("synth")),
            "the synth is an instrument, not an effect"
        );
        assert!(is_builtin(&device(utility::NAME)));
        let flare = identity(lumen_project::model::SYNTH).expect("Flare");
        assert_eq!((flare.name, flare.kind), ("Flare", "Polysynth"));
        assert!(has_table(&device(lumen_project::model::SYNTH)));
    }

    #[test]
    fn a_parameter_reads_in_the_device_s_own_words_and_is_read_back() {
        let prism = device(utility::NAME);
        let gain = defaults(utility::NAME)
            .into_iter()
            .find(|(id, _)| *id == utility::GAIN)
            .unwrap()
            .1;
        assert_eq!(
            text(&prism, utility::GAIN, f64::from(gain)).unwrap(),
            "0.0 dB"
        );
        let quieter = value_of(&prism, utility::GAIN, "-6 dB").unwrap();
        assert!(quieter < f64::from(gain));
        assert_eq!(
            text(&prism, utility::GAIN, quieter).unwrap(),
            "\u{2212}6.0 dB"
        );
        assert_eq!(choices(&prism, utility::SWAP), ["Off", "On"]);
        assert_eq!(value_of(&prism, utility::SWAP, "on"), Some(1.0));
        assert!(value_of(&prism, utility::GAIN, "loud").is_none());
        let plugin = Device::vst3(
            Id(6),
            "00112233445566778899AABBCCDDEEFF",
            "Plugin",
            "Vendor",
            lumen_project::model::DeviceKind::Effect,
        );
        assert!(text(&plugin, 1, 0.5).is_none());
        assert!(choices(&plugin, 1).is_empty());
    }

    #[test]
    fn a_project_s_routing_values_are_the_tables_with_the_project_s_own() {
        let mut project = crate::empty_project();
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        let mut prism = device(utility::NAME);
        prism.params.insert(utility::WIDTH, 0.5);
        track.devices = vec![prism.clone()];
        project.tracks = vec![std::sync::Arc::new(track)];
        let values = routing_values(&project);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].0, prism.id.0);
        let width = values[0]
            .1
            .iter()
            .find(|(id, _)| *id == utility::WIDTH)
            .unwrap()
            .1;
        assert_eq!(width, 0.5);
        assert_eq!(latencies(&project).get(&prism.id), Some(&0));
    }

    #[test]
    fn a_latency_that_follows_a_parameter_reaches_the_routing_and_the_plan() {
        use crate::engine::routing::{Cue, Devices, routing};
        let mut project = crate::empty_project();
        let mut timed = device(TEST_DEVICE);
        timed.params.insert(utility::GAIN, 0.0);
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![timed.clone()];
        project.tracks = vec![std::sync::Arc::new(track)];
        assert_eq!(latencies(&project).get(&timed.id), Some(&240));
        let plan = |project: &Project| {
            let devices = Devices {
                builtin: latencies(project),
                ..Devices::default()
            };
            let routing = routing(project, &Cue::default(), &devices, 48_000);
            lumen_engine::compile_routing(&routing).unwrap()
        };
        assert_eq!(plan(&project).compensation().frames, 240);

        let mut loud = timed.clone();
        loud.params.insert(utility::GAIN, 1.0);
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![loud];
        project.tracks = vec![std::sync::Arc::new(track)];
        assert_eq!(latencies(&project).get(&timed.id), Some(&480));
        assert_eq!(plan(&project).compensation().frames, 480);
        assert_eq!(
            plan(&project).compensation().path,
            [(1, 480)],
            "the track is the path that holds it back"
        );
    }

    #[test]
    fn the_running_app_s_latency_follows_a_setting_of_a_device_it_holds() {
        use lumen_engine::{clip_player, mixer};
        let mut timed = device(TEST_DEVICE);
        timed.params.insert(utility::GAIN, 0.0);
        let project_with = |timed: &Device| {
            let mut project = crate::empty_project();
            let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
            track.devices = vec![timed.clone()];
            project.tracks = vec![std::sync::Arc::new(track)];
            project
        };
        let (_clips, player) = clip_player();
        let (mut handle, _source) = mixer(player);
        let mut builtins = Builtins::default();
        let project = project_with(&timed);
        assert!(builtins.update(&project, &mut handle));
        assert_eq!(builtins.latencies().get(&timed.id), Some(&240));

        timed.params.insert(utility::GAIN, 1.0);
        let project = project_with(&timed);
        assert!(
            builtins.update(&project, &mut handle),
            "a new value is a change"
        );
        assert_eq!(builtins.latencies().get(&timed.id), Some(&480));
        assert_eq!(builtins.latencies(), &latencies(&project));
    }

    #[test]
    fn a_limiter_s_lookahead_is_its_latency_and_follows_a_setting() {
        use lumen_devices::dynamics::limiter as horizon;
        use lumen_engine::{clip_player, mixer};
        let mut horizon_device = device(horizon::NAME);
        for (id, value) in defaults(horizon::NAME) {
            horizon_device.params.insert(id, f64::from(value));
        }
        let project_with = |d: &Device| {
            let mut project = crate::empty_project();
            let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
            track.devices = vec![d.clone()];
            project.tracks = vec![std::sync::Arc::new(track)];
            project
        };
        let (_clips, player) = clip_player();
        let (mut handle, _source) = mixer(player);
        let mut builtins = Builtins::default();
        let project = project_with(&horizon_device);
        assert!(builtins.update(&project, &mut handle));
        assert_eq!(builtins.latencies().get(&horizon_device.id), Some(&72));
        assert_eq!(builtins.latencies(), &latencies(&project));

        horizon_device.params.insert(
            horizon::LOOK,
            f64::from(
                params::find(horizon::TABLE, horizon::LOOK)
                    .expect("a parameter")
                    .normalised(4.0),
            ),
        );
        let project = project_with(&horizon_device);
        assert!(builtins.update(&project, &mut handle), "a change");
        assert_eq!(builtins.latencies().get(&horizon_device.id), Some(&192));
        assert_eq!(builtins.latencies(), &latencies(&project));
    }

    #[test]
    fn a_compressor_hands_its_meter_to_the_interface() {
        use lumen_devices::dynamics::compressor as gravity;
        use lumen_engine::{clip_player, mixer};
        let mut compressor = device(gravity::NAME);
        for (id, value) in defaults(gravity::NAME) {
            compressor.params.insert(id, f64::from(value));
        }
        let mut project = crate::empty_project();
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![compressor.clone()];
        project.tracks = vec![std::sync::Arc::new(track)];
        let (_clips, player) = clip_player();
        let (mut handle, _source) = mixer(player);
        let mut builtins = Builtins::default();
        assert!(builtins.update(&project, &mut handle));
        assert!(builtins.has_meter(compressor.id), "a meter to read");
        builtins.read_meters();
        assert_eq!(
            builtins.reading(compressor.id).map(|r| r.reduction_db),
            Some(0.0),
            "nothing is playing, so nothing was taken away"
        );
        let mut prism = device(utility::NAME);
        for (id, value) in defaults(utility::NAME) {
            prism.params.insert(id, f64::from(value));
        }
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![prism.clone()];
        project.tracks = vec![std::sync::Arc::new(track)];
        assert!(builtins.update(&project, &mut handle));
        assert!(!builtins.has_meter(prism.id));
    }

    #[test]
    fn every_built_in_device_has_a_personality() {
        for description in lumen_devices::EFFECTS {
            let identity = identity(description.name)
                .unwrap_or_else(|| panic!("{} has no personality", description.name));
            assert_eq!(identity.timing.name, description.name);
        }
        for (name, identity) in IDENTITIES {
            assert_eq!(identity.timing.name, *name);
            if *name != TEST_DEVICE {
                assert!(
                    lumen_devices::effect_description(name).is_some(),
                    "{name} is not a device"
                );
            }
        }
    }

    #[test]
    fn a_still_project_does_nothing() {
        use lumen_engine::{Source, clip_player, mixer};
        let mut project = crate::empty_project();
        let mut prism = device(utility::NAME);
        for (id, value) in defaults(utility::NAME) {
            prism.params.insert(id, f64::from(value));
        }
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![prism.clone()];
        project.tracks = vec![std::sync::Arc::new(track)];
        let (_clips, player) = clip_player();
        let (mut handle, mut source) = mixer(player);
        let mut builtins = Builtins::default();
        assert!(
            builtins.update(&project, &mut handle),
            "the first frame builds it"
        );
        assert!(
            !builtins.update(&project, &mut handle),
            "a still project changes nothing"
        );
        let mut loud = prism;
        loud.params.insert(utility::GAIN, 0.25);
        let mut track = Track::new(Id(1), "Audio".to_owned(), TrackKind::Audio, Color(0));
        track.devices = vec![loud];
        project.tracks = vec![std::sync::Arc::new(track)];
        assert!(builtins.update(&project, &mut handle), "the edit is seen");
        assert!(!builtins.update(&project, &mut handle), "and only the once");
        source.idle(48_000, 128);
        assert!(builtins.attached(Id(5)));
    }

    #[test]
    fn a_render_attaches_more_built_ins_than_the_queue_holds() {
        use lumen_engine::{Destination, NodeSpec, Routing, Source, clip_player, mixer};
        const RATE: u32 = 48_000;
        let total = lumen_engine::MAX_BUILTINS + 4;
        let mut project = crate::empty_project();
        let mut tracks = Vec::with_capacity(total);
        for k in 0..total {
            let mut track = Track::new(
                Id(1_000 + k as u64),
                format!("Track {k}"),
                TrackKind::Audio,
                Color(0),
            );
            let mut prism = Device::new(
                Id(5_000 + k as u64),
                PluginRef::Builtin {
                    name: utility::NAME.to_owned(),
                },
                false,
                Vec::new(),
            );
            for (id, value) in defaults(utility::NAME) {
                prism.params.insert(id, f64::from(value));
            }
            track.devices = vec![prism];
            tracks.push(std::sync::Arc::new(track));
        }
        project.tracks = tracks;
        let mut node = NodeSpec::unity(1, Destination::Hardware(0));
        node.devices = (0..total)
            .map(|k| lumen_engine::Device::Builtin {
                device: 5_000 + k as u64,
                latency: 0,
                bypass: false,
            })
            .collect();
        let routing = Routing {
            nodes: vec![node],
            cue: None,
            measure: None,
        };
        let (_clips, player) = clip_player();
        let (mut handle, mut source) = mixer(player);
        attach_all(&project, &routing, &mut handle, || source.idle(RATE, 256))
            .expect("every device attaches");
        source.idle(RATE, 256);
        let refused = handle.take_refused_builtins();
        assert_eq!(refused.len(), total - lumen_engine::MAX_BUILTINS);
        assert!(
            handle.take_refused_builtins().is_empty(),
            "a refusal is reported once"
        );
    }

    #[test]
    fn the_automatic_tail_is_what_the_project_s_devices_ring_on_for() {
        use lumen_devices::{delay, reverb};
        let mut project = crate::empty_project();
        let one = |project: &mut Project, name: &str, id: u64| {
            let mut track = Track::new(Id(id), "Track".to_owned(), TrackKind::Audio, Color(0));
            track.devices = vec![device(name)];
            project.tracks = vec![std::sync::Arc::new(track)];
        };
        one(&mut project, utility::NAME, 1);
        assert_eq!(tail_frames(&project, 48_000), 0);
        let mut reverb_of = |decay: f32| {
            let mut verb = device(reverb::NAME);
            verb.params.insert(
                reverb::DECAY,
                f64::from(
                    params::find(reverb::TABLE, reverb::DECAY)
                        .unwrap()
                        .normalised(decay),
                ),
            );
            let mut track = Track::new(Id(2), "Verb".to_owned(), TrackKind::Audio, Color(0));
            track.devices = vec![verb];
            project.tracks = vec![std::sync::Arc::new(track)];
            tail_frames(&project, 48_000)
        };
        let short = reverb_of(0.5);
        let long = reverb_of(4.0);
        assert!(short > 0, "a reverb rings on");
        assert!(long > short, "{short} then {long}");
        assert!(long <= 10 * 48_000, "{long}");
        let mut echo = device(delay::NAME);
        echo.params.insert(
            delay::TIME,
            f64::from(
                params::find(delay::TABLE, delay::TIME)
                    .unwrap()
                    .normalised(500.0),
            ),
        );
        let track = Id(3);
        let mut node = Track::new(track, "Echo".to_owned(), TrackKind::Audio, Color(0));
        node.devices = vec![echo];
        project.tracks = vec![std::sync::Arc::new(node)];
        let echoed = tail_frames(&project, 48_000);
        assert!(echoed > 48_000, "half a second of echoes: {echoed}");
        assert_eq!(track_tail_frames(&project, track, 48_000), echoed);
        let mut verb = device(reverb::NAME);
        verb.params.insert(
            reverb::DECAY,
            f64::from(
                params::find(reverb::TABLE, reverb::DECAY)
                    .unwrap()
                    .normalised(1.0),
            ),
        );
        project.master = std::sync::Arc::new({
            let mut master = (*project.master).clone();
            master.devices = vec![verb];
            master
        });
        let both = tail_frames(&project, 48_000);
        assert!(both > echoed, "{both} against {echoed}");
        assert_eq!(track_tail_frames(&project, track, 48_000), echoed);
    }
}
