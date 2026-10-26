//! Importing Standard MIDI Files (M5 slice 7, owner's F1 and F2): a file
//! dropped on a MIDI track becomes one clip there; dropped between tracks or
//! on empty lanes, a new MIDI track per part. Notes are placed in beats, so
//! they follow the project's tempo; the file's tempo and meter are taken
//! only when asked ([`Document::use_file_tempo`]). Each is one undo step.

use std::sync::Arc;

use lumen_core::meter::{Meter, MeterPoint, tidy};
use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_project::edit;
use lumen_project::model::{
    Clip, ClipContent, ClipEvent, Device, Id, Note, Project, Track, TrackKind,
};
use lumen_project::smf::{FileTrack, MidiFile};

use super::clips::fresh_ids;
use super::{Document, MAX_BEATS_PER_BAR, instrument, track_color};

/// A clip's content from parts of a file: their notes and events merged,
/// in time order.
fn merged<'a>(parts: impl IntoIterator<Item = &'a FileTrack>) -> (Vec<Note>, Vec<ClipEvent>) {
    let (mut notes, mut events) = (Vec::new(), Vec::new());
    for part in parts {
        notes.extend_from_slice(&part.notes);
        events.extend_from_slice(&part.events);
    }
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    events.sort_by_key(|e| e.at);
    (notes, events)
}

/// A clip's length for content ending at `end`: whole bars of the
/// project's meter where the clip begins at `at`, one at least.
fn bars_for(project: &Project, at: Ticks, end: Ticks) -> Ticks {
    let bar = project.meter_map().meter_at(at).bar();
    Ticks(((end.0 + bar.0 - 1) / bar.0).max(1) * bar.0)
}

fn clip(
    id: Id,
    name: &str,
    at: Ticks,
    length: Ticks,
    notes: Vec<Note>,
    events: Vec<ClipEvent>,
) -> Arc<Clip> {
    Arc::new(Clip {
        id,
        name: name.into(),
        start: at,
        length,
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        content: ClipContent::Midi { notes, events },
    })
}

/// The new tracks' names: each part's track name, or the file's; a name
/// shared by several parts (a type 0 file split by channel) gets its
/// channel.
fn part_names(file: &MidiFile, fallback: &str) -> Vec<String> {
    let base: Vec<&str> = file
        .tracks
        .iter()
        .map(|p| p.name.as_deref().unwrap_or(fallback))
        .collect();
    file.tracks
        .iter()
        .zip(&base)
        .map(|(part, name)| {
            if base.iter().filter(|b| *b == name).count() > 1 {
                format!("{name} ch {}", part.channel + 1)
            } else {
                (*name).to_string()
            }
        })
        .collect()
}

impl Document {
    /// Puts every part of `file` as one clip named `name` on the MIDI track
    /// `track` at `at`, cutting away the unlocked clips it covers. Returns
    /// the clip, or nothing when the file holds no notes or events, the
    /// track is not a MIDI track, or a locked clip is in the way.
    pub fn import_midi_clip(
        &mut self,
        track: Id,
        at: Ticks,
        name: &str,
        file: &MidiFile,
    ) -> Option<Id> {
        let (notes, events) = merged(&file.tracks);
        if notes.is_empty() && events.is_empty() {
            return None;
        }
        let project = self.project();
        let target = project
            .tracks
            .iter()
            .find(|t| t.id == track && t.kind == TrackKind::Midi)?;
        let at = at.max(Ticks::ZERO);
        let length = bars_for(project, at, file.end());
        let end = at + length;
        if target
            .clips
            .iter()
            .any(|c| c.locked && c.start < end && c.start + c.length > at)
        {
            return None;
        }
        let id = self.fresh_id();
        let next = &mut self.next_id;
        self.history
            .apply(&format!("Import {name}"), None, |project| {
                if let Some(t) = project.tracks.iter_mut().find(|t| t.id == track) {
                    let t = Arc::make_mut(t);
                    t.clips.push(clip(id, name, at, length, notes, events));
                    t.clips.sort_by_key(|c| c.start);
                }
                edit::overwrite(project, &[id], &mut fresh_ids(next));
            });
        Some(id)
    }

