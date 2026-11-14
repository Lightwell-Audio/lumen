//! Built-in devices in the app (M8 slice B0, ADR-0145).
//!
//! A built-in effect is Lumen's own DSP, so the interface builds it (its
//! constructor allocates) and hands it to the audio thread through the mixer,
//! as a plugin's host is handed over — never the other way round. Its
//! parameters are a static table ([`lumen_devices::params`]) turned into the
//! same [`ParamInfo`] a plugin's parameters arrive in, so the card's strip,
//! All parameters, automation lanes, MIDI learn, overrides and presets are the
//! same code for a built-in as for a plugin.
//!
//! The values the project holds travel with the routing (a knob reaches the
//! audio thread with the next block); what is here is what the interface reads:
//! the tables, their words for a value, and which devices were built.
//!
//! Each built-in has a **personality** (owner, 2026-11-12): a name of its own,
//! with what it is as a subtitle, and a signature colour (DESIGN.md §7k).

pub mod samples;

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::Color32;
use lumen_devices::analyzer;
use lumen_devices::params::{self, ParamInfo as Info};
use lumen_devices::{Description, eq, sampler, utility};
use lumen_engine::{MixerHandle, Routing, Slot};
use lumen_hosting::{FLAG_CAN_AUTOMATE, ParamInfo};
use lumen_project::model::{Device, Id, PluginRef, Project};
use lumen_project::routing::all_tracks;

use crate::theme;

/// What a built-in device is, as a person reads it: its personality's name,
/// what it is, its signature colour, and the description its table and timing
/// come from.
#[derive(Clone, Copy)]
pub struct Identity {
    /// The name of its own ("Prism").
    pub name: &'static str,
    /// What it is ("Utility"): the plain name in tooltips and the parameter
    /// list.
    pub kind: &'static str,
    /// What the card says under its name: what it is, and what it does
    /// ("Utility · gain, width, mono").
    pub subtitle: &'static str,
    /// Its signature colour (`theme::personality`).
    pub colour: Color32,
    /// Its table, and how its latency and tail follow its values (M8 slice B0,
    /// review H1: a pure function of the values, so the routing follows a
    /// setting exactly as it follows a plugin's reported latency).
    pub timing: &'static Description,
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.kind == other.kind && self.colour == other.colour
    }
}

impl Eq for Identity {}

/// The stable names of the built-in effects this build knows, with the
/// personality each has.
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
        sampler::NAME,
        Identity {
            name: "Shutter",
            kind: "Sampler",
            subtitle: "Sampler",
            colour: theme::personality::SHUTTER,
            timing: &sampler::DESCRIPTION,
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

/// A device that exists only in tests (M8 slice B0, review H1): its latency
/// follows one of its parameters, so a latency that is a function of the
/// values can be followed from the project to the routing and the plan.
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

/// What `name` is as a person reads it, if it is a built-in effect this build
/// knows.
pub fn identity(name: &str) -> Option<Identity> {
    IDENTITIES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, identity)| *identity)
}

/// Whether `plugin` is a built-in effect this build knows how to run.
pub fn is_builtin_effect(plugin: &PluginRef) -> bool {
    matches!(plugin, PluginRef::Builtin { name } if identity(name).is_some())
}

/// Whether `device` is a built-in effect this build can run (the mixer's
/// built-in path, M8 slice B0). A built-in instrument is not one: nothing in
/// the audio path delays it, it makes the sound.
pub fn is_builtin(device: &Device) -> bool {
    is_builtin_effect(&device.plugin)
        && identity(name_of(device)).is_some()
        && lumen_devices::instrument_params(name_of(device)).is_none()
}

/// Whether `device` is a built-in instrument this build can run: the synth or
/// the sampler, whose sound the interface builds (M8 slice B5). Its
/// parameters are its table's, as an effect's are, but the mixer runs no
/// device for it.
pub fn is_builtin_instrument(device: &Device) -> bool {
    matches!(&device.plugin, PluginRef::Builtin { name } if lumen_devices::instrument_params(name).is_some())
}

/// Whether `device` is a built-in of any kind this build knows.
pub fn is_builtin_device(device: &Device) -> bool {
    is_builtin(device) || is_builtin_instrument(device)
}

/// The table a device of `name` declares.
pub fn table(name: &str) -> Option<&'static [Info]> {
    identity(name).map(|identity| identity.timing.table)
}

