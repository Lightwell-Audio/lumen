//! Built-in devices in the app (M8 slice B0, ADR-XXXX).
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

use std::collections::BTreeMap;

use eframe::egui::Color32;
use lumen_devices::params::{self, ParamInfo as Info};
use lumen_devices::utility;
use lumen_engine::{MixerHandle, Slot};
use lumen_hosting::{FLAG_CAN_AUTOMATE, ParamInfo};
use lumen_project::model::{Device, Id, PluginRef, Project};
use lumen_project::routing::all_tracks;

use crate::theme;

/// What a built-in device is, as a person reads it: its personality's name,
/// what it is, and its signature colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The name of its own ("Prism").
    pub name: &'static str,
    /// What it is ("Utility"): the card's subtitle, and the plain name in
    /// tooltips and the parameter list.
    pub kind: &'static str,
    /// Its signature colour (`theme::personality`).
    pub colour: Color32,
}

/// The stable names of the built-in effects this build knows, with the
/// personality each has.
const IDENTITIES: &[(&str, Identity)] = &[(
    utility::NAME,
    Identity {
        name: "Prism",
        kind: "Utility",
        colour: theme::personality::UTILITY,
    },
)];

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

/// Whether `device` is a built-in effect this build can run.
pub fn is_builtin(device: &Device) -> bool {
    is_builtin_effect(&device.plugin)
}

/// The table a device of `name` declares.
pub fn table(name: &str) -> Option<&'static [Info]> {
    lumen_devices::effect_params(name)
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

/// The latency each built-in device reports, in frames, for the routing's
/// delay compensation. Read from the device's own build, so a device whose
/// latency depends on a setting says so. A frozen track's devices do not play
/// (its file does), so they are left out, as the routing leaves them out.
pub fn latencies(project: &Project) -> BTreeMap<Id, u32> {
    let sounding = lumen_project::edit::freeze::played(project);
    all_tracks(&sounding)
        .flat_map(|track| track.devices.iter())
        .filter(|d| is_builtin(d))
        .filter_map(|d| lumen_devices::effect(name_of(d)).map(|effect| (d.id, effect.latency())))
        .collect()
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
    /// What is attached, by device id.
    attached: BTreeMap<Id, String>,
    /// Frames each built-in reports its output trails its input by (M8 slice
    /// B0), read from the effect when it is built, for the routing's delay
    /// compensation.
    latency: BTreeMap<Id, u32>,
    /// The parameters of each built-in device, in the shape a plugin's arrive
    /// in, so the chain never has to build them again in a frame.
    params: BTreeMap<Id, Vec<ParamInfo>>,
    /// Built-ins the mixer's queue had no room for: tried again next frame.
    waiting: Vec<(Id, Box<Slot>)>,
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

    /// Keeps the attached built-ins in step with the project: one is built and
    /// handed over when its device appears, and taken away when it goes.
    pub fn update(&mut self, project: &Project, mixer: &mut MixerHandle) {
        // A frozen track plays its file and runs no device, as the routing
        // has it.
        let sounding = lumen_project::edit::freeze::played(project);
        let project: &Project = &sounding;
        let mut wanted: Vec<(Id, String)> = Vec::new();
        for track in all_tracks(project) {
            for device in &track.devices {
                if is_builtin(device) {
                    wanted.push((device.id, name_of(device).to_owned()));
                }
            }
        }
        self.forget_gone(&wanted, mixer);
        for (id, name) in &wanted {
            if self.attached.contains_key(id) {
                continue;
            }
            let Some(effect) = lumen_devices::effect(name) else {
                continue;
            };
            self.attached.insert(*id, name.clone());
            self.latency.insert(*id, effect.latency());
            self.waiting.push((*id, Slot::new(effect)));
        }
        self.latency
            .retain(|id, _| wanted.iter().any(|(known, _)| known == id));
        // The tables the interface reads, for the devices the project holds.
        self.params
            .retain(|id, _| wanted.iter().any(|(known, _)| known == id));
        for track in all_tracks(project) {
            for device in &track.devices {
                if !is_builtin(device) {
                    continue;
                }
                let fresh = describe(device);
                if self.params.get(&device.id) != Some(&fresh) {
                    self.params.insert(device.id, fresh);
                }
            }
        }
        self.attach_waiting(mixer);
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
            // A queue with no room leaves the device playing for a block
            // more, which is the fallback's silence until the plan drops it.
            mixer.detach_builtin(id.0);
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
}

/// Builds every built-in effect in `project` and attaches them to `mixer`, for
/// a render with no interface behind it (an export, a bounce).
pub fn attach_all(project: &Project, mixer: &mut MixerHandle) -> Result<(), String> {
    let sounding = lumen_project::edit::freeze::played(project);
    for track in all_tracks(&sounding) {
        for device in &track.devices {
            if !is_builtin(device) {
                continue;
            }
            let Some(effect) = lumen_devices::effect(name_of(device)) else {
                continue;
            };
            mixer
                .attach_builtin(device.id.0, Slot::new(effect))
                .map_err(|_| "Too many built-in devices to render at once".to_owned())?;
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
}
