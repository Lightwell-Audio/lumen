//! A MIDI track's clips as the engine plays them: in **runs** (ADR-0058).
//!
//! Clips that meet at a seam (one begins where the one before it stops,
//! neither loops: `edit::seams`) play as one engine clip, a run. A note
//! that runs past its clip's end is cut there unless the next clip of the
//! run carries it on, holding the same note; then it sounds on until it
//! ends or the run stops carrying it. Two things follow:
//!
//! - a note held across a clip cut in two, or across the start of an Overdub begun
//!   inside a clip, sustains instead of stopping at the seam;
//! - the engine knows a sounding note by its clip, the clip's start and the
//!   note's place in it (`hold.rs`). A run is known by its first clip, so
//!   while a recording moves the seams after it once a commit (about once a
//!   second), the notes playing keep their owner and are neither released
//!   nor struck again.
//!
//! Lengths are worked out here, so each note reaches the engine already cut
//! where it stops (a looping clip, always a run of its own, keeps its notes
//! whole: the engine cuts them at each repeat). Only what is in a clip's
//! window plays (ADR-0056); muted notes play nothing.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_engine::{MidiClip, MidiControl, MidiNote, control_list, note_list};
use lumen_project::edit::seams;
use lumen_project::model::{self, ClipContent, EventKind, Id};

/// A clip in a run, and how much of it plays (less than its length when a
/// Replace take is recording over its end).
#[derive(Clone, Copy)]
pub(crate) struct Member<'a> {
    pub clip: &'a Arc<model::Clip>,
    pub length: Ticks,
}

/// Groups `members` (in start order) into runs: each next member that
/// meets the one before at a seam joins its run.
pub(crate) fn runs<'a>(members: impl IntoIterator<Item = Member<'a>>) -> Vec<Vec<Member<'a>>> {
    let mut out: Vec<Vec<Member<'a>>> = Vec::new();
    for m in members {
        match out.last_mut() {
            Some(run)
                if run
                    .last()
                    .is_some_and(|p| seams::is_seam(p.clip, p.length, m.clip)) =>
            {
                run.push(m);
            }
            _ => out.push(vec![m]),
        }
    }
    out
}

fn content(clip: &model::Clip) -> (&[model::Note], &[model::ClipEvent]) {
    match &clip.content {
        ClipContent::Midi { notes, events } => (notes, events),
        ClipContent::Audio { .. } => (&[], &[]),
    }
}

/// Whether clip time `at` is in the window of a member playing `length`.
fn shows(m: &Member<'_>, at: Ticks) -> bool {
    lumen_project::edit::in_window(m.clip, at) && at < m.length
}

/// Whether the member's clip loops (it is then a run of its own).
fn loops(m: &Member<'_>) -> bool {
    m.clip.loop_length.is_some_and(|l| l.0 > 0)
}

