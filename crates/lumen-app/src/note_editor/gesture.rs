//! The editor's mouse gestures (M5 slice 3c), as pure arithmetic. Live A:
//! while a button is down the notes follow the pointer as a *preview*
//! drawn over the clip; the document changes once, on release, as one undo
//! step. A [`Gesture`] remembers where it began; [`Gesture::resolve`] turns
//! the pointer's place now into what the gesture means ([`Resolved`]), and
//! [`Resolved::preview`] into the notes to draw. The release commits the
//! same `Resolved` through `Document::{move_notes, copy_notes,
//! resize_notes, add_note}`, so what was shown is what is made.

use std::sync::Arc;

use eframe::egui::{Pos2, Rect};
use lumen_core::time::Ticks;
use lumen_project::edit::notes::{self as ops, End, MIN_NOTE};
use lumen_project::model::{Clip, Note};

use super::view::EditorView;
use crate::audition::Tone;

/// Points the pointer must travel before a press on a note becomes a drag.
pub(crate) const DRAG_THRESHOLD: f32 = 3.0;
/// The velocity of a drawn note.
pub(crate) const DRAW_VELOCITY: u8 = 100;

/// The tones `notes` sound as, in order: what a preview plays for them.
pub(crate) fn tones_of(notes: &[Note]) -> Vec<Tone> {
    notes
        .iter()
        .map(|n| Tone {
            pitch: n.pitch(),
            velocity: n.velocity(),
        })
        .collect()
}

/// What a gesture is doing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// Moving the picked notes; with `copy` (Ctrl) the originals stay.
    Move { copy: bool },
    /// Dragging one end of the picked notes.
    Resize { end: End },
    /// A box from the press; `base` are the picks kept when it adds.
    Marquee { base: Vec<usize> },
    /// Drawing a new note from `start` (clip time) at `pitch`. Dragged
    /// left, the note ends at `end`: the far side of the cell pressed (the
    /// press itself off the grid).
    Draw {
        start: Ticks,
        end: Ticks,
        pitch: u8,
        /// The grid step the note has at least.
        step: Ticks,
    },
}

/// A gesture in progress.
#[derive(Debug, Clone)]
pub(crate) struct Gesture {
    pub kind: Kind,
    /// Where the button went down.
    pub origin: Pos2,
    /// The clip as the gesture found it: if it changes, the gesture ends.
    pub clip: Arc<Clip>,
    /// The note grabbed (Move, Resize): the one that snaps.
    pub grabbed: usize,
    /// The picked notes the gesture acts on.
    pub picked: Vec<usize>,
    /// Whether the pointer has gone far enough to count as a drag.
    pub dragging: bool,
    /// A vertical drag on a picked hit sets its velocity instead of moving it
    /// (the drum editor's lanes, M8 D2): the diamond fills to the velocity,
    /// and the hit keeps its lane.
    pub velocity_on_vertical: bool,
}

/// What the pointer's place means now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Resolved {
    Nothing,
    Move {
        time: Ticks,
        pitch: i32,
        copy: bool,
    },
    Resize {
        end: End,
        by: Ticks,
        min: Ticks,
    },
    Draw(Note),
    Marquee(Rect),
    /// The picked notes' velocities set to one value (a vertical drag on a
    /// hit in the lanes), each with its index.
    Velocity(Vec<(usize, u8)>),
}

/// What the arithmetic needs to know about the world.
pub(crate) struct Env<'a> {
    pub view: &'a EditorView,
    /// The clip's notes as the gesture found them.
    pub notes: &'a [Note],
    /// The content's length: notes stay inside it.
    pub length: Ticks,
    /// The grid step now, the shortest a resized note gets.
    pub step: Ticks,
    /// Snaps an arrangement time to the grid (or leaves it, when the grid is
    /// off or Alt is held).
    pub snap: &'a dyn Fn(Ticks) -> Ticks,
}

