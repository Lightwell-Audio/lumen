//! A track's device chain (M6 slice 2, format v17): adding a plugin, taking
//! one away and bypassing one. Each is one undo step, and every refusal says
//! why, so the interface can show the reason. A MIDI track has at most one
//! instrument, always first; an effect goes anywhere after it.

use std::fmt;

use std::sync::Arc;

use lumen_project::model::{Device, DeviceKind, Id, PluginRef, SYNTH, Track, TrackKind};

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
    /// A track plays one instrument, so one can't be copied onto its own track.
    OneInstrument,
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
            Refusal::OneInstrument => "A track plays one instrument.",
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

/// Where in `track`'s chain a device of `kind` lands for `slot`, counted in
/// the chain as it stands (a drop gap is a position between today's cards).
/// An instrument goes first and needs a MIDI track; an effect never goes
/// before the instrument.
fn place_in(track: &Track, kind: DeviceKind, slot: Slot) -> Result<usize, Refusal> {
    match kind {
        DeviceKind::Instrument => {
            if track.kind != TrackKind::Midi {
                return Err(Refusal::InstrumentNeedsMidi);
            }
            Ok(0)
        }
        DeviceKind::Effect => {
            let len = track.devices.len();
            let wanted = match slot {
                Slot::End => len,
                Slot::After(device) => {
                    track
                        .devices
                        .iter()
                        .position(|d| d.id == device)
                        .ok_or(Refusal::NoDevice)?
                        + 1
                }
                Slot::At(at) => at.min(len),
            };
            let floor = track
                .devices
                .iter()
                .position(|d| d.is_instrument())
                .map_or(0, |at| at + 1);
            if wanted < floor {
                return Err(Refusal::BeforeInstrument);
            }
            Ok(wanted)
        }
    }
}

/// A checked move or copy (see `Document::transfer`).
struct Transfer {
    device: Device,
    /// Where the device sits on its own track.
    from_at: usize,
    /// The gap it goes to on the target, in the target's chain as it stands.
    index: usize,
    to_name: String,
}

/// Takes `device` off `track` and hands it back.
fn take_device(project: &mut lumen_project::model::Project, track: Id, device: Id) -> Device {
    let track = project
        .tracks
        .iter_mut()
        .find(|t| t.id == track)
        .expect("the move was checked against this project");
    let track = Arc::make_mut(track);
    let at = track
        .devices
        .iter()
        .position(|d| d.id == device)
        .expect("the move was checked against this project");
    track.devices.remove(at)
}