/// The values a fresh device of `name` starts with: every parameter at the
/// table's default, normalised. A project holds them, so a control a lane
/// takes over shows the value that was there.
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

/// What the project holds for a device's parameters, normalised: its saved
/// values, and the table's default for any the project does not name (a
/// parameter a later build added).
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

/// The name a built-in device is saved under; empty for anything else.
pub fn name_of(device: &Device) -> &str {
    match &device.plugin {
        PluginRef::Builtin { name } => name,
        PluginRef::Vst3 { .. } => "",
    }
}

/// Every built-in effect in `project`, with its values, by device id: what the
/// audio thread is given with the routing. A frozen track contributes nothing.
pub fn routing_values(project: &Project) -> Vec<(u64, Vec<(u32, f32)>)> {
    let sounding = lumen_project::edit::freeze::played(project);
    all_tracks(&sounding)
        .flat_map(|track| track.devices.iter())
        .filter(|d| is_builtin(d))
        .map(|d| (d.id.0, values(d)))
        .collect()
}

/// The latency each built-in device reports at the values the project holds,
/// in frames, for the routing's delay compensation. Read from the device's
/// description and the project's values — never from a throwaway instance
/// (M8 slice B0, review H1) — so a setting that changes the latency changes
/// the routing exactly as a plugin's reported latency change does, and an
/// export or a bounce (with no interface behind it) gets the same answer.
/// A frozen track's devices do not play (its file does), so they are left out,
/// as the routing leaves them out.
pub fn latencies(project: &Project) -> BTreeMap<Id, u32> {
    sounding_latencies(&lumen_project::edit::freeze::played(project))
}

/// [`latencies`] of a project whose frozen tracks are already left out: the
/// one function both the running app (`Builtins`) and the renders read, so the
/// two cannot disagree (review H1's re-check, N1).
fn sounding_latencies(sounding: &Project) -> BTreeMap<Id, u32> {
    all_tracks(sounding)
        .flat_map(|track| track.devices.iter())
        .filter(|d| is_builtin(d))
        .filter_map(|d| {
            let identity = identity(name_of(d))?;
            Some((d.id, (identity.timing.latency)(&values(d))))
        })
        .collect()
}

/// What the project holds for the devices `update` cares about, in one number
/// (M8 slice B0, review L7): every built-in's id, name, bypass and values, and
/// which tracks are frozen (a frozen track's devices are not played). When it
/// has not changed there is nothing for `Builtins` to do, so a still project
/// costs one pass and no allocation.
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
            if !is_builtin_device(device) {
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

/// The parameters of `device` in the shape a plugin's arrive in.
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
                flags: FLAG_CAN_AUTOMATE,
                unit: 0,
                value,
            }
        })
        .collect()
}

/// The words a built-in has for a value of one of its parameters, or `None`
/// when it is not a built-in this build knows.
pub fn text(device: &Device, param: u32, value: f64) -> Option<String> {
    let table = table(name_of(device))?;
    Some(params::find(table, param)?.text_of(value as f32))
}

/// The value of a parameter from what a person typed, normalised; `None` when
/// the text says nothing that parameter understands.
pub fn value_of(device: &Device, param: u32, text: &str) -> Option<f64> {
    let table = table(name_of(device))?;
    params::find(table, param)?.value_of(text).map(f64::from)
}

/// The words of each step of a stepped parameter, for a chooser's list.
pub fn choices(device: &Device, param: u32) -> Vec<String> {
    let Some(table) = table(name_of(device)) else {
        return Vec::new();
    };
    params::find(table, param)
        .map(|info| info.choices.iter().map(|c| (*c).to_owned()).collect())
        .unwrap_or_default()
}

/// The built-in effects handed to the audio thread, and the tables the
/// interface reads them through.
#[derive(Default)]
pub struct Builtins {
    /// What is attached, by device id, and the name it is attached as.
    attached: BTreeMap<Id, String>,
    /// Frames each built-in reports its output trails its input by (M8 slice
    /// B0), read from its description and the values the project holds, for
    /// the routing's delay compensation.
    latency: BTreeMap<Id, u32>,
    /// The parameters of each built-in device, in the shape a plugin's arrive
    /// in, so the chain never has to build them again in a frame.
    params: BTreeMap<Id, Vec<ParamInfo>>,
    /// Built-ins the mixer's queue had no room for: tried again next frame.
    waiting: Vec<(Id, Box<Slot>)>,
    /// The spectrum windows of the built-in devices that fill one, by device
    /// id (M8 slice B1): the device writes, the face reads. A device the pool
    /// refused keeps its window; it fills nothing while it is not running.
    spectrums: BTreeMap<Id, analyzer::Reader>,
    /// The devices the mixer's pool had no room for (M8 slice B0, review M3):
    /// the audio thread gave their slots back, so they are not running and the
    /// person is told once, in full.
    refused: BTreeSet<Id>,
    /// What the last update saw of the project, in one number (review L7): a
    /// still project does no work at all.
    revision: u64,
    /// A sentence to show, until the frame reads it.
    note: Option<String>,
}

