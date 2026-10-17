//! A track's device chain (M6 slice 2, format v17): adding a plugin, taking
//! one away and bypassing one. Each is one undo step, and every refusal says
//! why, so the interface can show the reason. A MIDI track has at most one
//! instrument, always first; an effect goes anywhere after it.

use std::fmt;

use lumen_project::model::{Device, DeviceKind, Id, PluginRef, SYNTH, TrackKind};

use super::Document;

/// Where an added effect goes in a chain (the owner's D5: a double-click puts
/// it after the selected card or at the end; a drag drops it where the
/// preview shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    End,
    /// Right after this device.
    After(Id),
    /// At this position (past the end means the end).
    At(usize),
}

/// Why a device edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The track is gone.
    NoTrack,
    /// The master track takes devices with chain editing (slice 3).
    Unsupported,
    InstrumentNeedsMidi,
    BeforeInstrument,
    /// The device is gone.
    NoDevice,
    /// The plugin's identity could not be saved.
    BadPlugin,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::NoTrack => "That track is gone.",
            Refusal::Unsupported => "The master track takes devices in a later update.",
            Refusal::InstrumentNeedsMidi => "An instrument needs a MIDI track.",
            Refusal::BeforeInstrument => "An effect can't go before the instrument.",
            Refusal::NoDevice => "That device is gone.",
            Refusal::BadPlugin => "That plugin can't be added: its identity is not valid.",
        })
    }
}

/// What the interface calls a device in a menu or an undo label.
pub fn name_of(device: &Device) -> String {
    match &device.plugin {
        PluginRef::Vst3 { name, .. } => name.clone(),
        PluginRef::Builtin { name } if name == SYNTH => "Synth".to_owned(),
        PluginRef::Builtin { name } => name.clone(),
    }
}

fn valid_vst3(class_id: &str, name: &str) -> bool {
    class_id.len() == 32 && class_id.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty()
}

/// A device as the browser makes it: the synth starts on the default
/// preset, a plugin with no state.
fn fresh_device(id: Id, plugin: PluginRef) -> Device {
    match &plugin {
        PluginRef::Builtin { name } if name == SYNTH => {
            Device::synth(id, super::instrument::DEFAULT_PRESET.id())
        }
        _ => Device::new(id, plugin, false, Vec::new()),
    }
}

impl Document {
    /// Makes a track for `plugin` (a MIDI track for an instrument, an audio
    /// track for an effect) after `after`, with the plugin as its first
    /// device, as one undo step (the owner's D3). Returns the track and the
    /// device.
    pub fn add_device_on_new_track(
        &mut self,
        plugin: PluginRef,
        after: Option<Id>,
    ) -> Result<(Id, Id), Refusal> {
        if let PluginRef::Vst3 { class_id, name, .. } = &plugin
            && !valid_vst3(class_id, name)
        {
            return Err(Refusal::BadPlugin);
        }
        let kind = match plugin.kind() {
            DeviceKind::Instrument => TrackKind::Midi,
            DeviceKind::Effect => TrackKind::Audio,
        };
        let id = self.fresh_id();
        let track = self.insert_track(kind, after, Some(fresh_device(id, plugin)));
        Ok((track, id))
    }