/// Puts `device` on `track` at `index`. An instrument first drives out the
/// one there and returns a MIDI-output track to it.
fn put_device(
    project: &mut lumen_project::model::Project,
    track: Id,
    device: Device,
    index: usize,
    instrument: bool,
) {
    let track = project
        .tracks
        .iter_mut()
        .find(|t| t.id == track)
        .expect("the edit was checked against this project");
    let track = Arc::make_mut(track);
    if instrument {
        track.devices.retain(|d| !d.is_instrument());
        track.midi_to = None;
    }
    track.devices.insert(index.min(track.devices.len()), device);
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
        let index = place_in(current, kind, slot)?;
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

    /// Moves `device` to `slot` on `to`, which may be its own track (a
    /// reorder) or another (the device and its host go with it, state and
    /// bypass kept). `slot` counts the chain as it stands, so a drop gap
    /// after the device itself, or just before it, changes nothing and
    /// records nothing. An instrument takes the first place and replaces the
    /// one on a MIDI track it moves to (D4); an effect is never put before
    /// the instrument. One undo step.
    pub fn move_device(&mut self, from: Id, device: Id, to: Id, slot: Slot) -> Result<(), Refusal> {
        let plan = self.transfer(from, device, to, slot)?;
        let instrument = plan.device.is_instrument();
        if from == to && instrument {
            // Already first; nothing to move, and the MIDI output stays.
            return Ok(());
        }
        let label = if from == to {
            format!("Move {}", name_of(&plan.device))
        } else {
            format!("Move {} to {}", name_of(&plan.device), plan.to_name)
        };
        // The gap was counted with the device still in the chain.
        let index = if from == to && plan.from_at < plan.index {
            plan.index - 1
        } else {
            plan.index
        };
        self.history.apply(&label, None, |project| {
            let moved = take_device(project, from, device);
            put_device(project, to, moved, index, instrument);
        });
        Ok(())
    }

    /// Copies `device` to `slot` on `to` (its own track too) as a new device:
    /// a fresh id, so its host and state are its own, with the source's saved
    /// state, bypass and reported latency. Returns the copy's id. A track
    /// plays one instrument, so an instrument copied to another MIDI track
    /// replaces the one there (D4) and cannot be copied onto its own track.
    /// One undo step.
    pub fn copy_device(&mut self, from: Id, device: Id, to: Id, slot: Slot) -> Result<Id, Refusal> {
        let plan = self.transfer(from, device, to, slot)?;
        if from == to && plan.device.is_instrument() {
            return Err(Refusal::OneInstrument);
        }
        let id = self.fresh_id();
        let mut copy = plan.device;
        let name = name_of(&copy);
        copy.id = id;
        let label = if from == to {
            format!("Copy {name}")
        } else {
            format!("Copy {name} to {}", plan.to_name)
        };
        let instrument = copy.is_instrument();
        self.history.apply(&label, None, |project| {
            put_device(project, to, copy, plan.index, instrument);
        });
        Ok(id)
    }

    /// Puts a copy of `device`, kept earlier by a Copy, at `slot` on `to`: a
    /// new device with a fresh id and the saved settings. The source may be
    /// gone or changed since. An instrument replaces the one on the track
    /// (D4). Returns the copy's id. One undo step.
    pub fn paste_device(&mut self, device: &Device, to: Id, slot: Slot) -> Result<Id, Refusal> {
        if self.project().master.id == to {
            return Err(Refusal::Unsupported);
        }
        let target = self
            .project()
            .tracks
            .iter()
            .find(|t| t.id == to)
            .ok_or(Refusal::NoTrack)?;
        let index = place_in(target, device.kind(), slot)?;
        let id = self.fresh_id();
        let mut copy = device.clone();
        copy.id = id;
        let label = format!("Paste {}", name_of(&copy));
        let instrument = copy.is_instrument();
        self.history.apply(&label, None, |project| {
            put_device(project, to, copy, index, instrument);
        });
        Ok(id)
    }

    /// What a move or a copy needs, checked: the device, where it lands and
    /// the target's name.
    fn transfer(&self, from: Id, device: Id, to: Id, slot: Slot) -> Result<Transfer, Refusal> {
        let master = self.project().master.id;
        if from == master || to == master {
            return Err(Refusal::Unsupported);
        }
        let tracks = &self.project().tracks;
        let source = tracks
            .iter()
            .find(|t| t.id == from)
            .ok_or(Refusal::NoTrack)?;
        let target = tracks.iter().find(|t| t.id == to).ok_or(Refusal::NoTrack)?;
        let from_at = source
            .devices
            .iter()
            .position(|d| d.id == device)
            .ok_or(Refusal::NoDevice)?;
        let device = source.devices[from_at].clone();
        let index = place_in(target, device.kind(), slot)?;
        Ok(Transfer {
            device,
            from_at,
            index,
            to_name: target.name.clone(),
        })
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
            Refusal::OneInstrument,
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

    fn effect(doc: &mut Document, on: Id, name: &str) -> Id {
        doc.add_device(on, plugin(name, DeviceKind::Effect), Slot::End)
            .unwrap()
    }

    /// Gives a device saved state and bypass, as a plugin that was set up has.
    fn set_up(doc: &mut Document, on: Id, device: Id, state: &[u8]) {
        doc.apply("Set up", None, |p| {
            let t = p.tracks.iter_mut().find(|t| t.id == on).unwrap();
            let d = Arc::make_mut(t)
                .devices
                .iter_mut()
                .find(|d| d.id == device)
                .unwrap();
            d.state = state.to_vec();
            d.controller_state = vec![9, 9];
            d.bypass = true;
        });
    }

    fn steps(doc: &Document) -> usize {
        doc.history().steps().0.len()
    }

    fn device_ids(doc: &Document) -> Vec<Id> {
        let mut ids: Vec<Id> = doc
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.devices.iter().map(|d| d.id))
            .collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    #[test]
    fn a_device_moves_within_its_chain_as_one_step_and_a_gap_beside_it_changes_nothing() {
        let (mut doc, _, audio) = doc();
        let a = effect(&mut doc, audio, "A");
        let b = effect(&mut doc, audio, "B");
        let c = effect(&mut doc, audio, "C");
        doc.move_device(audio, c, audio, Slot::At(0)).unwrap();
        assert_eq!(names(&doc, audio), ["C", "A", "B"]);
        assert_eq!(doc.undo().as_deref(), Some("Move C"));
        assert_eq!(names(&doc, audio), ["A", "B", "C"]);
        // Gaps count the chain as it stands: the gap before C is index 2.
        doc.move_device(audio, a, audio, Slot::At(2)).unwrap();
        assert_eq!(names(&doc, audio), ["B", "A", "C"]);
        doc.move_device(audio, a, audio, Slot::After(c)).unwrap();
        assert_eq!(names(&doc, audio), ["B", "C", "A"]);
        doc.move_device(audio, b, audio, Slot::End).unwrap();
        assert_eq!(names(&doc, audio), ["C", "A", "B"]);

        // B is last: the gaps before and after it leave it where it is.
        let before = steps(&doc);
        for slot in [Slot::At(2), Slot::At(3), Slot::After(a), Slot::After(b)] {
            doc.move_device(audio, b, audio, slot).unwrap();
        }
        assert_eq!(names(&doc, audio), ["C", "A", "B"]);
        assert_eq!(
            steps(&doc),
            before,
            "dropping a card where it is records nothing"
        );
        assert_eq!(
            track(&doc, audio).devices.len(),
            3,
            "nothing was made or lost"
        );
    }

    #[test]
    fn an_effect_moves_to_another_track_with_its_state_and_undo_puts_both_chains_back() {
        let (mut doc, midi, audio) = doc();
        let a = effect(&mut doc, audio, "A");
        let b = effect(&mut doc, audio, "B");
        set_up(&mut doc, audio, a, &[1, 2, 3]);
        let moved = track(&doc, audio).devices[0].clone();
        let before = doc.project().clone();
        let steps_before = steps(&doc);
        doc.move_device(audio, a, midi, Slot::End).unwrap();
        assert_eq!(names(&doc, audio), ["B"]);
        assert_eq!(names(&doc, midi), ["Synth", "A"]);
        assert_eq!(
            track(&doc, midi).devices[1],
            moved,
            "the same device, id and state"
        );
        assert_eq!(steps(&doc), steps_before + 1);
        let label = format!("Move A to {}", track(&doc, midi).name);
        assert_eq!(doc.undo().as_deref(), Some(label.as_str()));
        assert_eq!(*doc.project(), before);
        assert_eq!(track(&doc, audio).devices[1].id, b);
    }

    #[test]
    fn an_instrument_moves_to_another_midi_track_replacing_its_instrument() {
        let (mut doc, midi, _) = doc();
        let other = doc.add_track(TrackKind::Midi, None);
        let pad = doc
            .add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        effect(&mut doc, other, "Verb");
        doc.set_midi_to(
            &[other],
            Some(lumen_project::model::MidiTo {
                port: "Out".into(),
                channel: 0,
            }),
        );
        let before = doc.project().clone();
        doc.move_device(midi, pad, other, Slot::End).unwrap();
        assert!(
            names(&doc, midi).is_empty(),
            "the source is left without one"
        );
        assert_eq!(
            names(&doc, other),
            ["Glass Pad", "Verb"],
            "first, over the synth"
        );
        assert!(track(&doc, other).midi_to.is_none());
        assert_eq!(
            doc.undo().as_deref(),
            Some(format!("Move Glass Pad to {}", track(&doc, other).name).as_str())
        );
        assert_eq!(*doc.project(), before, "the synth and the output come back");
    }

    #[test]
    fn an_instrument_stays_first_on_its_own_track() {
        let (mut doc, midi, audio) = doc();
        let synth = track(&doc, midi).devices[0].id;
        effect(&mut doc, midi, "Verb");
        let before = steps(&doc);
        doc.move_device(midi, synth, midi, Slot::End).unwrap();
        assert_eq!(names(&doc, midi), ["Synth", "Verb"]);
        assert_eq!(steps(&doc), before);
        assert_eq!(
            doc.move_device(midi, synth, audio, Slot::End),
            Err(Refusal::InstrumentNeedsMidi)
        );
    }

    #[test]
    fn moves_and_copies_that_cannot_happen_say_why_and_change_nothing() {
        let (mut doc, midi, audio) = doc();
        let verb = effect(&mut doc, audio, "Verb");
        let synth = track(&doc, midi).devices[0].id;
        let master = doc.project().master.id;
        let before = doc.project().clone();
        let steps_before = steps(&doc);
        assert_eq!(
            doc.move_device(audio, verb, midi, Slot::At(0)),
            Err(Refusal::BeforeInstrument)
        );
        assert_eq!(
            doc.copy_device(audio, verb, midi, Slot::At(0)),
            Err(Refusal::BeforeInstrument)
        );
        assert_eq!(
            doc.move_device(audio, GONE, midi, Slot::End),
            Err(Refusal::NoDevice)
        );
        assert_eq!(
            doc.copy_device(audio, verb, midi, Slot::After(GONE)),
            Err(Refusal::NoDevice),
            "a gap after a device that is not there"
        );
        assert_eq!(
            doc.move_device(GONE, verb, midi, Slot::End),
            Err(Refusal::NoTrack)
        );
        assert_eq!(
            doc.copy_device(audio, verb, GONE, Slot::End),
            Err(Refusal::NoTrack)
        );
        assert_eq!(
            doc.move_device(audio, verb, master, Slot::End),
            Err(Refusal::Unsupported)
        );
        assert_eq!(
            doc.copy_device(master, verb, audio, Slot::End),
            Err(Refusal::Unsupported)
        );
        assert_eq!(
            doc.copy_device(midi, synth, midi, Slot::End),
            Err(Refusal::OneInstrument)
        );
        assert_eq!(
            doc.copy_device(midi, synth, audio, Slot::End),
            Err(Refusal::InstrumentNeedsMidi)
        );
        assert_eq!(*doc.project(), before);
        assert_eq!(
            steps(&doc),
            steps_before,
            "no step, and no id spent on a refusal"
        );
        assert!(!Refusal::OneInstrument.to_string().is_empty());
        let next = doc.fresh_id();
        let copy = doc.copy_device(audio, verb, audio, Slot::End).unwrap();
        assert_eq!(copy.0, next.0 + 1, "a refusal did not take an id");
    }

    #[test]
    fn a_copy_is_a_new_device_with_the_same_settings_and_the_original_stays() {
        let (mut doc, _, audio) = doc();
        let other = doc.add_track(TrackKind::Audio, None);
        let verb = effect(&mut doc, audio, "Verb");
        effect(&mut doc, audio, "Delay");
        set_up(&mut doc, audio, verb, &[7, 8, 9]);
        let original = track(&doc, audio).devices[0].clone();
        let before = doc.project().clone();
        let copy = doc.copy_device(audio, verb, other, Slot::End).unwrap();
        assert_ne!(copy, verb);
        assert_eq!(
            names(&doc, audio),
            ["Verb", "Delay"],
            "the original did not move"
        );
        assert_eq!(track(&doc, audio).devices[0], original);
        let mut expected = original.clone();
        expected.id = copy;
        assert_eq!(
            track(&doc, other).devices,
            [expected],
            "state and bypass came too"
        );
        let ids = device_ids(&doc);
        assert!(ids.windows(2).all(|w| w[0] != w[1]), "every id is its own");
        // Changing the copy leaves the original alone.
        doc.set_device_bypass(other, copy, false).unwrap();
        assert!(track(&doc, audio).devices[0].bypass);

        let label = format!("Copy Verb to {}", track(&doc, other).name);
        doc.undo();
        assert_eq!(doc.undo().as_deref(), Some(label.as_str()));
        assert_eq!(*doc.project(), before);
    }

    #[test]
    fn a_copy_lands_beside_its_original_and_an_instrument_copy_replaces_the_target_instrument() {
        let (mut doc, midi, audio) = doc();
        let other = doc.add_track(TrackKind::Midi, None);
        let a = effect(&mut doc, audio, "A");
        effect(&mut doc, audio, "B");
        let copy = doc.copy_device(audio, a, audio, Slot::After(a)).unwrap();
        assert_eq!(names(&doc, audio), ["A", "A", "B"]);
        assert_eq!(track(&doc, audio).devices[1].id, copy);
        assert_eq!(doc.undo().as_deref(), Some("Copy A"));

        let pad = doc
            .add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        let before = doc.project().clone();
        let copy = doc.copy_device(midi, pad, other, Slot::End).unwrap();
        assert_eq!(names(&doc, midi), ["Glass Pad"], "the original stays");
        assert_eq!(names(&doc, other), ["Glass Pad"], "over the synth");
        assert_eq!(track(&doc, other).devices[0].id, copy);
        doc.undo();
        assert_eq!(*doc.project(), before);
    }

    #[test]
    fn a_pasted_device_is_new_keeps_its_settings_and_outlives_its_source() {
        let (mut doc, midi, audio) = doc();
        let a = effect(&mut doc, audio, "A");
        set_up(&mut doc, audio, a, &[4, 5]);
        let kept = track(&doc, audio).devices[0].clone();
        doc.remove_device(audio, a).unwrap();
        // The source is gone; the paste still has what was copied.
        let id = doc.paste_device(&kept, midi, Slot::End).unwrap();
        assert_ne!(id, a);
        let pasted = track(&doc, midi).devices[1].clone();
        let mut expected = kept.clone();
        expected.id = id;
        assert_eq!(pasted, expected);
        assert_eq!(doc.undo().as_deref(), Some("Paste A"));
        // The same rules as an add: not before the instrument, none on the
        // master, an instrument pasted on an audio track is refused.
        assert_eq!(
            doc.paste_device(&kept, midi, Slot::At(0)),
            Err(Refusal::BeforeInstrument)
        );
        let master = doc.project().master.id;
        assert_eq!(
            doc.paste_device(&kept, master, Slot::End),
            Err(Refusal::Unsupported)
        );
        let synth = track(&doc, midi).devices[0].clone();
        assert_eq!(
            doc.paste_device(&synth, audio, Slot::End),
            Err(Refusal::InstrumentNeedsMidi)
        );
        assert_eq!(
            doc.paste_device(&kept, GONE, Slot::End),
            Err(Refusal::NoTrack)
        );
    }

    #[test]
    fn a_project_after_moves_and_copies_saves_and_reopens() {
        let (mut doc, midi, audio) = doc();
        let a = effect(&mut doc, audio, "A");
        let pad = doc
            .add_device(midi, plugin("Glass Pad", DeviceKind::Instrument), Slot::End)
            .unwrap();
        doc.copy_device(audio, a, midi, Slot::End).unwrap();
        doc.move_device(audio, a, midi, Slot::After(pad)).unwrap();
        let bytes = lumen_project::to_bytes(doc.project()).expect("the project can be saved");
        assert_eq!(lumen_project::from_bytes(&bytes).unwrap(), *doc.project());
    }
}
