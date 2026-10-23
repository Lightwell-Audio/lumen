//! Output tracks (M6 slice 6d, format v21): a plugin instrument with several
//! audio outputs gets an audio track for each output that is turned on. The
//! track is made for the output (named after it, in the instrument track's
//! colour and group, right under it) and plays what comes out of it; the
//! instrument's own track keeps the first output. Turning outputs on or off
//! is one undo step, however many; an output that is turned off takes its
//! track with it unless the track holds clips or devices, when the choice
//! between removing it and keeping it is the caller's.

use std::sync::Arc;

use lumen_project::model::{Id, PluginOutput, Track, TrackKind};

use super::Document;
use super::devices::{Refusal, find_device};

/// What to do with the track of an output that is turned off while it holds
/// clips or devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// The track goes, and its clips with it (undo brings them back).
    Remove,
    /// The track stays, with no input: it is an audio track like any other.
    Keep,
}

/// A change of the outputs of one instrument.
#[derive(Debug, Clone, Copy)]
pub struct OutputEdit<'a> {
    /// Outputs to turn on: the bus, and the name of the track made for it. One
    /// that is already on is left as it is.
    pub on: &'a [(u32, String)],
    /// Outputs to turn off.
    pub off: &'a [u32],
    /// What to do with a track that holds clips or devices (`None`: refuse and
    /// say which, so the person can be asked).
    pub leave: Option<Leave>,
}

/// Whether an output track holds nothing of anyone's work.
pub fn is_unused(track: &Track) -> bool {
    track.clips.is_empty() && track.devices.is_empty() && track.sends.is_empty()
}

impl Document {
    /// The tracks that play outputs of `device`: its output bus and the track,
    /// in bus order.
    pub fn output_tracks(&self, device: Id) -> Vec<(u32, Id)> {
        let mut found: Vec<(u32, Id)> = self
            .project()
            .tracks
            .iter()
            .filter_map(|t| {
                t.plugin_output
                    .filter(|o| o.device == device)
                    .map(|o| (o.bus, t.id))
            })
            .collect();
        found.sort_unstable();
        found
    }

    /// Which of the outputs in `off` have a track that holds clips or devices.
    pub fn outputs_in_use(&self, device: Id, off: &[u32]) -> Vec<u32> {
        let tracks = &self.project().tracks;
        self.output_tracks(device)
            .into_iter()
            .filter(|(bus, _)| off.contains(bus))
            .filter(|(_, id)| {
                tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .is_some_and(|t| !is_unused(t))
            })
            .map(|(bus, _)| bus)
            .collect()
    }

    /// Turns outputs of the plugin instrument `device` on and off, as one
    /// undo step. Returns the tracks made. Refused when `device` is not a
    /// plugin instrument, or (with no `leave`) when an output to turn off has
    /// a track with something in it.
    pub fn set_outputs(&mut self, device: Id, edit: OutputEdit<'_>) -> Result<Vec<Id>, Refusal> {
        let (owner, held) = find_device(self.project(), device).ok_or(Refusal::NoDevice)?;
        if !held.is_vst3() || !held.is_instrument() {
            return Err(Refusal::NotAnInstrument);
        }
        let existing = self.output_tracks(device);
        let in_use = self.outputs_in_use(device, edit.off);
        if !in_use.is_empty() && edit.leave.is_none() {
            return Err(Refusal::OutputHasContent);
        }
        let make: Vec<(u32, String)> = edit
            .on
            .iter()
            .filter(|(bus, _)| !existing.iter().any(|(b, _)| b == bus))
            .cloned()
            .collect();
        let drop: Vec<(u32, Id)> = existing
            .iter()
            .filter(|(bus, _)| edit.off.contains(bus))
            .copied()
            .collect();
        if make.is_empty() && drop.is_empty() {
            return Ok(Vec::new());
        }
        let label = label_of(&make, &drop, edit.leave, self);
        let made: Vec<(Id, u32, String)> = make
            .into_iter()
            .map(|(bus, name)| (self.fresh_id(), bus, name))
            .collect();
        let created: Vec<Id> = made.iter().map(|(id, _, _)| *id).collect();
        let leave = edit.leave;
        self.history.apply(&label, None, |project| {
            // What goes: unused tracks always, the rest as asked.
            let mut gone = Vec::new();
            for &(_, id) in &drop {
                let Some(track) = project.tracks.iter_mut().find(|t| t.id == id) else {
                    continue;
                };
                if is_unused(track) || leave == Some(Leave::Remove) {
                    gone.push(id);
                } else {
                    Arc::make_mut(track).plugin_output = None;
                }
            }
            project.tracks.retain(|t| !gone.contains(&t.id));
            super::mixer::forget_routes_to(project, &gone);

            let Some(at) = project.tracks.iter().position(|t| t.id == owner) else {
                return;
            };
            let (color, parent) = (project.tracks[at].color, project.tracks[at].parent);
            for (id, bus, name) in made {
                // Right under the instrument's track, among its other output
                // tracks in bus order.
                let mut index = at + 1;
                while project.tracks.get(index).is_some_and(|t| {
                    t.plugin_output
                        .is_some_and(|o| o.device == device && o.bus < bus)
                }) {
                    index += 1;
                }
                project.tracks.insert(
                    index,
                    Arc::new(Track {
                        parent,
                        plugin_output: Some(PluginOutput { device, bus }),
                        ..Track::new(id, name, TrackKind::Audio, color)
                    }),
                );
            }
            lumen_project::groups::normalize(&mut project.tracks);
        });
        Ok(created)
    }
}