    /// Adds `plugin` to `track`'s chain. An instrument goes first and takes
    /// the place of the one there (the built-in synth or another plugin),
    /// and a track that was playing to a MIDI output plays it again; an
    /// effect goes at `slot`, never before the instrument. Returns the new
    /// device's id.
    pub fn add_device(&mut self, track: Id, plugin: PluginRef, slot: Slot) -> Result<Id, Refusal> {
        if let PluginRef::Vst3 { class_id, name, .. } = &plugin
            && !valid_vst3(class_id, name)
        {
            return Err(Refusal::BadPlugin);
        }
        if self.project().master.id == track {
            return Err(Refusal::Unsupported);
        }
        let current = self
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .ok_or(Refusal::NoTrack)?;
        let kind = plugin.kind();
        let replaced = current.devices.iter().any(|d| d.is_instrument());
        let index = match kind {
            DeviceKind::Instrument => {
                if current.kind != TrackKind::Midi {
                    return Err(Refusal::InstrumentNeedsMidi);
                }
                0
            }
            DeviceKind::Effect => {
                let len = current.devices.len();
                let wanted = match slot {
                    Slot::End => len,
                    Slot::After(device) => {
                        current
                            .devices
                            .iter()
                            .position(|d| d.id == device)
                            .ok_or(Refusal::NoDevice)?
                            + 1
                    }
                    Slot::At(at) => at.min(len),
                };
                let floor = current
                    .devices
                    .iter()
                    .position(|d| d.is_instrument())
                    .map_or(0, |at| at + 1);
                if wanted < floor {
                    return Err(Refusal::BeforeInstrument);
                }
                wanted
            }
        };
        let id = self.fresh_id();
        let device = fresh_device(id, plugin);
        let name = name_of(&device);
        let label = match (kind, replaced) {
            (DeviceKind::Instrument, true) => format!("Replace instrument with {name}"),
            _ => format!("Add {name}"),
        };
        self.edit_tracks(&label, &[track], |t| {
            if kind == DeviceKind::Instrument {
                t.devices.retain(|d| !d.is_instrument());
                t.midi_to = None;
            }
            t.devices.insert(index.min(t.devices.len()), device.clone());
        });
        Ok(id)
    }

    /// Takes `device` off `track`'s chain.
    pub fn remove_device(&mut self, track: Id, device: Id) -> Result<(), Refusal> {
        let name = self.device_name(track, device)?;
        self.edit_tracks(&format!("Remove {name}"), &[track], |t| {
            t.devices.retain(|d| d.id != device)
        });
        Ok(())
    }

    /// Bypasses `device` (`true`) or turns it on again. A device already in
    /// that state is left alone and nothing is recorded.
    pub fn set_device_bypass(
        &mut self,
        track: Id,
        device: Id,
        bypass: bool,
    ) -> Result<(), Refusal> {
        let name = self.device_name(track, device)?;
        let label = if bypass {
            format!("Bypass {name}")
        } else {
            format!("Enable {name}")
        };
        self.edit_tracks(&label, &[track], |t| {
            if let Some(d) = t.devices.iter_mut().find(|d| d.id == device) {
                d.bypass = bypass;
            }
        });
        Ok(())
    }

    fn device_name(&self, track: Id, device: Id) -> Result<String, Refusal> {
        let track = self
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .ok_or(Refusal::NoTrack)?;
        track
            .devices
            .iter()
            .find(|d| d.id == device)
            .map(name_of)
            .ok_or(Refusal::NoDevice)
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::Track;

    use super::*;

    const CLASS: &str = "ABCDEF0123456789ABCDEF0123456789";
    /// An id no track or device has.
    const GONE: Id = Id(9_999_999);

    fn plugin(name: &str, kind: DeviceKind) -> PluginRef {
        PluginRef::Vst3 {
            class_id: CLASS.into(),
            name: name.into(),
            vendor: "Vendor A".into(),
            kind,
        }
    }

    /// A document with a MIDI track (with the synth) and an audio track.
    fn doc() -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let midi = doc.add_track(TrackKind::Midi, None);
        let audio = doc.add_track(TrackKind::Audio, None);
        (doc, midi, audio)
    }

    fn track(doc: &Document, id: Id) -> &Track {
        doc.project().tracks.iter().find(|t| t.id == id).unwrap()
    }

    fn names(doc: &Document, id: Id) -> Vec<String> {
        track(doc, id).devices.iter().map(name_of).collect()
    }

