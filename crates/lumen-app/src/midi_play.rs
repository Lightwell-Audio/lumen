//! The engine's MIDI tracks derived from the project (M5 slice 1, ADR-0045):
//! each MIDI track's clips as notes and controller messages in ticks, with
//! loop repeats, where they go (the track's instrument, or a MIDI output
//! port) and which instrument plays them. Pure, like `lanes::engine_clips`
//! and `routing::routing`, so the translation is tested.
//!
//! Clips that did not change keep their note lists (shared by clip, so the
//! engine's comparison is a pointer check and a 100,000-note clip is not
//! copied on every edit elsewhere).

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_engine::{MidiClip, MidiControl, MidiNote, MidiTrack, Target, control_list, note_list};
use lumen_project::model::{self, ClipContent, EventKind, Id, Project, TrackKind};

/// A clip's translated lists, kept while the clip itself is unchanged.
struct Cached {
    /// The model clip they were built from (also keeps its address from
    /// being reused while the entry lives).
    source: Arc<model::Clip>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
}

/// Translated clips by clip id.
#[derive(Default)]
pub struct Cache {
    clips: HashMap<Id, Cached>,
}

impl Cache {
    /// The lists for `clip`, rebuilt only if it changed.
    fn lists(&mut self, clip: &Arc<model::Clip>) -> (Arc<[MidiNote]>, Arc<[MidiControl]>) {
        if let Some(c) = self.clips.get(&clip.id)
            && (Arc::ptr_eq(&c.source, clip) || *c.source == **clip)
        {
            return (Arc::clone(&c.notes), Arc::clone(&c.controls));
        }
        let ClipContent::Midi { notes, events } = &clip.content else {
            return (note_list(Vec::new()), control_list(Vec::new()));
        };
        let notes = note_list(
            notes
                .iter()
                .map(|n| MidiNote {
                    start: n.start(),
                    length: n.length(),
                    pitch: n.pitch(),
                    velocity: n.velocity(),
                })
                .collect(),
        );
        let controls = control_list(events.iter().flat_map(control).collect());
        self.clips.insert(
            clip.id,
            Cached {
                source: Arc::clone(clip),
                notes: Arc::clone(&notes),
                controls: Arc::clone(&controls),
            },
        );
        (notes, controls)
    }

    /// Forgets clips that are not in `project` any more.
    fn retain(&mut self, project: &Project) {
        let live: std::collections::HashSet<Id> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        if self.clips.len() > live.len() {
            self.clips.retain(|id, _| live.contains(id));
        }
    }
}

/// A clip event as the messages that carry it (a program change with a
/// bank sends the bank first).
fn control(event: &model::ClipEvent) -> Vec<MidiControl> {
    let at = event.at;
    let msg = |m: Msg| MidiControl { at, msg: m };
    match event.kind {
        EventKind::Controller { number, value } => vec![msg(Msg::controller(0, number, value))],
        EventKind::PitchBend { value } => vec![msg(Msg::pitch_bend(0, value))],
        EventKind::Pressure { value } => vec![msg(Msg::pressure(0, value))],
        EventKind::Program { program, bank } => {
            let mut out = Vec::new();
            if let Some(bank) = bank {
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_MSB,
                    (bank >> 7) as u8,
                )));
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_LSB,
                    (bank & 127) as u8,
                )));
            }
            out.push(msg(Msg::program(0, program)));
            out
        }
    }
}

/// What the engine plays: every MIDI track's unmuted clips, in start
/// order, to the track's instrument.
pub fn engine_midi(project: &Project, cache: &mut Cache) -> Vec<MidiTrack> {
    cache.retain(project);
    let mut tracks = Vec::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Midi {
            continue;
        }
        let clips = track
            .clips
            .iter()
            .filter(|c| !c.muted && matches!(c.content, ClipContent::Midi { .. }))
            .filter(|c| c.length.0 > 0)
            .map(|clip| {
                let (notes, controls) = cache.lists(clip);
                MidiClip {
                    id: clip.id.0,
                    start: clip.start,
                    length: clip.length,
                    period: clip.loop_length.filter(|l| l.0 > 0),
                    notes,
                    controls,
                }
            })
            .collect();
        tracks.push(MidiTrack::new(track.id.0, Target::Instrument, None, clips));
    }
    tracks
}

