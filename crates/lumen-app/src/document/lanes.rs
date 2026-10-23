//! Velocity and lane edits as undoable `Document` methods (M5 slice 4a). The
//! arithmetic is `lumen_project::edit::{velocity, events}`; each method here
//! is one undo step through the history, as the note edits are (`notes.rs`).
//!
//! Velocity edits change a clip's notes and never its events; lane edits
//! change its events and never its notes. Selections are indices into the
//! list an edit works on, and every method returns what it answers with: the
//! indices of the items picked or changed, in the list after the edit (see
//! the selection contracts in `lumen_project::edit::{velocity, events}`).
//!
//! Gestures (a drag through a lane or the velocity lane) take `base`, the
//! project as the gesture found it, and redo the whole edit from there each
//! frame; with the same `merge` key the frames are one undo step. One-shot
//! edits work on the project as it is.
//!
//! A locked clip, a clip that is not there and an audio clip refuse every
//! edit: nothing changes and the selection given comes back (none, for an
//! edit that takes none).

use std::sync::Arc;

use lumen_core::curve::Curve;
use lumen_core::time::Ticks;
use lumen_project::edit::{self, events as ev, velocity as vel};
use lumen_project::model::{Clip, ClipContent, ClipEvent, Id, Note, Project};

use super::{Document, plural};

/// The most events one drawn line or curve makes on its lane (its end point
/// included). A longer span is drawn at a coarser step; see [`span_step`].
pub const MAX_SPAN_EVENTS: usize = 4000;

/// How an edit takes part in a drag: the project the gesture began from and the
/// key that merges its frames into one undo step. `Drag::default()` is a one-shot edit.
#[derive(Debug, Clone, Copy, Default)]
pub struct Drag<'a> {
    /// The project the gesture began from; `None` edits the project as it is.
    pub base: Option<&'a Project>,
    /// The key that merges the gesture's frames into one undo step.
    pub merge: Option<&'a str>,
}

/// A line or curve between two (tick, value) ends, shaped by a curve.
#[derive(Debug, Clone, Copy)]
pub struct Line<'a, V> {
    /// The start of the line: a tick and a value.
    pub from: (Ticks, V),
    /// The end of the line: a tick and a value.
    pub to: (Ticks, V),
    /// The shape of the line between its ends.
    pub curve: &'a Curve,
}

/// How far a group of points moves.
#[derive(Debug, Clone, Copy)]
pub struct Shift {
    /// The move in ticks.
    pub time: Ticks,
    /// The move in the lane's value.
    pub value: i32,
}

impl Document {
    /// Runs `change` on the notes and the events of the unlocked MIDI clip
    /// `clip` (starting from `base` when given) as one step, with the clip's
    /// content length. Returns the selection `change` answers with, or
    /// `selection` when the clip is refused.
    fn edit_midi(
        &mut self,
        label: &str,
        base: Option<&Project>,
        merge: Option<&str>,
        clip: Id,
        selection: &[usize],
        change: impl FnOnce(&mut Vec<Note>, &mut Vec<ClipEvent>, Ticks) -> Vec<usize>,
    ) -> Vec<usize> {
        let source = base.unwrap_or_else(|| self.project());
        if find(source, clip).is_none() {
            return selection.to_vec();
        }
        let mut result = selection.to_vec();
        self.history.apply(label, merge, |project| {
            if let Some(b) = base {
                *project = b.clone();
            }
            for track in &mut project.tracks {
                let Some(at) = track
                    .clips
                    .iter()
                    .position(|c| c.id == clip && !c.locked && is_midi(c))
                else {
                    continue;
                };
                let track = Arc::make_mut(track);
                let clip = Arc::make_mut(&mut track.clips[at]);
                let length = edit::content_length(clip);
                if let ClipContent::Midi { notes, events } = &mut clip.content {
                    result = change(notes, events, length);
                }
                break;
            }
        });
        result
    }

    /// [`edit_midi`](Self::edit_midi) for a change to the clip's events only.
    fn edit_events(
        &mut self,
        label: &str,
        base: Option<&Project>,
        merge: Option<&str>,
        clip: Id,
        selection: &[usize],
        change: impl FnOnce(&mut Vec<ClipEvent>, Ticks) -> Vec<usize>,
    ) -> Vec<usize> {
        self.edit_midi(label, base, merge, clip, selection, |_, events, length| {
            change(events, length)
        })
    }