impl Gesture {
    /// What the gesture means with the pointer at `pointer`.
    pub fn resolve(&self, pointer: Pos2, env: &Env<'_>) -> Resolved {
        let view = env.view;
        match &self.kind {
            Kind::Marquee { .. } => {
                Resolved::Marquee(Rect::from_two_pos(self.origin, pointer).intersect(view.body))
            }
            Kind::Draw {
                start,
                end,
                pitch,
                step,
            } => {
                let at = (env.snap)(view.absolute_at(pointer.x)) - view.clip_start;
                let min = (*step).max(MIN_NOTE);
                // Dragged right, the press is the note's start; dragged left,
                // the note keeps the pressed cell and the pointer sets the
                // start.
                let (begin, length) = if at >= *start {
                    (*start, (at - *start).max(min))
                } else {
                    let begin = at.min(*end - min).max(Ticks::ZERO);
                    (begin, *end - begin)
                };
                Resolved::Draw(Note::new(begin, length, *pitch, DRAW_VELOCITY))
            }
            _ if !self.dragging => Resolved::Nothing,
            Kind::Move { copy } => {
                let Some(note) = env.notes.get(self.grabbed) else {
                    return Resolved::Nothing;
                };
                let moved = view.local_at(pointer.x) - view.local_at(self.origin.x);
                // The lanes: a drag that starts up or down on a picked hit
                // sets the velocity of every picked hit, and moves nothing
                // (DE A: the fill is the velocity).
                if self.velocity_on_vertical && self.picked.contains(&self.grabbed) {
                    let dy = pointer.y - self.origin.y;
                    if dy.abs() > (pointer.x - self.origin.x).abs() {
                        return Resolved::Velocity(velocity_drag(
                            view,
                            self.grabbed,
                            &self.picked,
                            pointer,
                            env,
                        ));
                    }
                }
                let target = (env.snap)(view.clip_start + note.start() + moved) - view.clip_start;
                let pitch = view.pitch_at(pointer.y) - view.pitch_at(self.origin.y);
                Resolved::Move {
                    time: target - note.start(),
                    pitch,
                    copy: *copy,
                }
            }
            Kind::Resize { end } => {
                let Some(note) = env.notes.get(self.grabbed) else {
                    return Resolved::Nothing;
                };
                let edge = match end {
                    End::Start => note.start(),
                    End::End => note.end(),
                };
                let moved = view.local_at(pointer.x) - view.local_at(self.origin.x);
                let target = (env.snap)(view.clip_start + edge + moved) - view.clip_start;
                Resolved::Resize {
                    end: *end,
                    by: target - edge,
                    min: env.step.max(MIN_NOTE),
                }
            }
        }
    }
}

/// The notes a [`Resolved`] makes, to draw over the clip, and the notes it
/// hides while it moves them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Preview {
    pub ghosts: Vec<Note>,
    /// Indices of the notes shown moved: their old places are drawn faint.
    pub hidden: Vec<usize>,
    pub marquee: Option<Rect>,
    /// What the live chip says: a pitch and position, or a length.
    pub chip: Option<String>,
}

impl Resolved {
    /// The preview of this outcome for the picked notes of `env.notes`.
    pub fn preview(&self, picked: &[usize], env: &Env<'_>) -> Preview {
        let picked = ops::picked(picked, env.notes.len());
        let subset = || -> Vec<Note> { picked.iter().map(|&i| env.notes[i]).collect() };
        let everything: Vec<usize> = (0..picked.len()).collect();
        match self {
            Resolved::Nothing => Preview::default(),
            Resolved::Marquee(rect) => Preview {
                marquee: Some(*rect),
                ..Preview::default()
            },
            Resolved::Draw(note) => Preview {
                ghosts: vec![*note],
                ..Preview::default()
            },
            Resolved::Move { time, pitch, copy } => {
                let mut ghosts = subset();
                ops::move_notes(&mut ghosts, &everything, *time, *pitch, env.length);
                Preview {
                    ghosts,
                    hidden: if *copy { Vec::new() } else { picked },
                    ..Preview::default()
                }
            }
            Resolved::Resize { end, by, min } => {
                let mut ghosts = subset();
                ops::resize(&mut ghosts, &everything, *end, *by, *min, env.length);
                Preview {
                    ghosts,
                    hidden: picked,
                    ..Preview::default()
                }
            }
            Resolved::Velocity(changes) => {
                let mut ghosts = subset();
                for (slot, &index) in picked.iter().enumerate() {
                    if let Some((_, velocity)) = changes.iter().find(|(i, _)| *i == index) {
                        ghosts[slot].3 = *velocity;
                    }
                }
                Preview {
                    ghosts,
                    hidden: picked,
                    ..Preview::default()
                }
            }
        }
    }
}