impl Builtins {
    /// A built-in's parameters as the chain reads them: empty for a plugin,
    /// whose list the host keeps.
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.params.get(&device).map_or(&[], Vec::as_slice)
    }

    /// The frames each attached built-in reports, for the routing.
    pub fn latencies(&self) -> &BTreeMap<Id, u32> {
        &self.latency
    }

    /// What the person should be told, once (a device the mixer had no room
    /// for). Read in full.
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// Whether the device is not running because the mixer had no room for it.
    pub fn refused(&self, device: Id) -> bool {
        self.refused.contains(&device)
    }

    /// Keeps the attached built-ins in step with the project: one is built and
    /// handed over when its device appears, and taken away when it goes.
    /// Returns whether what the routing reads changed (a device's latency), so
    /// the caller only copies the latencies when they can differ.
    pub fn update(&mut self, project: &Project, mixer: &mut MixerHandle) -> bool {
        let revision = revision(project);
        let mut changed = false;
        if revision != self.revision {
            self.revision = revision;
            self.follow(project, mixer);
            changed = true;
        }
        // A device the pool refused (review M3) and one whose queue was full
        // are retried every frame, whatever else changed.
        self.take_refused(mixer);
        self.attach_waiting(mixer);
        changed
    }

    /// Devices that came, went or changed what they are.
    fn follow(&mut self, project: &Project, mixer: &mut MixerHandle) {
        // A frozen track plays its file and runs no device, as the routing
        // has it.
        let sounding = lumen_project::edit::freeze::played(project);
        let project: &Project = &sounding;
        let mut wanted: Vec<(Id, String)> = Vec::new();
        for track in all_tracks(project) {
            for device in &track.devices {
                if is_builtin_device(device) {
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
                // A device that analyses its own audio is built with the
                // window its face reads (M8 slice B1); the interface keeps the
                // reader and hands over the writer.
                let mut tap = None;
                if lumen_devices::watches_spectrum(name) {
                    let (writer, reader) = analyzer::pair();
                    self.spectrums.insert(id, reader);
                    tap = Some(writer);
                }
                let Some(effect) = lumen_devices::effect_tapped(name, tap) else {
                    continue;
                };
                self.attached.insert(id, name.to_owned());
                // A device the pool refused is tried again when the project
                // changes; if the pool refuses it again, `take_refused` says
                // so again (re-check N5: it no longer reads "not running"
                // after it starts).
                self.refused.remove(&id);
                self.waiting.push((id, Slot::new(effect)));
            }
        }
        // Every device's latency, attached or new, is worked out again from
        // the values the project holds (review H1 and its re-check N1): a
        // setting that moves it is a new routing with the new number, as an
        // export of the same project has.
        self.latency = sounding_latencies(project);
        // A refused device that left the project is forgotten with it.
        self.refused
            .retain(|id| wanted.iter().any(|(known, _)| known == id));
        // The tables the interface reads, for the devices the project holds.
        self.params
            .retain(|id, _| wanted.iter().any(|(known, _)| known == id));
        for track in all_tracks(project) {
            for device in &track.devices {
                if !is_builtin_device(device) {
                    continue;
                }
                let fresh = describe(device);
                if self.params.get(&device.id) != Some(&fresh) {
                    self.params.insert(device.id, fresh);
                }
            }
        }
    }

    /// The effects of devices that left the project, or changed what they are.
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
            self.spectrums.remove(&id);
            // A queue with no room leaves the device playing for a block
            // more, which is the fallback's silence until the plan drops it.
            mixer.detach_builtin(id.0);
        }
    }

    /// Built-in effects the mixer's pool had no room for (review M3): they are
    /// not running, and the person is told once, with what it means.
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

    /// Effects the mixer's queue had no room for last time.
    fn attach_waiting(&mut self, mixer: &mut MixerHandle) {
        let mut still = Vec::new();
        for (id, slot) in std::mem::take(&mut self.waiting) {
            if let Err(slot) = mixer.attach_builtin(id.0, slot) {
                still.push((id, slot));
            }
        }
        self.waiting = still;
    }

    /// Whether the effect of `device` is attached now.
    pub(crate) fn attached(&self, device: Id) -> bool {
        self.attached.contains_key(&device)
    }

    /// The spectrum window of `device`, when it is a built-in that fills one
    /// (M8 slice B1).
    pub(crate) fn spectrum(&mut self, device: Id) -> Option<&mut analyzer::Reader> {
        self.spectrums.get_mut(&device)
    }

    /// Every built-in's window by id, so the interface can silence the ones no
    /// face is drawn for.
    pub(crate) fn spectrum_ids(&self) -> Vec<Id> {
        self.spectrums.keys().copied().collect()
    }
}