    /// [`edit_midi`](Self::edit_midi) for a change to the clip's notes only.
    fn edit_velocities(
        &mut self,
        label: &str,
        base: Option<&Project>,
        merge: Option<&str>,
        clip: Id,
        selection: &[usize],
        change: impl FnOnce(&mut Vec<Note>) -> Vec<usize>,
    ) -> Vec<usize> {
        self.edit_midi(label, base, merge, clip, selection, |notes, _, _| {
            change(notes)
        })
    }

    /// Sets each listed note's velocity (the pencil in the velocity lane;
    /// a later entry for the same note wins). A drag, from `base`. Refused,
    /// it answers with the indices named in `changes`.
    pub fn set_velocities_each(
        &mut self,
        base: Option<&Project>,
        clip: Id,
        changes: &[(usize, u8)],
        merge: Option<&str>,
    ) -> Vec<usize> {
        let named: Vec<usize> = changes.iter().map(|&(i, _)| i).collect();
        self.edit_velocities("Draw velocities", base, merge, clip, &named, |notes| {
            vel::set_each(notes, changes)
        })
    }

    /// Sets the picked notes' velocities along a line from `from` to `to`
    /// (each a tick and a velocity) shaped by `curve`. A drag, from `base`.
    /// Returns the picked indices.
    pub fn ramp_velocities(
        &mut self,
        drag: Drag<'_>,
        clip: Id,
        selection: &[usize],
        line: Line<'_, u8>,
    ) -> Vec<usize> {
        let Line { from, to, curve } = line;
        self.edit_velocities(
            "Ramp velocities",
            drag.base,
            drag.merge,
            clip,
            selection,
            |notes| vel::ramp(notes, selection, from, to, curve),
        )
    }

    /// Adds `delta` to the picked notes' velocities (clamped to 1..=127).
    /// Returns the picked indices.
    pub fn offset_velocities(&mut self, clip: Id, selection: &[usize], delta: i32) -> Vec<usize> {
        let label = if delta < 0 {
            "Lower velocities"
        } else {
            "Raise velocities"
        };
        self.edit_velocities(label, None, None, clip, selection, |notes| {
            vel::offset(notes, selection, delta)
        })
    }

    /// Moves the picked points of `lane` by `time` ticks and `value` as a
    /// group, clamped as a whole (see `events::move_points`); a drag, from
    /// `base`. Returns the moved points' indices.
    pub fn move_lane_points(
        &mut self,
        drag: Drag<'_>,
        clip: Id,
        lane: ev::Lane,
        selection: &[usize],
        shift: Shift,
    ) -> Vec<usize> {
        let label = plural(selection.len(), "Move point", "Move {} points");
        let Shift { time, value } = shift;
        self.edit_events(
            &label,
            drag.base,
            drag.merge,
            clip,
            selection,
            |events, length| ev::move_points(events, lane, selection, time, value, length),
        )
    }

    /// Deletes the picked events. Nothing remains selected.
    pub fn delete_lane_points(&mut self, clip: Id, selection: &[usize]) -> Vec<usize> {
        let label = plural(selection.len(), "Delete point", "Delete {} points");
        self.edit_events(&label, None, None, clip, selection, |events, _| {
            ev::delete_points(events, selection)
        })
    }

    /// Draws a line or curve on `lane` from `from` to `to` (each a tick and a
    /// value), shaped by `curve`, with a point every `step` ticks. The step is
    /// raised where the span would make more than [`MAX_SPAN_EVENTS`] points.
    /// The line replaces the lane's points between its two ends, clipped to
    /// the clip; the rest of the lane and every other lane stay. A drag, from
    /// `base`. Returns the points it added.
    pub fn draw_lane_span(
        &mut self,
        drag: Drag<'_>,
        clip: Id,
        lane: ev::Lane,
        line: Line<'_, i32>,
        step: Ticks,
    ) -> Vec<usize> {
        let Line { from, to, curve } = line;
        let label = if curve.is_linear() {
            "Draw line"
        } else {
            "Draw curve"
        };
        self.edit_events(label, drag.base, drag.merge, clip, &[], |events, length| {
            let last = length - Ticks(1);
            let (lo, hi) = (from.0.min(to.0), from.0.max(to.0));
            if length <= Ticks::ZERO || hi < Ticks::ZERO || lo > last {
                return Vec::new();
            }
            let step = step.max(span_step(from.0, to.0));
            let points = ev::ramp(lane, from, to, curve, step);
            let (lo, hi) = (lo.max(Ticks::ZERO), hi.min(last));
            ev::replace_span(events, lane, lo, hi, points, length)
        })
    }

