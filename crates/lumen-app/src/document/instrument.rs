//! What plays a MIDI track (M5 slice 1, format v12): its built-in
//! instrument and its MIDI output. Each is one undo step. A track plays its
//! instrument or a MIDI output port, one or the other (the owner's T1):
//! choosing a port keeps the instrument on the track for switching back,
//! and choosing an instrument switches the track back to it.

use lumen_devices::Preset;
use lumen_project::model::{Device, Id, MidiTo, TrackKind};

use super::Document;

/// The sound new MIDI tracks get (the owner's S1).
pub const DEFAULT_PRESET: Preset = Preset::Poly;

impl Document {
    /// Gives MIDI tracks among `ids` the synth on `preset`, or takes the
    /// instrument away (`None`). A track that was playing to a MIDI output
    /// plays its instrument again.
    pub fn set_instrument(&mut self, ids: &[Id], preset: Option<Preset>) {
        let tracks: Vec<(Id, Id)> = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && ids.contains(&t.id))
            .map(|t| t.id)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| (t, self.fresh_id()))
            .collect();
        if tracks.is_empty() {
            return;
        }
        let label = match preset {
            Some(p) => format!("Instrument: {}", p.name()),
            None => "Remove instrument".to_owned(),
        };
        self.edit_tracks(&label, ids, |track| {
            let Some((_, device_id)) = tracks.iter().find(|(t, _)| *t == track.id) else {
                return;
            };
            match preset {
                Some(p) => {
                    track.midi_to = None;
                    match track.devices.iter_mut().find(|d| d.is_synth()) {
                        Some(synth) => synth.state = p.id().as_bytes().to_vec(),
                        None => track.devices.insert(0, Device::synth(*device_id, p.id())),
                    }
                }
                None => track.devices.retain(|d| !d.is_synth()),
            }
        });
    }

    /// Sends the notes of MIDI tracks among `ids` to a MIDI output
    /// (`Some`), or back to their instrument (`None`).
    pub fn set_midi_to(&mut self, ids: &[Id], to: Option<MidiTo>) {
        let label = match &to {
            Some(out) => format!("MIDI To: {} {}", out.port, out.channel + 1),
            None => "MIDI To: Instrument".to_owned(),
        };
        let midi: Vec<Id> = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && ids.contains(&t.id))
            .map(|t| t.id)
            .collect();
        if midi.is_empty()
            || to
                .as_ref()
                .is_some_and(|t| t.channel > 15 || t.port.is_empty())
        {
            return;
        }
        self.edit_tracks(&label, &midi, |track| track.midi_to = to.clone());
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::TrackKind;

    use super::*;

    fn doc() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let id = doc.add_track(TrackKind::Midi, None);
        (doc, id)
    }

    fn track(doc: &Document, id: Id) -> &lumen_project::model::Track {
        doc.project().tracks.iter().find(|t| t.id == id).unwrap()
    }

    fn to(port: &str, channel: u8) -> MidiTo {
        MidiTo {
            port: port.into(),
            channel,
        }
    }

    #[test]
    fn a_new_midi_track_gets_the_synth_on_poly() {
        let (doc, id) = doc();
        let synth = track(&doc, id).instrument().expect("an instrument");
        assert_eq!(synth.preset(), Some("poly"));
        assert!(track(&doc, id).midi_to.is_none());
        // Audio tracks have none.
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        assert!(track(&doc, audio).devices.is_empty());
    }

    #[test]
    fn choosing_a_preset_changes_it_in_place_and_undo_restores_it() {
        let (mut doc, id) = doc();
        let device = track(&doc, id).devices[0].id;
        doc.set_instrument(&[id], Some(Preset::Pad));
        let t = track(&doc, id);
        assert_eq!((t.devices.len(), t.devices[0].id), (1, device));
        assert_eq!(t.instrument().unwrap().preset(), Some("pad"));
        assert_eq!(doc.undo().as_deref(), Some("Instrument: Pad"));
        assert_eq!(track(&doc, id).instrument().unwrap().preset(), Some("poly"));
        doc.redo();
        assert_eq!(track(&doc, id).instrument().unwrap().preset(), Some("pad"));
    }

    #[test]
    fn removing_the_instrument_and_choosing_one_again_is_one_step_each() {
        let (mut doc, id) = doc();
        doc.set_instrument(&[id], None);
        assert!(track(&doc, id).instrument().is_none());
        doc.set_instrument(&[id], Some(Preset::Bass));
        let t = track(&doc, id);
        assert_eq!(t.instrument().unwrap().preset(), Some("bass"));
        // The new device has an id of its own.
        let ids: std::collections::HashSet<_> = lumen_project::routing::all_tracks(doc.project())
            .flat_map(|t| t.devices.iter().map(|d| d.id))
            .collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(doc.undo().as_deref(), Some("Instrument: Bass"));
        assert_eq!(doc.undo().as_deref(), Some("Remove instrument"));
    }

    #[test]
    fn a_midi_output_keeps_the_instrument_and_choosing_an_instrument_returns_to_it() {
        let (mut doc, id) = doc();
        doc.set_midi_to(&[id], Some(to("Microsoft GS Wavetable Synth", 9)));
        let t = track(&doc, id);
        assert_eq!(t.midi_to.as_ref().unwrap().channel, 9);
        assert!(t.instrument().is_some(), "kept for switching back");
        doc.set_instrument(&[id], Some(Preset::Keys));
        assert!(track(&doc, id).midi_to.is_none());
        doc.set_midi_to(&[id], Some(to("Port", 0)));
        doc.set_midi_to(&[id], None);
        assert!(track(&doc, id).midi_to.is_none());
        assert_eq!(doc.undo().as_deref(), Some("MIDI To: Instrument"));
        assert_eq!(doc.undo().as_deref(), Some("MIDI To: Port 1"));
    }

    #[test]
    fn only_midi_tracks_take_instruments_and_outputs_and_bad_ones_change_nothing() {
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        let midi = doc.add_track(TrackKind::Midi, None);
        let state = doc.state();
        doc.set_instrument(&[audio], Some(Preset::Pad));
        doc.set_midi_to(&[audio], Some(to("Port", 0)));
        doc.set_midi_to(&[midi], Some(to("Port", 16)));
        doc.set_midi_to(&[midi], Some(to("", 0)));
        assert_eq!(doc.state(), state);
    }

    #[test]
    fn duplicating_a_midi_track_gives_the_copy_its_own_device_id() {
        let (mut doc, id) = doc();
        let copies = doc.duplicate_tracks(&[id]);
        assert_eq!(copies.len(), 1);
        let a = track(&doc, id).devices[0].id;
        let b = track(&doc, copies[0]).devices[0].id;
        assert_ne!(a, b);
        assert!(lumen_project::to_bytes(doc.project()).is_ok());
    }
}