    #[test]
    fn an_effect_goes_to_the_end_and_undo_takes_it_off() {
        let (mut doc, _, audio) = doc();
        let id = doc
            .add_device(audio, plugin("Room Reverb", DeviceKind::Effect), Slot::End)
            .unwrap();
        assert_eq!(names(&doc, audio), ["Room Reverb"]);
        assert_eq!(track(&doc, audio).devices[0].id, id);
        assert_eq!(doc.undo().as_deref(), Some("Add Room Reverb"));
        assert!(track(&doc, audio).devices.is_empty());
        doc.redo();
        assert_eq!(names(&doc, audio), ["Room Reverb"]);
    }

    #[test]
    fn effects_land_after_the_selected_device_or_where_dropped() {
        let (mut doc, _, audio) = doc();
        let a = doc
            .add_device(audio, plugin("A", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.add_device(audio, plugin("C", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.add_device(audio, plugin("B", DeviceKind::Effect), Slot::After(a))
            .unwrap();
        assert_eq!(names(&doc, audio), ["A", "B", "C"]);
        doc.add_device(audio, plugin("D", DeviceKind::Effect), Slot::At(0))
            .unwrap();
        doc.add_device(audio, plugin("E", DeviceKind::Effect), Slot::At(99))
            .unwrap();
        assert_eq!(names(&doc, audio), ["D", "A", "B", "C", "E"]);
    }

    #[test]
    fn an_instrument_replaces_the_synth_in_one_step_and_undo_brings_it_back() {
        let (mut doc, midi, _) = doc();
        assert_eq!(names(&doc, midi), ["Synth"]);
        let id = doc
            .add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        assert_eq!(names(&doc, midi), ["Glass Pad"]);
        assert_eq!(track(&doc, midi).sound_source().map(|d| d.id), Some(id));
        assert!(track(&doc, midi).instrument().is_none(), "no synth left");
        assert_eq!(
            doc.undo().as_deref(),
            Some("Replace instrument with Glass Pad")
        );
        assert_eq!(names(&doc, midi), ["Synth"]);
        assert_eq!(
            track(&doc, midi).instrument().unwrap().preset(),
            Some("poly")
        );
    }

    #[test]
    fn an_instrument_goes_first_before_the_effects_and_switches_the_track_back_to_it() {
        let (mut doc, midi, _) = doc();
        doc.add_device(midi, plugin("Room Reverb", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.set_midi_to(
            &[midi],
            Some(lumen_project::model::MidiTo {
                port: "Out".into(),
                channel: 0,
            }),
        );
        doc.add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        assert_eq!(names(&doc, midi), ["Glass Pad", "Room Reverb"]);
        assert!(track(&doc, midi).midi_to.is_none());
    }

    #[test]
    fn refusals_say_why_and_change_nothing() {
        let (mut doc, midi, audio) = doc();
        let before = doc.project().clone();
        let effect = plugin("Room Reverb", DeviceKind::Effect);
        let instrument = plugin("Glass Pad", DeviceKind::Instrument);
        assert_eq!(
            doc.add_device(audio, instrument.clone(), Slot::End),
            Err(Refusal::InstrumentNeedsMidi)
        );
        assert_eq!(
            doc.add_device(midi, effect.clone(), Slot::At(0)),
            Err(Refusal::BeforeInstrument)
        );
        assert_eq!(
            doc.add_device(midi, effect.clone(), Slot::After(GONE)),
            Err(Refusal::NoDevice)
        );
        assert_eq!(
            doc.add_device(GONE, effect.clone(), Slot::End),
            Err(Refusal::NoTrack)
        );
        let master = doc.project().master.id;
        assert_eq!(
            doc.add_device(master, effect, Slot::End),
            Err(Refusal::Unsupported)
        );
        let bad = PluginRef::Vst3 {
            class_id: "nope".into(),
            name: "X".into(),
            vendor: String::new(),
            kind: DeviceKind::Effect,
        };
        assert_eq!(
            doc.add_device(audio, bad, Slot::End),
            Err(Refusal::BadPlugin)
        );
        assert_eq!(*doc.project(), before, "nothing changed");
        assert_eq!(
            doc.undo().as_deref(),
            Some("Add audio track"),
            "and nothing was recorded: the last step is still the track"
        );
        for refusal in [
            Refusal::NoTrack,
            Refusal::Unsupported,
            Refusal::InstrumentNeedsMidi,
            Refusal::BeforeInstrument,
            Refusal::NoDevice,
            Refusal::BadPlugin,
        ] {
            assert!(!refusal.to_string().is_empty());
        }
    }

    #[test]
    fn bypass_and_remove_are_one_undo_step_each() {
        let (mut doc, _, audio) = doc();
        let id = doc
            .add_device(audio, plugin("Room Reverb", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.set_device_bypass(audio, id, true).unwrap();
        assert!(track(&doc, audio).devices[0].bypass);
        doc.set_device_bypass(audio, id, true).unwrap();
        assert_eq!(doc.undo().as_deref(), Some("Bypass Room Reverb"));
        assert!(!track(&doc, audio).devices[0].bypass, "no second step");
        doc.redo();
        doc.set_device_bypass(audio, id, false).unwrap();
        assert_eq!(doc.undo().as_deref(), Some("Enable Room Reverb"));
        doc.remove_device(audio, id).unwrap();
        assert!(track(&doc, audio).devices.is_empty());
        assert_eq!(doc.undo().as_deref(), Some("Remove Room Reverb"));
        assert_eq!(names(&doc, audio), ["Room Reverb"]);
        assert_eq!(doc.remove_device(audio, GONE), Err(Refusal::NoDevice));
        assert_eq!(doc.set_device_bypass(GONE, id, true), Err(Refusal::NoTrack));
    }

    #[test]
    fn choosing_the_built_in_synth_replaces_a_plugin_instrument() {
        let (mut doc, midi, _) = doc();
        doc.add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        doc.add_device(midi, plugin("Room Reverb", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.set_instrument(&[midi], Some(lumen_devices::Preset::Pad));
        assert_eq!(names(&doc, midi), ["Synth", "Room Reverb"]);
        assert_eq!(
            track(&doc, midi).instrument().unwrap().preset(),
            Some("pad")
        );
        let instruments = track(&doc, midi)
            .devices
            .iter()
            .filter(|d| d.is_instrument())
            .count();
        assert_eq!(instruments, 1, "one instrument a track");
    }

    #[test]
    fn every_edit_leaves_a_project_that_validates() {
        let (mut doc, midi, audio) = doc();
        doc.add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        doc.add_device(midi, plugin("Room Reverb", DeviceKind::Effect), Slot::End)
            .unwrap();
        doc.add_device(audio, plugin("Tape Delay", DeviceKind::Effect), Slot::End)
            .unwrap();
        let bytes = lumen_project::to_bytes(doc.project()).expect("the project can be saved");
        assert_eq!(lumen_project::from_bytes(&bytes).unwrap(), *doc.project());
    }

    #[test]
    fn a_device_on_a_new_track_is_one_undo_step_and_the_synth_starts_on_a_preset() {
        let (mut doc, _, _) = doc();
        let before = doc.project().tracks.len();
        let steps = doc.history().steps().0.len();
        let (track, device) = doc
            .add_device_on_new_track(plugin("Verb", DeviceKind::Effect), None)
            .unwrap();
        assert_eq!(doc.project().tracks.len(), before + 1);
        assert_eq!(track_of(&doc, track).kind, TrackKind::Audio);
        assert_eq!(track_of(&doc, track).devices[0].id, device);
        assert_eq!(doc.history().steps().0.len(), steps + 1);
        doc.undo();
        assert_eq!(doc.project().tracks.len(), before);

        let synth = PluginRef::Builtin { name: SYNTH.into() };
        let (track, _) = doc.add_device_on_new_track(synth, None).unwrap();
        let made = track_of(&doc, track);
        assert_eq!(made.kind, TrackKind::Midi);
        assert_eq!(made.devices.len(), 1);
        assert!(made.devices[0].preset().is_some());
    }

    fn track_of(doc: &Document, id: Id) -> &Track {
        track(doc, id)
    }
}