    /// Draws the points of `lane` as the pencil does: each `(tick, value)` is
    /// one event, and the lane's points between the first and the last tick
    /// are replaced by them. Points are sorted by tick, a later one on the
    /// same tick wins, and ticks are clamped into the clip, as the pencil does.
    /// Empty `points` change nothing and record no step. A drag, from `base`.
    /// Returns the added points' indices.
    pub fn draw_lane_points(
        &mut self,
        base: Option<&Project>,
        clip: Id,
        lane: ev::Lane,
        points: &[(Ticks, i32)],
        merge: Option<&str>,
    ) -> Vec<usize> {
        if points.is_empty() {
            return Vec::new();
        }
        self.edit_events(
            "Draw controller",
            base,
            merge,
            clip,
            &[],
            |events, length| {
                if length <= Ticks::ZERO {
                    return Vec::new();
                }
                let last = length - Ticks(1);
                // A stable sort keeps a later point after an earlier one on its
                // tick, so the pass below keeps the later one.
                let mut sorted = points.to_vec();
                sorted.sort_by_key(|&(at, _)| at);
                let mut drawn: Vec<ClipEvent> = Vec::with_capacity(sorted.len());
                for (at, value) in sorted {
                    let at = at.max(Ticks::ZERO).min(last);
                    let event = ClipEvent {
                        at,
                        kind: lane.event(value),
                    };
                    if let Some(previous) = drawn.last_mut().filter(|p| p.at == at) {
                        *previous = event;
                    } else {
                        drawn.push(event);
                    }
                }
                let (Some(first), Some(end)) = (drawn.first(), drawn.last()) else {
                    return Vec::new();
                };
                let (from, to) = (first.at, end.at);
                ev::replace_span(events, lane, from, to, drawn, length)
            },
        )
    }
}

/// The step that keeps a span from `from` to `to` within [`MAX_SPAN_EVENTS`]
/// points. With points every `step` ticks from one end to the other, the
/// count is at most `ceil(span / step) + 1`, so a step of
/// `ceil(span / (MAX_SPAN_EVENTS - 1))` (or more) keeps it at the cap. A
/// span too long for `i64` steps are saturated.
fn span_step(from: Ticks, to: Ticks) -> Ticks {
    let span = u128::from(from.0.abs_diff(to.0));
    let per_step = (MAX_SPAN_EVENTS - 1) as u128;
    Ticks(i64::try_from(span.div_ceil(per_step)).unwrap_or(i64::MAX))
}

fn is_midi(clip: &Clip) -> bool {
    matches!(clip.content, ClipContent::Midi { .. })
}

/// The unlocked MIDI clip `id`.
fn find(project: &Project, id: Id) -> Option<&Arc<Clip>> {
    project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id && !c.locked && is_midi(c))
}

#[cfg(test)]
mod tests {
    use lumen_core::time::PPQ;
    use lumen_project::model::{EventKind, TrackKind};

    use super::super::midi_clips::NewClip;
    use super::*;

    const CC7: ev::Lane = ev::Lane::Controller(7);

    fn q(n: i64) -> Ticks {
        Ticks(PPQ * n)
    }

    fn t(n: i64) -> Ticks {
        Ticks(n)
    }

    fn note(start: i64, len: i64, pitch: u8, velocity: u8) -> Note {
        Note::new(q(start), q(len), pitch, velocity)
    }