#[cfg(test)]
mod tests {
    use lumen_core::midi::Kind;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, ClipEvent, Note};

    use super::*;
    use crate::document::Document;

    fn project_with(clip: Clip) -> (Project, Id) {
        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Midi, None);
        let id = doc.project().tracks[0].id;
        doc.apply("add clip", None, |p| {
            Arc::make_mut(&mut p.tracks[0]).clips.push(Arc::new(clip));
        });
        (doc.project().clone(), id)
    }

    fn midi_clip(notes: Vec<Note>, events: Vec<ClipEvent>) -> Clip {
        Clip {
            id: Id(900),
            name: "Part".into(),
            start: Ticks::quarters(4),
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            content: ClipContent::Midi { notes, events },
        }
    }

    #[test]
    fn notes_and_events_reach_the_engine_sorted_with_their_loop() {
        let q = Ticks::quarters;
        let mut clip = midi_clip(
            vec![Note(q(2), q(1), 64, 90), Note(q(0), q(1), 60, 100)],
            vec![
                ClipEvent {
                    at: q(1),
                    kind: EventKind::Program {
                        program: 4,
                        bank: Some(130),
                    },
                },
                ClipEvent {
                    at: q(0),
                    kind: EventKind::PitchBend { value: -100 },
                },
            ],
        );
        clip.loop_length = Some(q(4));
        let (project, track) = project_with(clip);
        let tracks = engine_midi(&project, &mut Cache::default());
        assert_eq!(tracks.len(), 1);
        let t = &tracks[0];
        assert_eq!(t.id, track.0);
        let c = &t.clips[0];
        assert_eq!((c.start, c.length, c.period), (q(4), q(8), Some(q(4))));
        let starts: Vec<_> = c.notes.iter().map(|n| (n.start, n.pitch)).collect();
        assert_eq!(starts, vec![(q(0), 60), (q(2), 64)]);
        let kinds: Vec<_> = c.controls.iter().map(|m| (m.at, m.msg.kind())).collect();
        assert_eq!(
            kinds,
            vec![
                (q(0), Kind::PitchBend { value: -100 }),
                (
                    q(1),
                    Kind::Controller {
                        number: 0,
                        value: 1
                    }
                ),
                (
                    q(1),
                    Kind::Controller {
                        number: 32,
                        value: 2
                    }
                ),
                (q(1), Kind::Program { program: 4 }),
            ]
        );
    }

    #[test]
    fn muted_clips_audio_tracks_and_empty_clips_play_nothing() {
        let mut muted = midi_clip(vec![Note(Ticks::ZERO, Ticks::quarters(1), 60, 100)], vec![]);
        muted.muted = true;
        let (project, _) = project_with(muted);
        let tracks = engine_midi(&project, &mut Cache::default());
        assert!(
            tracks[0].clips.is_empty(),
            "the track stays, so its notes are released"
        );

        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Audio, None);
        assert!(engine_midi(doc.project(), &mut Cache::default()).is_empty());
    }

    #[test]
    fn an_unchanged_clip_keeps_its_lists() {
        let clip = midi_clip(vec![Note(Ticks::ZERO, Ticks::quarters(1), 60, 100)], vec![]);
        let (project, _) = project_with(clip);
        let mut cache = Cache::default();
        let a = engine_midi(&project, &mut cache);
        let b = engine_midi(&project, &mut cache);
        assert!(Arc::ptr_eq(&a[0].clips[0].notes, &b[0].clips[0].notes));
    }
}