    /// Adds a new MIDI track for each part of `file`, with the synth, each
    /// holding the part as one clip at `at`; before the track `before` (at
    /// the end when `None`), inside `group`. `name` names parts the file
    /// leaves unnamed. Returns the new tracks (none for an empty file).
    pub fn import_midi_tracks(
        &mut self,
        before: Option<Id>,
        group: Option<Id>,
        at: Ticks,
        name: &str,
        file: &MidiFile,
    ) -> Vec<Id> {
        let parts: Vec<&FileTrack> = file
            .tracks
            .iter()
            .filter(|p| !p.notes.is_empty() || !p.events.is_empty())
            .collect();
        if parts.is_empty() {
            return Vec::new();
        }
        let names = part_names(file, name);
        let names: Vec<&String> = file
            .tracks
            .iter()
            .zip(&names)
            .filter(|(p, _)| !p.notes.is_empty() || !p.events.is_empty())
            .map(|(_, n)| n)
            .collect();
        let at = at.max(Ticks::ZERO);
        let ids: Vec<(Id, Id, Id)> = parts
            .iter()
            .map(|_| (self.fresh_id(), self.fresh_id(), self.fresh_id()))
            .collect();
        let label = match ids.len() {
            1 => format!("Import {name}"),
            n => format!("Import {name} ({n} tracks)"),
        };
        self.history.apply(&label, None, |project| {
            let group = group.filter(|g| {
                project
                    .tracks
                    .iter()
                    .any(|t| t.id == *g && t.kind == TrackKind::Group)
            });
            let first = before
                .and_then(|id| project.tracks.iter().position(|t| t.id == id))
                .unwrap_or(project.tracks.len());
            let added = parts.iter().zip(&names).zip(&ids);
            for (k, ((part, track_name), &(track, synth, clip_id))) in added.enumerate() {
                let (notes, events) = merged([*part]);
                let length = bars_for(project, at, part.end());
                let color = track_color(project.new_track_color, project.tracks.len());
                project.tracks.insert(
                    first + k,
                    Arc::new(Track {
                        parent: group,
                        devices: vec![Device::synth(synth, instrument::DEFAULT_PRESET.id())],
                        clips: vec![clip(clip_id, track_name, at, length, notes, events)],
                        ..Track::new(track, track_name.as_str(), TrackKind::Midi, color)
                    }),
                );
            }
            lumen_project::groups::normalize(&mut project.tracks);
        });
        ids.into_iter().map(|(track, _, _)| track).collect()
    }