/// Builds the built-in effects a render's routing names and attaches them to
/// `mixer`, for a render with no interface behind it (an export, a bounce).
///
/// A render's queue is finite and nothing drains it before the first block, so
/// a project with more built-ins than the queue holds would fail on the 65th
/// (M8 slice B0, review H2). `drain` is called when the queue is full: it runs
/// the mixer idle for a block, which is how the audio side takes the devices
/// (an export's callback does this itself). Only the built-ins the routing
/// names are attached: a bounce attaches its one track's devices, not every
/// device in the project.
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
            // Give the audio side a block whenever the queue is full, so a
            // project of any size attaches; a bounded number of rounds keeps a
            // mixer that is not running from looping forever.
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
        // Gain is 0 dB of −60 to +24: not the middle.
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
        assert!(!is_builtin(&device("synth")));
        assert!(is_builtin(&device(utility::NAME)));
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
        // A plugin has none of these.
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

    /// H1: a device whose latency follows a parameter has that latency in the
    /// project's latencies, in the routing and in the compiled plan, and a
    /// change of the parameter changes all three — exactly as a host's
    /// reported latency change does.
    #[test]
    fn a_latency_that_follows_a_parameter_reaches_the_routing_and_the_plan() {
        use crate::engine::routing::{Cue, Devices, routing};
        let mut project = crate::empty_project();
        let mut timed = device(TEST_DEVICE);
        // The gain at its lowest: 240 frames.
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

        // The setting moves: so does the plan's timing, with no device built.
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

    /// The re-check's N1: the running app's latencies follow a setting of a
    /// device that is already in the project, not only a new one, so the live
    /// routing is the one an export of the same project builds.
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

    /// Every device the engine can run has a personality, and every personality
    /// is a device the engine can run: the two lists cannot drift apart.
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
                    lumen_devices::effect_description(name).is_some()
                        || lumen_devices::instrument_params(name).is_some(),
                    "{name} is not a device"
                );
            }
        }
        // And every built-in instrument that has a table has one too (M8
        // slice B5); the synth's own personality arrives with its controls.
        for (name, table) in lumen_devices::INSTRUMENTS {
            if table.is_empty() {
                continue;
            }
            let identity = identity(name).unwrap_or_else(|| panic!("{name} has no personality"));
            let pinned: Vec<(u32, &str)> = identity
                .timing
                .table
                .iter()
                .map(|info| (info.id, info.name))
                .collect();
            let listed: Vec<(u32, &str)> = table.iter().map(|info| (info.id, info.name)).collect();
            assert_eq!(pinned, listed);
        }
    }

    /// L7: a still project does nothing at all here. The first frame builds
    /// what the project holds; the next, with nothing changed, allocates
    /// nothing and hands over nothing; one edit is noticed again.
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
        // A value the person moved is an edit: it is noticed, and the device
        // is not built again.
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

    /// H2: a render attaches more built-ins than the mixer's queue holds. The    /// queue is drained while attaching, so a project of thousands of devices
    /// cannot fail on the 65th (M8 slice B0, review H2); the ones past the
    /// pool come back as refused, so the interface can say which are not
    /// running, and nothing is dropped on the audio thread.
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
        // A routing whose one node names every device: what `attach_all` reads.
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
        // One more block takes the last of them, which the pool refuses.
        source.idle(RATE, 256);
        let refused = handle.take_refused_builtins();
        assert_eq!(refused.len(), total - lumen_engine::MAX_BUILTINS);
        assert!(
            handle.take_refused_builtins().is_empty(),
            "a refusal is reported once"
        );
    }
}