/// The notes a run plays, in ticks from its first clip's start, each cut
/// where it stops (see the module documentation). Unsorted.
pub(crate) fn run_notes(run: &[Member<'_>]) -> Vec<MidiNote> {
    let Some(head) = run.first() else {
        return Vec::new();
    };
    let mut out: Vec<MidiNote> = Vec::new();
    // Notes sounding up to the end of the member before: arrangement start,
    // pitch, and where they are in `out`.
    let mut reaching: Vec<(Ticks, u8, usize)> = Vec::new();
    let mut carried: Vec<usize> = Vec::new();
    for (k, m) in run.iter().enumerate() {
        let offset = m.clip.start - head.clip.start;
        let (notes, _) = content(m.clip);
        let mut next: Vec<(Ticks, u8, usize)> = Vec::new();
        carried = match k {
            0 => Vec::new(),
            _ => {
                let p = &run[k - 1];
                seams::carried_on(p.clip, p.length, &carried, m.clip)
            }
        };
        for &i in &carried {
            let n = notes[i];
            let at = m.clip.start + n.start();
            if let Some(&(_, _, e)) = reaching.iter().find(|r| (r.0, r.1) == (at, n.pitch())) {
                out[e].length = offset + n.end().min(m.length) - out[e].start;
                if n.end() >= m.length {
                    next.push((at, n.pitch(), e));
                }
            }
        }
        for n in notes.iter().filter(|n| !n.muted() && shows(m, n.start())) {
            let length = if loops(m) {
                n.length()
            } else {
                n.length().min(m.length - n.start())
            };
            if !loops(m) && n.end() >= m.length {
                next.push((m.clip.start + n.start(), n.pitch(), out.len()));
            }
            out.push(MidiNote {
                start: offset + n.start(),
                length,
                pitch: n.pitch(),
                velocity: n.velocity(),
            });
        }
        reaching = next;
    }
    out
}

/// The controllers a run plays, in ticks from its first clip's start.
fn run_controls(run: &[Member<'_>]) -> Vec<MidiControl> {
    let Some(head) = run.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for m in run {
        let offset = m.clip.start - head.clip.start;
        let (_, events) = content(m.clip);
        for e in events.iter().filter(|e| shows(m, e.at)) {
            out.extend(control(e).into_iter().map(|mut c| {
                c.at = offset + c.at;
                c
            }));
        }
    }
    out
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

/// Where a run lies: its first clip's start, how long it plays, and the
/// loop of a looping clip.
fn extent(run: &[Member<'_>]) -> (Ticks, Ticks, Option<Ticks>) {
    let head = &run[0];
    let last = &run[run.len() - 1];
    let period = match run.len() {
        1 => head.clip.loop_length.filter(|l| l.0 > 0),
        _ => None,
    };
    let start = head.clip.start;
    (start, last.clip.start + last.length - start, period)
}

/// A run's translated lists, kept while its clips are unchanged.
struct Cached {
    /// The clips they were built from, and how much of each played (also
    /// keeps their addresses from being reused while the entry lives).
    members: Vec<(Arc<model::Clip>, Ticks)>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
}

impl Cached {
    fn matches(&self, run: &[Member<'_>]) -> bool {
        self.members.len() == run.len()
            && self.members.iter().zip(run).all(|((clip, length), m)| {
                *length == m.length && (Arc::ptr_eq(clip, m.clip) || **clip == **m.clip)
            })
    }
}

/// Translated runs by their first clip's id, so an edit elsewhere does not
/// copy a 100,000-note clip again and the engine's comparison is a pointer
/// check.
#[derive(Default)]
pub struct Cache {
    runs: HashMap<Id, Cached>,
}

impl Cache {
    /// The engine clip `run` plays as, its lists rebuilt only if it changed.
    pub(crate) fn clip(&mut self, run: &[Member<'_>]) -> MidiClip {
        let head = run[0].clip;
        let (start, length, period) = extent(run);
        let (notes, controls) = match self.runs.get(&head.id) {
            Some(c) if c.matches(run) => (Arc::clone(&c.notes), Arc::clone(&c.controls)),
            _ => {
                let notes = note_list(run_notes(run));
                let controls = control_list(run_controls(run));
                self.runs.insert(
                    head.id,
                    Cached {
                        members: run.iter().map(|m| (Arc::clone(m.clip), m.length)).collect(),
                        notes: Arc::clone(&notes),
                        controls: Arc::clone(&controls),
                    },
                );
                (notes, controls)
            }
        };
        MidiClip {
            id: head.id.0,
            start,
            length,
            period,
            notes,
            controls,
        }
    }

    /// Forgets runs whose first clip is not in `project` any more.
    pub(crate) fn retain(&mut self, project: &model::Project) {
        let live: std::collections::HashSet<Id> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        if self.runs.len() > live.len() {
            self.runs.retain(|id, _| live.contains(id));
        }
    }
}

/// The keys `track`'s clips hold at arrangement time `at`, as the engine
/// plays them (`hold.rs`): a note sounds from its start up to where it
/// stops, within its repeat of a looping clip's content. A zero-length note
/// sounds for one tick. Each key once.
pub(crate) fn sounding_keys(track: &model::Track, at: Ticks) -> Vec<u8> {
    let members = track
        .clips
        .iter()
        .filter(|c| !c.muted && c.length.0 > 0 && matches!(c.content, ClipContent::Midi { .. }))
        .map(|clip| Member {
            clip,
            length: clip.length,
        });
    let mut keys = Vec::new();
    for run in runs(members) {
        let (start, length, period) = extent(&run);
        let local = (at - start).0;
        if local < 0 || local >= length.0 {
            continue;
        }
        let local = period.map_or(local, |p| local % p.0);
        for n in run_notes(&run) {
            let (from, to) = (n.start.0, n.start.0 + n.length.0.max(1));
            // A loop's notes stop at the end of their repeat.
            let to = period.map_or(to, |p| to.min(p.0));
            if (from..to.max(from + 1)).contains(&local) && !keys.contains(&n.pitch) {
                keys.push(n.pitch);
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{Clip, Color, Note, Track, TrackKind};

    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Arc<Clip> {
        Arc::new(Clip {
            id: Id(id),
            name: String::new(),
            start: q(start),
            length: q(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        })
    }

    fn note(start: i64, length: i64, pitch: u8) -> Note {
        Note::new(q(start), q(length), pitch, 100)
    }

    fn whole(clips: &[Arc<Clip>]) -> Vec<Member<'_>> {
        clips
            .iter()
            .map(|clip| Member {
                clip,
                length: clip.length,
            })
            .collect()
    }

    /// `(start, length, pitch)` of a run's notes, in quarters, sorted.
    fn played(run: &[Member<'_>]) -> Vec<(i64, i64, u8)> {
        let mut out: Vec<_> = run_notes(run)
            .iter()
            .map(|n| (n.start.0 / q(1).0, n.length.0 / q(1).0, n.pitch))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn a_note_held_across_a_split_plays_whole_as_one_note() {
        // A note from 2 to 10 split at 4 and 6, with a short one on each side.
        let clips = [
            clip(1, 0, 4, vec![note(2, 8, 60), note(0, 1, 62)]),
            clip(2, 4, 2, vec![note(-2, 8, 60), note(1, 1, 64)]),
            clip(3, 6, 6, vec![note(-4, 8, 60), note(-6, 1, 62)]),
        ];
        let found = runs(whole(&clips));
        assert_eq!(found.len(), 1, "touching clips play as one");
        assert_eq!(played(&found[0]), vec![(0, 1, 62), (2, 8, 60), (5, 1, 64)]);
    }

    #[test]
    fn a_clip_that_does_not_hold_the_note_cuts_it_at_the_seam() {
        // Moved up against the first clip, the second holds no such note.
        let clips = [
            clip(1, 0, 4, vec![note(2, 8, 60)]),
            clip(2, 4, 4, vec![note(0, 1, 64)]),
        ];
        let found = runs(whole(&clips));
        assert_eq!(found.len(), 1);
        assert_eq!(played(&found[0]), vec![(2, 2, 60), (4, 1, 64)]);
    }

    #[test]
    fn apart_or_looping_clips_are_runs_of_their_own() {
        let clips = [
            clip(1, 0, 4, vec![note(2, 8, 60)]),
            clip(2, 5, 4, vec![note(-3, 8, 60)]),
        ];
        let found = runs(whole(&clips));
        assert_eq!(found.len(), 2);
        assert_eq!(played(&found[0]), vec![(2, 2, 60)]);
        assert_eq!(played(&found[1]), vec![], "hidden before its start");

        let mut looping = clip(3, 4, 8, vec![note(0, 3, 60)]);
        Arc::make_mut(&mut looping).loop_length = Some(q(2));
        let clips = [clip(1, 0, 4, vec![note(2, 8, 60)]), looping];
        let found = runs(whole(&clips));
        assert_eq!(found.len(), 2);
        assert_eq!(played(&found[1]), vec![(0, 3, 60)], "the engine cuts loops");
    }

    #[test]
    fn a_run_cut_short_stops_at_its_end() {
        // A Replace take records over the first clip from 3: no seam.
        let clips = [
            clip(1, 0, 4, vec![note(2, 8, 60)]),
            clip(2, 4, 4, vec![note(-2, 8, 60)]),
        ];
        let members = [
            Member {
                clip: &clips[0],
                length: q(3),
            },
            Member {
                clip: &clips[1],
                length: q(4),
            },
        ];
        let found = runs(members);
        assert_eq!(found.len(), 2);
        assert_eq!(played(&found[0]), vec![(2, 1, 60)]);
    }

    #[test]
    fn a_run_is_known_by_its_first_clip_while_the_seams_after_it_move() {
        // An Overdub begun at 4 inside a clip, committed twice: the notes keep
        // their places and lengths in the run, so the engine keeps them.
        let commit = |end: i64| {
            [
                clip(1, 0, 4, vec![note(2, 8, 60)]),
                clip(50, 4, end - 4, vec![note(-2, 8, 60), note(3, 4, 64)]),
                clip(
                    900,
                    end,
                    16 - end,
                    vec![note(2 - end, 8, 60), note(7 - end, 4, 64)],
                ),
            ]
        };
        let (a, b) = (commit(6), commit(9));
        let mut cache = Cache::default();
        let first = cache.clip(&runs(whole(&a))[0]);
        let second = cache.clip(&runs(whole(&b))[0]);
        assert_eq!((first.id, first.start), (second.id, second.start));
        assert_eq!(first.notes, second.notes);
        assert_eq!(played(&runs(whole(&b))[0]), vec![(2, 8, 60), (7, 4, 64)]);
    }

    #[test]
    fn sounding_keys_follow_a_note_across_a_seam() {
        let track = Track {
            clips: vec![
                clip(1, 0, 4, vec![note(2, 8, 60)]),
                clip(2, 4, 8, vec![note(-2, 8, 60)]),
            ],
            ..Track::new(Id(9), "t", TrackKind::Midi, Color(0))
        };
        assert_eq!(sounding_keys(&track, q(3)), vec![60]);
        assert_eq!(sounding_keys(&track, q(9)), vec![60]);
        assert!(sounding_keys(&track, q(10)).is_empty());
    }
}