/// The velocity a vertical drag at `pointer` gives the picked notes: the row
/// of the grabbed note's diamond, its foot at 0 and its top at 127 (the lanes'
/// DE A fill, followed by the hand). A drag past either end holds there.
fn velocity_drag(
    view: &EditorView,
    grabbed: usize,
    picked: &[usize],
    pointer: Pos2,
    env: &Env<'_>,
) -> Vec<(usize, u8)> {
    let Some(note) = env.notes.get(grabbed) else {
        return Vec::new();
    };
    let top = view.row_top(i32::from(note.pitch()));
    let centre = top + view.row_height / 2.0;
    let radius = crate::drum_editor::geometry::hit_radius(view, env.step).max(0.1);
    let t = ((centre + radius - pointer.y) / (2.0 * radius)).clamp(0.0, 1.0);
    let velocity = (t * 127.0).round().clamp(1.0, 127.0) as u8;
    picked.iter().map(|&index| (index, velocity)).collect()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{pos2, vec2};

    use super::*;
    use crate::timeview::TimeView;

    fn view() -> EditorView {
        EditorView {
            body: Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 400.0)),
            time: TimeView {
                left: 0.0,
                zoom: 100.0,
            },
            clip_start: Ticks::ZERO,
            row_height: 10.0,
            scroll: 0.0,
            rows: crate::piano_roll::draw::Rows::all(),
            lanes: false,
        }
    }

    fn clip() -> Arc<Clip> {
        Arc::new(Clip {
            id: lumen_project::model::Id(1),
            name: "c".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: lumen_project::model::ClipContent::midi(),
        })
    }

    fn to_quarter(t: Ticks) -> Ticks {
        let q = lumen_core::time::PPQ;
        Ticks((t.0 + q / 2).div_euclid(q) * q)
    }

    fn env<'a>(
        view: &'a EditorView,
        notes: &'a [Note],
        snap: &'a dyn Fn(Ticks) -> Ticks,
    ) -> Env<'a> {
        Env {
            view,
            notes,
            length: Ticks::quarters(8),
            step: Ticks::quarters(1),
            snap,
        }
    }

    fn gesture(kind: Kind, origin: Pos2) -> Gesture {
        Gesture {
            kind,
            origin,
            clip: clip(),
            grabbed: 0,
            picked: vec![0],
            dragging: true,
            velocity_on_vertical: false,
        }
    }

    #[test]
    fn a_move_snaps_the_grabbed_note_and_takes_the_pitch_rows() {
        let v = view();
        let notes = [Note::new(Ticks::quarters(1), Ticks::quarters(1), 60, 100)];
        let snap = to_quarter;
        let e = env(&v, &notes, &snap);
        // Grab in the note's middle; move 1.4 beats right and 2 rows up.
        let g = gesture(Kind::Move { copy: false }, pos2(150.0, 100.0));
        let r = g.resolve(pos2(150.0 + 140.0, 80.0), &e);
        assert_eq!(
            r,
            Resolved::Move {
                time: Ticks::quarters(1),
                pitch: 2,
                copy: false
            }
        );
        let p = r.preview(&[0], &e);
        assert_eq!(p.ghosts[0].start(), Ticks::quarters(2));
        assert_eq!(p.ghosts[0].pitch(), 62);
        assert_eq!(p.hidden, vec![0]);
    }

    #[test]
    fn a_press_that_has_not_dragged_does_nothing() {
        let v = view();
        let notes = [Note::new(Ticks::quarters(1), Ticks::quarters(1), 60, 100)];
        let snap = to_quarter;
        let e = env(&v, &notes, &snap);
        let mut g = gesture(Kind::Move { copy: false }, pos2(150.0, 100.0));
        g.dragging = false;
        assert_eq!(g.resolve(pos2(300.0, 100.0), &e), Resolved::Nothing);
    }

    #[test]
    fn a_copy_leaves_the_originals_showing() {
        let v = view();
        let notes = [Note::new(Ticks::quarters(1), Ticks::quarters(1), 60, 100)];
        let snap = to_quarter;
        let e = env(&v, &notes, &snap);
        let g = gesture(Kind::Move { copy: true }, pos2(150.0, 100.0));
        let r = g.resolve(pos2(350.0, 100.0), &e);
        assert!(r.preview(&[0], &e).hidden.is_empty());
    }

    #[test]
    fn a_resize_moves_the_end_by_the_snapped_amount_and_keeps_the_minimum() {
        let v = view();
        let notes = [Note::new(Ticks::quarters(1), Ticks::quarters(2), 60, 100)];
        let snap = to_quarter;
        let e = env(&v, &notes, &snap);
        // The end is at beat 3 (x=300); drag it to beat 4.2.
        let g = gesture(Kind::Resize { end: End::End }, pos2(299.0, 100.0));
        let r = g.resolve(pos2(299.0 + 120.0, 100.0), &e);
        let Resolved::Resize { by, min, end } = r.clone() else {
            panic!("{r:?}");
        };
        assert_eq!((by, end), (Ticks::quarters(1), End::End));
        assert_eq!(min, Ticks::quarters(1));
        assert_eq!(r.preview(&[0], &e).ghosts[0].length(), Ticks::quarters(3));
    }

    #[test]
    fn drawing_makes_a_note_of_a_step_and_a_drag_lengthens_it() {
        let v = view();
        let snap = to_quarter;
        let e = env(&v, &[], &snap);
        let g = gesture(
            Kind::Draw {
                start: Ticks::quarters(2),
                end: Ticks::quarters(3),
                pitch: 64,
                step: Ticks::quarters(1),
            },
            pos2(210.0, 100.0),
        );
        let Resolved::Draw(n) = g.resolve(pos2(212.0, 100.0), &e) else {
            panic!()
        };
        assert_eq!(
            (n.start(), n.length(), n.pitch()),
            (Ticks::quarters(2), Ticks::quarters(1), 64)
        );
        let Resolved::Draw(n) = g.resolve(pos2(510.0, 100.0), &e) else {
            panic!()
        };
        assert_eq!(n.length(), Ticks::quarters(3));
    }

    #[test]
    fn drawing_right_to_left_keeps_the_cell_pressed() {
        let v = view();
        let snap = to_quarter;
        let e = env(&v, &[], &snap);
        // Pressed in the cell from beat 5 to 6; dragged left to beat 2.2,
        // which snaps to 2: the note keeps the pressed cell, 2 to 6.
        let g = gesture(
            Kind::Draw {
                start: Ticks::quarters(5),
                end: Ticks::quarters(6),
                pitch: 64,
                step: Ticks::quarters(1),
            },
            pos2(510.0, 100.0),
        );
        let Resolved::Draw(n) = g.resolve(pos2(222.0, 100.0), &e) else {
            panic!()
        };
        assert_eq!(
            (n.start(), n.end(), n.pitch()),
            (Ticks::quarters(2), Ticks::quarters(6), 64)
        );
        // One step left of the press adds a cell, still ending there.
        let Resolved::Draw(n) = g.resolve(pos2(440.0, 100.0), &e) else {
            panic!()
        };
        assert_eq!(
            (n.start(), n.end()),
            (Ticks::quarters(4), Ticks::quarters(6))
        );
        // Off the grid (Alt), the minimum length still holds.
        let free = |t: Ticks| t;
        let e = env(&v, &[], &free);
        let g = gesture(
            Kind::Draw {
                start: Ticks::quarters(5),
                end: Ticks::quarters(5),
                pitch: 64,
                step: Ticks::quarters(1),
            },
            pos2(510.0, 100.0),
        );
        let Resolved::Draw(n) = g.resolve(pos2(495.0, 100.0), &e) else {
            panic!()
        };
        assert_eq!(
            (n.start(), n.end()),
            (Ticks::quarters(4), Ticks::quarters(5))
        );
        // Dragged past the start of the clip, the note stops at zero.
        let at_start = gesture(
            Kind::Draw {
                start: Ticks::quarters(1),
                end: Ticks::quarters(2),
                pitch: 64,
                step: Ticks::quarters(1),
            },
            pos2(110.0, 100.0),
        );
        let Resolved::Draw(n) = at_start.resolve(pos2(0.0, 100.0), &env(&v, &[], &snap)) else {
            panic!()
        };
        assert_eq!((n.start(), n.end()), (Ticks::ZERO, Ticks::quarters(2)));
    }

    #[test]
    fn a_marquee_is_the_box_inside_the_body() {
        let v = view();
        let snap = to_quarter;
        let e = env(&v, &[], &snap);
        let g = gesture(Kind::Marquee { base: vec![] }, pos2(100.0, 100.0));
        let Resolved::Marquee(r) = g.resolve(pos2(900.0, 50.0), &e) else {
            panic!()
        };
        assert_eq!(r, Rect::from_min_max(pos2(100.0, 50.0), pos2(800.0, 100.0)));
    }

    #[test]
    fn a_lane_view_resolves_the_same_drag_as_the_roll_would() {
        // The same pixels, the same time: the lanes change only which row a
        // pitch is (E5: one gesture code), and a vertical drag there is the
        // hit's velocity rather than a move.
        let mut lane = view();
        lane.lanes = true;
        lane.row_height = crate::drum_editor::geometry::ROW;
        lane.rows = crate::piano_roll::draw::Rows::lanes([36_u8, 38, 42]);
        let notes = [Note::new(Ticks::quarters(1), Ticks(1), 36, 100)];
        let snap = to_quarter;
        let lane_env = env(&lane, &notes, &snap);
        let plain = view();
        let roll_env = env(&plain, &notes, &snap);
        let origin = pos2(200.0, lane.row_height / 2.0);
        let mut next = 0x1234_5678_u64;
        for _ in 0..24 {
            next ^= next << 13;
            next ^= next >> 7;
            next ^= next << 17;
            let dx = (next % 400) as f32 - 100.0;
            let mut moved = gesture(Kind::Move { copy: false }, origin);
            moved.velocity_on_vertical = true;
            let rolled = gesture(Kind::Move { copy: false }, origin);
            // Sideways: both move the same time, in the same row.
            let lane_res = moved.resolve(origin + vec2(dx, 0.0), &lane_env);
            let roll_res = rolled.resolve(origin + vec2(dx, 0.0), &roll_env);
            let (
                Resolved::Move {
                    time: lt,
                    pitch: lp,
                    ..
                },
                Resolved::Move { time: rt, .. },
            ) = (lane_res, roll_res)
            else {
                panic!("sideways is a move in both")
            };
            assert_eq!(lt, rt, "the same pixels move the same time");
            assert_eq!(lp, 0, "a sideways drag stays in its lane");
            // Up or down: the lanes set the velocity, the roll moves a pitch.
            let lane_res = moved.resolve(origin + vec2(0.0, -12.0), &lane_env);
            assert!(matches!(lane_res, Resolved::Velocity(_)), "{lane_res:?}");
            let roll_res = rolled.resolve(origin + vec2(0.0, -12.0), &roll_env);
            assert!(
                matches!(roll_res, Resolved::Move { pitch, .. } if pitch > 0),
                "{roll_res:?}"
            );
        }
    }

    #[test]
    fn in_the_lanes_a_vertical_drag_sets_the_velocity_and_keeps_the_lane() {
        // A drum lane: rows are 28 pt, the hit sits in the first.
        let mut v = view();
        v.lanes = true;
        v.row_height = crate::drum_editor::geometry::ROW;
        v.rows = crate::piano_roll::draw::Rows::lanes([36_u8]);
        let notes = [Note::new(Ticks::quarters(1), Ticks(1), 36, 100)];
        let snap = to_quarter;
        let e = env(&v, &notes, &snap);
        let mut g = gesture(Kind::Move { copy: false }, pos2(150.0, v.row_height / 2.0));
        g.velocity_on_vertical = true;
        // Straight up from the middle: full velocity, no move, no lane.
        let up = g.resolve(pos2(150.0, 2.0), &e);
        assert_eq!(up, Resolved::Velocity(vec![(0, 127)]));
        let p = up.preview(&[0], &e);
        assert_eq!(p.ghosts[0].velocity(), 127, "the diamond fills to it");
        assert_eq!(p.hidden, vec![0]);
        // Down to the diamond's foot: the least velocity.
        let down = g.resolve(pos2(150.0, 28.0), &e);
        assert_eq!(down, Resolved::Velocity(vec![(0, 1)]));
        // Sideways is a move: the lane can still change.
        let side = g.resolve(pos2(400.0, v.row_height / 2.0), &e);
        assert!(matches!(side, Resolved::Move { .. }), "{side:?}");
        // The piano roll never takes the vertical drag as velocity.
        let mut roll = gesture(Kind::Move { copy: false }, pos2(150.0, v.row_height / 2.0));
        roll.velocity_on_vertical = false;
        assert!(matches!(
            roll.resolve(pos2(150.0, 2.0), &e),
            Resolved::Move { .. }
        ));
    }
}