    fn cc(at: i64, number: u8, value: u8) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::Controller { number, value },
        }
    }

    fn bend(at: i64, value: i16) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::PitchBend { value },
        }
    }

    fn program(at: i64) -> ClipEvent {
        ClipEvent {
            at: t(at),
            kind: EventKind::Program {
                program: 3,
                bank: None,
            },
        }
    }

    /// Three notes, one per quarter, with velocities 40, 80 and 120.
    fn notes() -> Vec<Note> {
        vec![note(0, 1, 60, 40), note(1, 1, 62, 80), note(2, 1, 64, 120)]
    }

    /// A controller 7, a controller 1, a pitch bend and a controller 7 again,
    /// on quarters 0, 1, 2 and 3, with a program change at quarter 1.
    fn events() -> Vec<ClipEvent> {
        vec![
            cc(0, 7, 10),
            cc(PPQ, 1, 40),
            program(PPQ),
            bend(2 * PPQ, 100),
            cc(3 * PPQ, 7, 60),
        ]
    }

    /// A document with a 16-quarter MIDI clip holding `notes` and `events`;
    /// returns it with the clip's id.
    fn doc_with(notes: Vec<Note>, events: Vec<ClipEvent>) -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let clip = doc
            .create_midi_clip(track, NewClip::Range(q(4), q(20)))
            .unwrap();
        doc.apply("fill", None, |p| {
            let t = Arc::make_mut(&mut p.tracks[0]);
            if let ClipContent::Midi {
                notes: n,
                events: e,
            } = &mut Arc::make_mut(&mut t.clips[0]).content
            {
                *n = notes;
                *e = events;
            }
        });
        (doc, clip)
    }

    /// The standard test clip: [`notes`] and [`events`].
    fn standard() -> (Document, Id) {
        doc_with(notes(), events())
    }

    fn content(doc: &Document) -> (Vec<Note>, Vec<ClipEvent>) {
        match &doc.project().tracks[0].clips[0].content {
            ClipContent::Midi { notes, events } => (notes.clone(), events.clone()),
            _ => unreachable!(),
        }
    }

    fn velocities(doc: &Document) -> Vec<u8> {
        content(doc).0.iter().map(|n| n.velocity()).collect()
    }

    /// (tick, value) of `lane`'s points, in order.
    fn points(events: &[ClipEvent], lane: ev::Lane) -> Vec<(i64, i32)> {
        events
            .iter()
            .filter_map(|e| lane.value_of(e.kind).map(|v| (e.at.0, v)))
            .collect()
    }

    /// An audio clip on a new audio track, which the lane and velocity edits
    /// must refuse.
    fn with_audio_clip(doc: &mut Document) -> Id {
        doc.add_track(TrackKind::Audio, None);
        let clip = doc.fresh_id();
        doc.apply("audio clip", None, |p| {
            if let Some(track) = p.tracks.last_mut() {
                Arc::make_mut(track).clips.push(Arc::new(Clip {
                    id: clip,
                    name: "Audio".into(),
                    start: q(0),
                    length: q(4),
                    muted: false,
                    color: None,
                    locked: false,
                    fade_in: Default::default(),
                    fade_out: Default::default(),
                    loop_length: None,
                    loop_phase: Default::default(),
                    envelopes: Default::default(),
                    audio_phase_seconds: 0.0,
                    content: ClipContent::Audio {
                        media: Id(0),
                        offset: 0,
                        gain_db: 0.0,
                    },
                }));
            }
        });
        clip
    }

    #[test]
    fn each_velocity_edit_changes_the_notes_and_leaves_the_events() {
        let (mut doc, clip) = standard();
        let events0 = content(&doc).1;
        assert_eq!(
            doc.set_velocities_each(None, clip, &[(0, 90), (2, 90)], None),
            [0, 2]
        );
        assert_eq!(velocities(&doc), [90, 80, 90]);
        assert_eq!(doc.offset_velocities(clip, &[1], -30), [1]);
        assert_eq!(velocities(&doc), [90, 50, 90]);
        assert_eq!(
            doc.set_velocities_each(None, clip, &[(2, 10), (9, 5)], None),
            [2]
        );
        assert_eq!(velocities(&doc), [90, 50, 10]);
        assert_eq!(
            doc.ramp_velocities(
                Drag::default(),
                clip,
                &[0, 1, 2],
                Line {
                    from: (q(0), 20),
                    to: (q(2), 100),
                    curve: &Curve::LINEAR
                }
            ),
            [0, 1, 2]
        );
        // Ramped at each note's start: 0, halfway, and the end.
        assert_eq!(velocities(&doc), [20, 60, 100]);
        assert_eq!(
            content(&doc).1,
            events0,
            "events are untouched by velocity edits"
        );
    }

    #[test]
    fn each_lane_edit_changes_the_points_and_leaves_the_notes() {
        let (mut doc, clip) = standard();
        let notes0 = content(&doc).0;

        // A point on a tick with a bend: it goes after the events on its tick.
        assert_eq!(
            doc.draw_lane_points(None, clip, CC7, &[(q(2), 5)], None),
            [4]
        );
        assert_eq!(
            points(&content(&doc).1, CC7),
            [(0, 10), (2 * PPQ, 5), (3 * PPQ, 60)]
        );
        // A point on an existing point of its lane replaces it.
        assert_eq!(
            doc.draw_lane_points(None, clip, CC7, &[(q(3), 90)], None),
            [5]
        );
        assert_eq!(points(&content(&doc).1, CC7)[2], (3 * PPQ, 90));
        assert_eq!(content(&doc).0, notes0, "notes are untouched by lane edits");

        let base = doc.project().clone();
        assert_eq!(
            doc.move_lane_points(
                Drag {
                    base: Some(&base),
                    merge: None
                },
                clip,
                CC7,
                &[0, 5],
                Shift {
                    time: t(PPQ / 2),
                    value: 5
                }
            ),
            [0, 5]
        );
        assert_eq!(
            points(&content(&doc).1, CC7),
            [(PPQ / 2, 15), (2 * PPQ, 5), (7 * PPQ / 2, 95)]
        );
        assert_eq!(content(&doc).0, notes0);

        assert!(doc.delete_lane_points(clip, &[1, 3]).is_empty());
        assert_eq!(
            content(&doc).1.len(),
            4,
            "the controller 1 and the bend went"
        );
        assert_eq!(content(&doc).0, notes0);
    }

    #[test]
    fn a_line_replaces_the_lane_between_its_ends_and_nothing_else() {
        let (mut doc, clip) = standard();
        let before = content(&doc).1;
        let added = doc.draw_lane_span(
            Drag::default(),
            clip,
            CC7,
            Line {
                from: (t(0), 0),
                to: (q(2), 100),
                curve: &Curve::LINEAR,
            },
            ev::GRID,
        );
        let events = content(&doc).1;
        let ramp = points(&events, CC7);
        assert_eq!(ramp.first(), Some(&(0, 0)));
        assert_eq!(ramp[ramp.len() - 2], (2 * PPQ, 100), "lands on the end");
        assert_eq!(
            ramp.last(),
            Some(&(3 * PPQ, 60)),
            "the point past the end stays"
        );
        assert_eq!(
            added.len(),
            9,
            "nine points, a quarter apart over two quarters"
        );
        assert!(
            added
                .iter()
                .all(|&i| CC7.value_of(events[i].kind).is_some())
        );
        // Controller 1, the program change and the bend are as they were.
        let others = |e: &[ClipEvent]| -> Vec<ClipEvent> {
            e.iter()
                .copied()
                .filter(|x| ev::Lane::of(x.kind) != Some(CC7))
                .collect()
        };
        assert_eq!(others(&events), others(&before));
    }

    #[test]
    fn a_curve_is_drawn_with_its_own_shape() {
        let (mut doc, clip) = standard();
        doc.draw_lane_span(
            Drag::default(),
            clip,
            CC7,
            Line {
                from: (t(0), 0),
                to: (q(2), 127),
                curve: &Curve::SLOW,
            },
            ev::GRID,
        );
        assert_eq!(doc.undo().as_deref(), Some("Draw curve"));
    }

    #[test]
    fn a_pencil_stroke_sorts_its_points_and_a_later_one_wins_on_a_tick() {
        let (mut doc, clip) = doc_with(vec![], vec![cc(8 * PPQ, 7, 99)]);
        let added =
            doc.draw_lane_points(None, clip, CC7, &[(q(2), 20), (q(1), 10), (q(2), 60)], None);
        assert_eq!(added, [0, 1]);
        assert_eq!(
            content(&doc).1,
            [cc(PPQ, 7, 10), cc(2 * PPQ, 7, 60), cc(8 * PPQ, 7, 99)],
            "the range 1 to 2 quarters is replaced; the point at 8 stays"
        );
    }

    #[test]
    fn a_pencil_stroke_is_clamped_into_the_clip_and_empty_strokes_do_nothing() {
        let (mut doc, clip) = doc_with(vec![], vec![]);
        let last = 16 * PPQ - 1;
        let added =
            doc.draw_lane_points(None, clip, CC7, &[(t(-5), 3), (t(i64::MAX / 2), 7)], None);
        assert_eq!(added, [0, 1]);
        assert_eq!(content(&doc).1, [cc(0, 7, 3), cc(last, 7, 7)]);
        let state = doc.state();
        assert!(doc.draw_lane_points(None, clip, CC7, &[], None).is_empty());
        assert_eq!(doc.state(), state, "an empty stroke records no step");
    }

    #[test]
    fn a_drawn_pencil_point_is_clamped_by_the_lane() {
        let (mut doc, clip) = doc_with(vec![], vec![]);
        doc.draw_lane_points(None, clip, ev::Lane::PitchBend, &[(t(5), -20_000)], None);
        assert_eq!(content(&doc).1, [bend(5, -8192)]);
    }

    #[test]
    fn every_edit_is_one_undo_step_that_restores_the_exact_state() {
        let (mut doc, clip) = standard();
        let start = doc.project().clone();
        type Edit = (&'static str, Box<dyn Fn(&mut Document)>);
        let edits: Vec<Edit> = vec![
            (
                "Draw velocities",
                Box::new(move |d| {
                    d.set_velocities_each(None, clip, &[(0, 77), (1, 77)], None);
                }),
            ),
            (
                "Draw velocities",
                Box::new(move |d| {
                    d.set_velocities_each(None, clip, &[(2, 33)], None);
                }),
            ),
            (
                "Ramp velocities",
                Box::new(move |d| {
                    d.ramp_velocities(
                        Drag::default(),
                        clip,
                        &[0, 1, 2],
                        Line {
                            from: (q(0), 10),
                            to: (q(2), 120),
                            curve: &Curve::LINEAR,
                        },
                    );
                }),
            ),
            (
                "Raise velocities",
                Box::new(move |d| {
                    d.offset_velocities(clip, &[0], 5);
                }),
            ),
            (
                "Lower velocities",
                Box::new(move |d| {
                    d.offset_velocities(clip, &[0], -5);
                }),
            ),
            (
                "Draw controller",
                Box::new(move |d| {
                    d.draw_lane_points(None, clip, CC7, &[(q(2), 5)], None);
                }),
            ),
            (
                "Move 2 points",
                Box::new(move |d| {
                    let base = d.project().clone();
                    d.move_lane_points(
                        Drag {
                            base: Some(&base),
                            merge: None,
                        },
                        clip,
                        CC7,
                        &[0, 4],
                        Shift {
                            time: t(PPQ / 2),
                            value: 5,
                        },
                    );
                }),
            ),
            (
                "Move point",
                Box::new(move |d| {
                    let base = d.project().clone();
                    d.move_lane_points(
                        Drag {
                            base: Some(&base),
                            merge: None,
                        },
                        clip,
                        CC7,
                        &[0],
                        Shift {
                            time: t(PPQ / 2),
                            value: 5,
                        },
                    );
                }),
            ),
            (
                "Delete 2 points",
                Box::new(move |d| {
                    d.delete_lane_points(clip, &[1, 3]);
                }),
            ),
            (
                "Delete point",
                Box::new(move |d| {
                    d.delete_lane_points(clip, &[1]);
                }),
            ),
            (
                "Draw line",
                Box::new(move |d| {
                    d.draw_lane_span(
                        Drag::default(),
                        clip,
                        CC7,
                        Line {
                            from: (t(0), 0),
                            to: (q(2), 100),
                            curve: &Curve::LINEAR,
                        },
                        ev::GRID,
                    );
                }),
            ),
            (
                "Draw controller",
                Box::new(move |d| {
                    d.draw_lane_points(None, clip, CC7, &[(t(0), 5), (q(1), 90)], None);
                }),
            ),
        ];
        for (label, edit) in edits {
            let before = doc.project().clone();
            edit(&mut doc);
            let after = doc.project().clone();
            assert_ne!(after, before, "{label} changed something");
            assert_eq!(doc.undo().as_deref(), Some(label), "{label} is one step");
            assert_eq!(*doc.project(), before, "{label} undoes to the exact state");
            doc.redo();
            assert_eq!(*doc.project(), after, "{label} redoes");
            doc.undo();
            assert_eq!(*doc.project(), before);
        }
        assert_eq!(*doc.project(), start);
    }

    #[test]
    fn a_pencil_drag_over_several_frames_is_one_step() {
        let (mut doc, clip) = standard();
        let base = doc.project().clone();
        let mut last = Vec::new();
        for value in [20, 50, 90] {
            last = doc.draw_lane_points(Some(&base), clip, CC7, &[(q(2), value)], Some("pencil"));
        }
        doc.seal();
        assert_eq!(last, [4]);
        assert_eq!(content(&doc).1[4], cc(2 * PPQ, 7, 90));
        assert_eq!(doc.undo().as_deref(), Some("Draw controller"));
        assert_eq!(*doc.project(), base, "the whole drag was one step");
    }

    #[test]
    fn a_velocity_drag_and_a_line_drag_are_each_one_step() {
        let (mut doc, clip) = standard();
        let base = doc.project().clone();
        for end in [50, 90, 120] {
            doc.ramp_velocities(
                Drag {
                    base: Some(&base),
                    merge: Some("ramp drag"),
                },
                clip,
                &[0, 1],
                Line {
                    from: (q(0), 10),
                    to: (q(1), end),
                    curve: &Curve::LINEAR,
                },
            );
        }
        doc.seal();
        assert_eq!(velocities(&doc), [10, 120, 120]);
        assert_eq!(doc.undo().as_deref(), Some("Ramp velocities"));
        assert_eq!(*doc.project(), base);

        for end in [t(PPQ), t(2 * PPQ), t(3 * PPQ)] {
            doc.draw_lane_span(
                Drag {
                    base: Some(&base),
                    merge: Some("line drag"),
                },
                clip,
                CC7,
                Line {
                    from: (t(0), 0),
                    to: (end, 127),
                    curve: &Curve::LINEAR,
                },
                ev::GRID,
            );
        }
        doc.seal();
        assert_eq!(doc.undo().as_deref(), Some("Draw line"));
        assert_eq!(*doc.project(), base);
    }

    #[test]
    fn a_locked_clip_refuses_every_edit_and_gives_the_selection_back() {
        let (mut doc, clip) = standard();
        doc.set_clips_locked(&[clip], true);
        let before = doc.project().clone();
        let state = doc.state();
        let base = before.clone();
        let sel = [1_usize];
        assert_eq!(doc.set_velocities_each(None, clip, &[(1, 9)], None), sel);
        assert_eq!(
            doc.ramp_velocities(
                Drag {
                    base: Some(&base),
                    merge: None
                },
                clip,
                &sel,
                Line {
                    from: (t(0), 1),
                    to: (q(1), 9),
                    curve: &Curve::LINEAR
                }
            ),
            sel
        );
        assert_eq!(doc.offset_velocities(clip, &sel, 5), sel);
        assert_eq!(
            doc.move_lane_points(
                Drag {
                    base: Some(&base),
                    merge: None
                },
                clip,
                CC7,
                &[0],
                Shift {
                    time: t(5),
                    value: 3
                }
            ),
            [0]
        );
        assert_eq!(doc.delete_lane_points(clip, &[0]), [0]);
        assert!(
            doc.draw_lane_span(
                Drag {
                    base: Some(&base),
                    merge: None
                },
                clip,
                CC7,
                Line {
                    from: (t(0), 0),
                    to: (q(1), 127),
                    curve: &Curve::LINEAR
                },
                q(1)
            )
            .is_empty()
        );
        assert!(
            doc.draw_lane_points(Some(&base), clip, CC7, &[(t(5), 9)], None)
                .is_empty()
        );
        assert_eq!(doc.state(), state);
        assert_eq!(*doc.project(), before);

        doc.set_clips_locked(&[clip], false);
        doc.set_velocities_each(None, clip, &[(1, 9)], None);
        assert_eq!(velocities(&doc), [40, 9, 120]);
    }

    #[test]
    fn an_audio_clip_and_a_missing_clip_refuse_every_edit() {
        let (mut doc, _) = standard();
        let audio = with_audio_clip(&mut doc);
        let state = doc.state();
        let sel = [0_usize];
        assert_eq!(doc.set_velocities_each(None, audio, &[(0, 9)], None), sel);
        assert_eq!(doc.offset_velocities(audio, &sel, 5), sel);
        assert_eq!(doc.delete_lane_points(audio, &sel), sel);
        assert!(
            doc.draw_lane_points(None, audio, CC7, &[(t(5), 9)], None)
                .is_empty()
        );
        assert_eq!(doc.set_velocities_each(None, Id(777), &[(0, 9)], None), sel);
        assert_eq!(doc.delete_lane_points(Id(777), &sel), sel);
        assert_eq!(doc.state(), state);
    }

    #[test]
    fn a_long_span_is_drawn_at_a_coarser_step_within_the_cap() {
        let (mut doc, clip) = doc_with(vec![], vec![]);
        // A pitch bend sweep of 15,000 ticks at a step of one tick would be
        // 15,000 events; the cap draws it every four ticks.
        doc.draw_lane_span(
            Drag::default(),
            clip,
            ev::Lane::PitchBend,
            Line {
                from: (t(0), -8192),
                to: (t(15_000), 8191),
                curve: &Curve::LINEAR,
            },
            t(1),
        );
        let events = content(&doc).1;
        let found = points(&events, ev::Lane::PitchBend);
        assert!(found.len() <= MAX_SPAN_EVENTS, "{} points", found.len());
        assert_eq!(found.last(), Some(&(15_000, 8191)), "the end lands exactly");
        assert_eq!(found[1].0 - found[0].0, 4);
    }

    #[test]
    fn the_cap_holds_for_any_span_and_a_small_span_keeps_its_step() {
        for span in [1, 2, 3_999, 4_000, 4_001, 15_000, 1_000_000, i64::MAX / 4] {
            let step = span_step(t(0), t(span));
            let ramp = ev::ramp(
                ev::Lane::PitchBend,
                (t(0), -8192),
                (t(span), 8191),
                &Curve::LINEAR,
                step.max(t(1)),
            );
            assert!(
                ramp.len() <= MAX_SPAN_EVENTS,
                "span {span}: {} points",
                ramp.len()
            );
        }
        assert_eq!(
            span_step(t(0), t(3)),
            t(1),
            "a short span keeps a tick step"
        );
        assert_eq!(span_step(t(0), t(0)), t(0));
        assert_eq!(
            span_step(t(10), t(0)),
            span_step(t(0), t(10)),
            "either order"
        );
    }

    #[test]
    fn a_span_wholly_outside_the_clip_draws_nothing() {
        let (mut doc, clip) = standard();
        let state = doc.state();
        let last = 16 * PPQ;
        let sel = doc.draw_lane_span(
            Drag::default(),
            clip,
            CC7,
            Line {
                from: (t(last + 10), 0),
                to: (t(last + 20), 127),
                curve: &Curve::LINEAR,
            },
            t(1),
        );
        assert!(sel.is_empty());
        assert_eq!(doc.state(), state);
    }

    #[test]
    fn a_span_past_the_clip_end_is_cut_at_the_end() {
        let (mut doc, clip) = doc_with(vec![], vec![]);
        let last = 16 * PPQ - 1;
        doc.draw_lane_span(
            Drag::default(),
            clip,
            CC7,
            Line {
                from: (t(last - 10), 0),
                to: (t(last + 100), 127),
                curve: &Curve::LINEAR,
            },
            t(1),
        );
        let found = points(&content(&doc).1, CC7);
        assert!(!found.is_empty());
        assert!(found.iter().all(|&(at, _)| at <= last), "{found:?}");
        assert_eq!(found.first(), Some(&(last - 10, 0)));
    }
}