/// The undo label of a change: what was turned on or off, in words.
fn label_of(
    make: &[(u32, String)],
    drop: &[(u32, Id)],
    leave: Option<Leave>,
    doc: &Document,
) -> String {
    let name_of = |id: Id| {
        doc.project()
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(String::new, |t| t.name.clone())
    };
    match (make, drop) {
        ([(_, name)], []) => format!("Add output track {name}"),
        (more, []) => format!("Add {} output tracks", more.len()),
        ([], [(_, id)]) => match leave {
            Some(Leave::Keep) => format!("Detach output track {}", name_of(*id)),
            _ => format!("Remove output track {}", name_of(*id)),
        },
        ([], more) => format!("Remove {} output tracks", more.len()),
        _ => "Change outputs".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{DeviceKind, PluginRef};

    use super::super::Slot;
    use super::*;

    fn plugin(name: &str, kind: DeviceKind) -> PluginRef {
        PluginRef::Vst3 {
            class_id: "ABCDEF0123456789ABCDEF0123456789".into(),
            name: name.into(),
            vendor: "Vendor A".into(),
            kind,
        }
    }

    /// A document with a MIDI track holding a plugin instrument, and an audio
    /// track after it.
    fn doc() -> (Document, Id, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let midi = doc.add_track(TrackKind::Midi, None);
        let audio = doc.add_track(TrackKind::Audio, None);
        let device = doc
            .add_device(midi, plugin("Sampler A", DeviceKind::Instrument), Slot::End)
            .unwrap();
        (doc, midi, audio, device)
    }

    fn on(buses: &[(u32, &str)]) -> Vec<(u32, String)> {
        buses.iter().map(|&(b, n)| (b, n.to_owned())).collect()
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.project()
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    #[test]
    fn outputs_on_make_tracks_right_under_the_instrument_in_bus_order_in_one_step() {
        let (mut doc, midi, _, device) = doc();
        let made = doc
            .set_outputs(
                device,
                OutputEdit {
                    on: &on(&[(3, "Sampler A · Hats"), (1, "Sampler A · Kick")]),
                    off: &[],
                    leave: None,
                },
            )
            .unwrap();
        assert_eq!(made.len(), 2);
        assert_eq!(
            names(&doc),
            ["MIDI 1", "Sampler A · Kick", "Sampler A · Hats", "Audio 1"],
            "kick (1) before hats (3), under the instrument"
        );
        let track = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == made[1])
            .unwrap();
        assert_eq!(track.plugin_output, Some(PluginOutput { device, bus: 1 }));
        assert_eq!(track.kind, TrackKind::Audio);
        assert_eq!(track.color, doc.project().tracks[0].color);
        assert_eq!(doc.output_tracks(device).len(), 2);
        assert_eq!(doc.undo().as_deref(), Some("Add 2 output tracks"));
        assert_eq!(names(&doc), ["MIDI 1", "Audio 1"]);
        let _ = midi;
    }

    #[test]
    fn a_later_output_goes_among_the_earlier_ones_and_an_output_already_on_is_left_alone() {
        let (mut doc, _, _, device) = doc();
        let edit = |buses: &[(u32, &str)], doc: &mut Document| {
            doc.set_outputs(
                device,
                OutputEdit {
                    on: &on(buses),
                    off: &[],
                    leave: None,
                },
            )
            .unwrap()
        };
        edit(&[(1, "K"), (3, "H")], &mut doc);
        let again = edit(&[(1, "K"), (2, "S")], &mut doc);
        assert_eq!(again.len(), 1, "only snare is new");
        assert_eq!(names(&doc), ["MIDI 1", "K", "S", "H", "Audio 1"]);
        assert_eq!(doc.undo().as_deref(), Some("Add output track S"));
    }

    #[test]
    fn turning_an_output_off_removes_its_empty_track_and_undo_brings_it_back() {
        let (mut doc, _, _, device) = doc();
        doc.set_outputs(
            device,
            OutputEdit {
                on: &on(&[(1, "K"), (2, "S")]),
                off: &[],
                leave: None,
            },
        )
        .unwrap();
        doc.set_outputs(
            device,
            OutputEdit {
                on: &[],
                off: &[1],
                leave: None,
            },
        )
        .unwrap();
        assert_eq!(names(&doc), ["MIDI 1", "S", "Audio 1"]);
        assert_eq!(doc.undo().as_deref(), Some("Remove output track K"));
        assert_eq!(names(&doc), ["MIDI 1", "K", "S", "Audio 1"]);
    }

    #[test]
    fn an_output_track_with_work_in_it_needs_an_answer_and_either_is_one_step() {
        use lumen_project::model::{Clip, ClipContent, Color};
        let (mut doc, _, _, device) = doc();
        let made = doc
            .set_outputs(
                device,
                OutputEdit {
                    on: &on(&[(1, "K")]),
                    off: &[],
                    leave: None,
                },
            )
            .unwrap();
        let track = made[0];
        let clip = doc.fresh_id();
        doc.apply("Put a clip there", None, |p| {
            let t = p.tracks.iter_mut().find(|t| t.id == track).unwrap();
            Arc::make_mut(t).clips.push(Arc::new(Clip {
                id: clip,
                name: "Take".into(),
                start: lumen_core::time::Ticks::ZERO,
                length: lumen_core::time::Ticks::quarters(4),
                muted: false,
                color: None::<Color>,
                locked: false,
                fade_in: Default::default(),
                fade_out: Default::default(),
                loop_length: None,
                loop_phase: Default::default(),
                envelopes: Default::default(),
                audio_phase_seconds: 0.0,
                content: ClipContent::Midi {
                    notes: Vec::new(),
                    events: Vec::new(),
                },
            }));
        });
        assert_eq!(doc.outputs_in_use(device, &[1, 2]), [1]);
        let off = OutputEdit {
            on: &[],
            off: &[1],
            leave: None,
        };
        assert_eq!(doc.set_outputs(device, off), Err(Refusal::OutputHasContent));
        assert_eq!(doc.output_tracks(device).len(), 1, "nothing changed");

        // Keep: the track stays, with no input.
        doc.set_outputs(
            device,
            OutputEdit {
                leave: Some(Leave::Keep),
                ..off
            },
        )
        .unwrap();
        assert!(doc.output_tracks(device).is_empty());
        assert!(
            doc.project()
                .tracks
                .iter()
                .any(|t| t.id == track && t.plugin_output.is_none())
        );
        assert_eq!(doc.undo().as_deref(), Some("Detach output track K"));
        // Remove: the track goes with its clip.
        doc.set_outputs(
            device,
            OutputEdit {
                leave: Some(Leave::Remove),
                ..off
            },
        )
        .unwrap();
        assert!(!doc.project().tracks.iter().any(|t| t.id == track));
        assert_eq!(doc.undo().as_deref(), Some("Remove output track K"));
        assert!(doc.project().tracks.iter().any(|t| t.id == track));
    }

    #[test]
    fn only_a_plugin_instrument_has_outputs() {
        let (mut doc, midi, audio, _) = doc();
        let effect = doc
            .add_device(audio, plugin("Comp", DeviceKind::Effect), Slot::End)
            .unwrap();
        let none = OutputEdit {
            on: &[],
            off: &[],
            leave: None,
        };
        assert_eq!(doc.set_outputs(effect, none), Err(Refusal::NotAnInstrument));
        assert_eq!(doc.set_outputs(Id(9_999_999), none), Err(Refusal::NoDevice));
        let synth = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == midi)
            .and_then(|t| t.devices.iter().find(|d| !d.is_vst3()))
            .map(|d| d.id);
        // The built-in synth (replaced by the plugin) is gone; nothing to refuse.
        assert!(synth.is_none());
    }

    #[test]
    fn a_copied_output_track_does_not_play_the_same_output_twice() {
        let (mut doc, _, _, device) = doc();
        let made = doc
            .set_outputs(
                device,
                OutputEdit {
                    on: &on(&[(1, "K")]),
                    off: &[],
                    leave: None,
                },
            )
            .unwrap();
        let copies = doc.duplicate_tracks(&made);
        let copy = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == copies[0])
            .unwrap();
        assert_eq!(copy.plugin_output, None);
        assert_eq!(doc.output_tracks(device).len(), 1);
    }
}