    /// Takes a MIDI file's tempo map and time signature (F2's "Use them"),
    /// as one undo step. A tempo map Lumen cannot hold is left out; so is
    /// the meter, when there is none. Clips stay where they are in beats.
    pub fn use_file_tempo(&mut self, tempo: &[TempoPoint], meter: &[MeterPoint]) {
        let tempo = TempoMap::new(tempo.to_vec())
            .ok()
            .map(|map| map.points().to_vec());
        let meter: Vec<MeterPoint> = meter
            .iter()
            .map(|p| MeterPoint {
                at: p.at,
                meter: Meter {
                    numerator: p.meter.numerator.clamp(1, MAX_BEATS_PER_BAR),
                    denominator: p.meter.denominator,
                },
            })
            .collect();
        let meter = (!meter.is_empty()).then(|| tidy(&meter));
        if tempo.is_none() && meter.is_none() {
            return;
        }
        self.history.apply("Use the file's tempo", None, |project| {
            if let Some(tempo) = tempo {
                project.tempo = tempo;
            }
            if let Some(meter) = meter {
                project.meter = meter;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::EventKind;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    /// Two parts: keys on channel 1 with a pedal, drums on channel 10.
    fn song() -> MidiFile {
        MidiFile {
            tracks: vec![
                FileTrack {
                    name: Some("Song".into()),
                    channel: 0,
                    notes: vec![Note::new(q(1), q(1), 60, 100)],
                    events: vec![ClipEvent {
                        at: q(0),
                        kind: EventKind::Controller {
                            number: 64,
                            value: 127,
                        },
                    }],
                    ..FileTrack::default()
                },
                FileTrack {
                    name: Some("Song".into()),
                    channel: 9,
                    notes: vec![
                        Note::new(q(0), q(1), 36, 120),
                        Note::new(q(5), q(1), 38, 90),
                    ],
                    ..FileTrack::default()
                },
            ],
            tempo: vec![
                TempoPoint {
                    at: Ticks::ZERO,
                    bpm: 96.0,
                    ramp: false,
                },
                TempoPoint {
                    at: q(8),
                    bpm: 140.0,
                    ramp: false,
                },
            ],
            meter: vec![MeterPoint {
                at: Ticks::ZERO,
                meter: Meter {
                    numerator: 6,
                    denominator: 8,
                },
            }],
        }
    }

    fn notes_of(clip: &Clip) -> &[Note] {
        match &clip.content {
            ClipContent::Midi { notes, .. } => notes,
            ClipContent::Audio { .. } => panic!("a MIDI clip"),
        }
    }

    #[test]
    fn a_file_on_a_midi_track_is_one_clip_of_whole_bars_cutting_what_it_covers() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let old = doc
            .create_midi_clip(track, super::super::NewClip::Range(q(0), q(16)))
            .unwrap();
        let steps = doc.history().steps().0.len();
        let id = doc.import_midi_clip(track, q(4), "song", &song()).unwrap();
        assert_eq!(doc.history().steps().0.len(), steps + 1, "one step");
        let clips = &doc.project().tracks[0].clips;
        let new = clips.iter().find(|c| c.id == id).unwrap();
        // The drums end at quarter 6: two 4/4 bars.
        assert_eq!(
            (new.start, new.length, new.name.as_str()),
            (q(4), q(8), "song")
        );
        let pitches: Vec<u8> = notes_of(new).iter().map(|n| n.pitch()).collect();
        assert_eq!(pitches, [36, 60, 38], "every channel, in time order");
        assert_eq!(clips.len(), 3, "the old clip is cut around it");
        assert!(clips.iter().any(|c| c.id == old && c.length == q(4)));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));

        // Not a MIDI track, or an empty file: nothing.
        let audio = doc.add_track(TrackKind::Audio, None);
        assert_eq!(doc.import_midi_clip(audio, q(0), "song", &song()), None);
        assert_eq!(
            doc.import_midi_clip(track, q(0), "empty", &MidiFile::default()),
            None
        );
        doc.undo();
        doc.undo();
        assert_eq!(doc.project().tracks[0].clips.len(), 1, "undone in one step");
    }

    #[test]
    fn a_file_between_tracks_makes_a_midi_track_per_part_with_the_synth() {
        let mut doc = Document::new(crate::empty_project());
        let first = doc.add_track(TrackKind::Audio, None);
        let last = doc.add_track(TrackKind::Audio, None);
        let steps = doc.history().steps().0.len();
        let ids = doc.import_midi_tracks(Some(last), None, q(2), "song", &song());
        assert_eq!(doc.history().steps().0.len(), steps + 1, "one step");
        let tracks = &doc.project().tracks;
        let order: Vec<Id> = tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, [first, ids[0], ids[1], last]);
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names[1..3], ["Song ch 1", "Song ch 10"]);
        let keys = &tracks[1];
        assert_eq!(keys.kind, TrackKind::Midi);
        assert!(keys.instrument().is_some(), "with the synth");
        assert_eq!((keys.clips[0].start, keys.clips[0].length), (q(2), q(4)));
        assert_eq!(notes_of(&tracks[2].clips[0]).len(), 2);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        assert!(
            doc.import_midi_tracks(None, None, q(0), "x", &MidiFile::default())
                .is_empty()
        );
    }

    #[test]
    fn unnamed_parts_take_the_files_name() {
        let mut file = song();
        file.tracks[0].name = None;
        file.tracks[1].name = Some("Drums".into());
        assert_eq!(part_names(&file, "groove"), ["groove", "Drums"]);
    }

    #[test]
    fn the_files_tempo_and_meter_come_in_as_one_step() {
        let mut doc = Document::new(crate::empty_project());
        let file = song();
        doc.use_file_tempo(&file.tempo, &file.meter);
        let project = doc.project();
        assert_eq!(project.tempo, file.tempo);
        assert_eq!(project.meter, file.meter);
        doc.undo();
        assert_eq!(doc.project().tempo.len(), 1);
        // A map Lumen cannot hold is left out; the meter still comes.
        doc.use_file_tempo(
            &[TempoPoint {
                at: q(1),
                bpm: 90.0,
                ramp: false,
            }],
            &[MeterPoint {
                at: Ticks::ZERO,
                meter: Meter {
                    numerator: 200,
                    denominator: 4,
                },
            }],
        );
        assert_eq!(doc.project().tempo.len(), 1);
        assert_eq!(doc.project().meter[0].meter.numerator, 64);
    }

    #[test]
    fn a_file_with_changes_of_meter_brings_them_along() {
        let mut doc = Document::new(crate::empty_project());
        let changes = [
            MeterPoint {
                at: Ticks::ZERO,
                meter: Meter::FOUR_FOUR,
            },
            MeterPoint {
                at: q(8),
                meter: Meter {
                    numerator: 7,
                    denominator: 8,
                },
            },
        ];
        doc.use_file_tempo(&[], &changes);
        assert_eq!(doc.project().meter, changes);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }
}
