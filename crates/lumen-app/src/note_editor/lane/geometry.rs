//! Where lane values and points are on the screen (M5 slice 4b): the
//! velocity and controller lane's geometry as pure arithmetic, with no
//! drawing. The lane shares the piano roll's time axis ([`TimeView`]), so a
//! point sits at the same x as the notes above it. Values run vertically:
//! the top of the plotting area is the range's maximum, and the area is
//! inset by [`INSET`] at both ends so a value at either limit stays clear
//! of the edge.

use std::ops::Range;

use eframe::egui::{Pos2, Rect, pos2};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::edit::events::Lane;
use lumen_project::model::{ClipEvent, Note};

use crate::timeview::TimeView;

/// How near (points) a stem or a point must be to be hit.
pub(crate) const HIT: f32 = 8.0;
/// The radius (points) of a velocity head; a pointer this close covers it.
pub(crate) const HEAD_RADIUS: f32 = 4.0;
/// Points kept clear of the plotting area's top and bottom edges.
pub(crate) const INSET: f32 = 8.0;
/// Points beyond the body's sides that still count as visible.
const VISIBLE_MARGIN: f32 = 8.0;
/// The velocity lane's values: a note is never silent here (0 is a
/// note-off in MIDI), so the lane starts at 1.
const VELOCITY: (i32, i32) = (1, 127);

/// What the lane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LaneKind {
    /// The notes' velocities, one stem per start time.
    #[default]
    Velocity,
    /// A controller by number (0 to 127).
    Controller(u8),
    PitchBend,
    Pressure,
}

impl LaneKind {
    /// The lane's name as its header shows it.
    pub fn name(self) -> String {
        match self {
            LaneKind::Velocity => "Velocity".to_owned(),
            LaneKind::Controller(1) => "Mod wheel".to_owned(),
            LaneKind::Controller(7) => "Volume".to_owned(),
            LaneKind::Controller(10) => "Pan".to_owned(),
            LaneKind::Controller(11) => "Expression".to_owned(),
            LaneKind::Controller(64) => "Sustain".to_owned(),
            LaneKind::Controller(n) => format!("CC {n}"),
            LaneKind::PitchBend => "Pitch bend".to_owned(),
            LaneKind::Pressure => "Pressure".to_owned(),
        }
    }

    /// The lane an event belongs to, `None` for the velocity lane (whose
    /// values are the notes', not events').
    pub fn lane(self) -> Option<Lane> {
        match self {
            LaneKind::Velocity => None,
            LaneKind::Controller(n) => Some(Lane::Controller(n)),
            LaneKind::PitchBend => Some(Lane::PitchBend),
            LaneKind::Pressure => Some(Lane::Pressure),
        }
    }

    /// The values the lane takes, both ends included.
    pub fn range(self) -> (i32, i32) {
        match self.lane() {
            Some(lane) => lane.range(),
            None => VELOCITY,
        }
    }

    /// The baseline stems and fills start from: 0 for pitch bend (its
    /// centre), the bottom of the range for everything else.
    pub fn rest(self) -> i32 {
        match self {
            LaneKind::PitchBend => 0,
            other => other.range().0,
        }
    }
}

/// The lane's place on the screen: its plotting area, the shared time axis
/// and what the lane shows. `clip_start` is where the open clip starts on the
/// arrangement, as in the editor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneView {
    /// The area the lane is drawn in (right of the key column, below the
    /// notes' ruler).
    pub body: Rect,
    /// The editor's zoom and scroll (the same view the notes use).
    pub time: TimeView,
    /// Where the open clip starts on the arrangement.
    pub clip_start: Ticks,
    /// How long the clip's window is in clip time: events outside it are
    /// kept but hidden, and are not picked (format v15).
    pub window: Ticks,
    /// What the lane shows.
    pub kind: LaneKind,
}

impl LaneView {
    /// The x of clip time `local`.
    pub fn x_of(&self, local: Ticks) -> f32 {
        self.time
            .x(self.body.left(), (self.clip_start + local).as_quarters())
    }

    /// The clip time at `x`, unsnapped (it may be before the clip: negative).
    pub fn local_at(&self, x: f32) -> Ticks {
        let beat = self.time.beat_at(self.body.left(), x);
        Ticks((beat * PPQ as f64).round() as i64) - self.clip_start
    }

    /// The y of `value` (not clamped: a value outside the range lies outside
    /// the plotting area).
    pub fn y_of(&self, value: i32) -> f32 {
        let (low, high) = self.kind.range();
        let frac = f64::from(high - value) / f64::from((high - low).max(1));
        (f64::from(self.top()) + frac * self.span()) as f32
    }

    /// The value at `y`, rounded and clamped into the range.
    pub fn value_at(&self, y: f32) -> i32 {
        let (low, high) = self.kind.range();
        let value = self.value_exact(y).round() as i64;
        value.clamp(i64::from(low), i64::from(high)) as i32
    }

    /// The value at `y` before rounding and clamping: the lane's own units,
    /// as a fraction is taken from (a curve's handle needs the exact value).
    pub fn value_exact(&self, y: f32) -> f64 {
        let (low, high) = self.kind.range();
        let frac = (f64::from(y) - f64::from(self.top())) / self.span();
        f64::from(high) - frac * f64::from(high - low)
    }

    /// The note whose velocity head a press at `p` grabs in the velocity
    /// lane: among the notes of the stems within [`HIT`] points of `p` in x,
    /// those whose own head lies within [`HIT`] points of `p`. A picked note
    /// whose head covers the pointer (within [`HEAD_RADIUS`]) comes first,
    /// then the nearest head; a tie goes to the higher note index, which is
    /// the head drawn on top.
    pub fn head_at(
        &self,
        notes: &[Note],
        stems: &[(Ticks, Vec<usize>)],
        picked: &[usize],
        p: Pos2,
    ) -> Option<usize> {
        let near = self.stems_between(stems, p.x - HIT, p.x + HIT);
        stems[near]
            .iter()
            .flat_map(|(at, members)| {
                let x = self.x_of(*at);
                members.iter().filter_map(move |&i| {
                    let note = notes.get(i)?;
                    let head = pos2(x, self.y_of(i32::from(note.velocity())));
                    let distance = head.distance(p);
                    (distance <= HIT).then_some((i, distance))
                })
            })
            .max_by(|a, b| {
                let covers =
                    |&(i, distance): &(usize, f32)| distance <= HEAD_RADIUS && picked.contains(&i);
                covers(a)
                    .cmp(&covers(b))
                    .then(b.1.total_cmp(&a.1))
                    .then(a.0.cmp(&b.0))
            })
            .map(|(i, _)| i)
    }

    /// The index into `events` of the lane's event nearest `p`, if one lies
    /// within [`HIT`] points (in screen distance). Only events of this
    /// lane count; the velocity lane has none, so it answers `None`. A tie
    /// goes to the earlier event.
    pub fn point_at(&self, events: &[ClipEvent], p: Pos2) -> Option<usize> {
        let lane = self.kind.lane()?;
        events
            .iter()
            .enumerate()
            .filter(|(_, event)| self.in_window(event.at))
            .filter_map(|(i, event)| {
                let value = lane.value_of(event.kind)?;
                let distance = self.position(event.at, value).distance(p);
                (distance <= HIT).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The indices of the lane's events whose screen position lies inside
    /// `rect`, ascending.
    pub fn points_in(&self, events: &[ClipEvent], rect: Rect) -> Vec<usize> {
        let Some(lane) = self.kind.lane() else {
            return Vec::new();
        };
        events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                self.in_window(event.at)
                    && lane
                        .value_of(event.kind)
                        .is_some_and(|value| rect.contains(self.position(event.at, value)))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether clip time `local` is in the clip's window (see `window`).
    pub fn in_window(&self, local: Ticks) -> bool {
        local >= Ticks::ZERO && local < self.window
    }

    /// Whether clip time `local` could show: inside the body's x range with
    /// a small margin either side.
    pub fn visible(&self, local: Ticks) -> bool {
        let x = self.x_of(local);
        (self.body.left() - VISIBLE_MARGIN..=self.body.right() + VISIBLE_MARGIN).contains(&x)
    }

    /// The indices of the stems whose x lies in `lo..=hi` (`stems` in time
    /// order, as `velocity::stems` gives them). An empty range when `lo > hi`.
    pub(crate) fn stems_between(
        &self,
        stems: &[(Ticks, Vec<usize>)],
        lo: f32,
        hi: f32,
    ) -> Range<usize> {
        let start = stems.partition_point(|(at, _)| self.x_of(*at) < lo);
        let end = stems.partition_point(|(at, _)| self.x_of(*at) <= hi);
        start..end.max(start)
    }

    /// The top of the plotting area.
    fn top(&self) -> f32 {
        self.body.top() + INSET
    }

    /// The height of the plotting area (at least one point, so a tiny body
    /// still maps values consistently).
    fn span(&self) -> f64 {
        (f64::from(self.body.height()) - 2.0 * f64::from(INSET)).max(1.0)
    }

    /// The screen position of an event at `at` with `value`.
    fn position(&self, at: Ticks, value: i32) -> Pos2 {
        pos2(self.x_of(at), self.y_of(value))
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::edit::velocity::stems as stems_of;
    use lumen_project::model::{EventKind, Note};

    use super::*;

    fn fixture(kind: LaneKind) -> LaneView {
        LaneView {
            body: Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 250.0)),
            time: TimeView {
                left: 0.0,
                zoom: 40.0,
            },
            clip_start: Ticks::ZERO,
            window: Ticks::quarters(64),
            kind,
        }
    }

    /// `beats` quarter notes as clip time.
    fn beats(beats: f64) -> Ticks {
        Ticks((beats * PPQ as f64) as i64)
    }

    fn cc(at: Ticks, number: u8, value: u8) -> ClipEvent {
        ClipEvent {
            at,
            kind: EventKind::Controller { number, value },
        }
    }

    fn pressure(at: Ticks, value: u8) -> ClipEvent {
        ClipEvent {
            at,
            kind: EventKind::Pressure { value },
        }
    }

    /// Stems at beats 0, 1, 2 and 3 (x 100, 140, 180, 220 in [view]).
    fn four_stems() -> Vec<(Ticks, Vec<usize>)> {
        let notes: Vec<Note> = (0..4)
            .map(|b| Note::new(beats(f64::from(b)), beats(1.0), 60, 64))
            .collect();
        stems_of(&notes)
    }

    #[test]
    fn x_and_local_time_are_inverse_at_several_zooms_and_scrolls() {
        for zoom in [10.0, 40.0, 333.0, 2000.0] {
            for left in [0.0, 3.5, 100.25] {
                for clip_start in [Ticks::ZERO, Ticks::quarters(4)] {
                    let view = LaneView {
                        time: TimeView { left, zoom },
                        clip_start,
                        ..fixture(LaneKind::Velocity)
                    };
                    // One point is PPQ / zoom ticks; allow a couple of points
                    // of rounding, plus a tick.
                    let tolerance = 2.0 * PPQ as f64 / zoom + 1.0;
                    for t in [0.0, 0.5, 3.25, 7.75] {
                        let at = beats(t);
                        let back = view.local_at(view.x_of(at));
                        let err = (back - at).0.abs() as f64;
                        assert!(
                            err <= tolerance,
                            "zoom {zoom} left {left} start {clip_start:?}: {at:?} came back {back:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn x_follows_the_clip_start() {
        let view = LaneView {
            clip_start: Ticks::quarters(4),
            ..fixture(LaneKind::Velocity)
        };
        // Clip time 0 is beat 4 on the timeline: x = 100 + 4 * 40.
        assert_eq!(view.x_of(Ticks::ZERO), 260.0);
        assert_eq!(view.local_at(260.0), Ticks::ZERO);
    }

    #[test]
    fn value_and_y_are_inverse_for_every_lane() {
        let kinds = [
            LaneKind::Velocity,
            LaneKind::Controller(7),
            LaneKind::PitchBend,
            LaneKind::Pressure,
        ];
        for kind in kinds {
            let view = fixture(kind);
            let (low, high) = kind.range();
            for value in [low, low + 1, (low + high) / 2, high - 1, high] {
                assert_eq!(
                    view.value_at(view.y_of(value)),
                    value,
                    "{kind:?} at {value}"
                );
            }
        }
    }

    #[test]
    fn the_top_is_the_maximum_and_the_inset_keeps_the_limits_off_the_edges() {
        let view = fixture(LaneKind::Controller(7));
        assert_eq!(view.y_of(127), view.body.top() + INSET);
        assert_eq!(view.y_of(0), view.body.bottom() - INSET);
        let pitch = fixture(LaneKind::PitchBend);
        assert_eq!(pitch.y_of(8191), pitch.body.top() + INSET);
        assert_eq!(pitch.y_of(-8192), pitch.body.bottom() - INSET);
    }

    #[test]
    fn values_clamp_into_the_range() {
        let view = fixture(LaneKind::Velocity);
        assert_eq!(view.value_at(-1_000_000.0), 127);
        assert_eq!(view.value_at(1_000_000.0), 1);
        assert_eq!(view.value_at(view.body.top()), 127);
        assert_eq!(view.value_at(view.body.bottom()), 1);
    }

    #[test]
    fn value_at_rounds_to_the_nearest_value() {
        let view = fixture(LaneKind::Controller(7));
        // The plotting area is 184 points for 127 steps.
        let per_value = 184.0_f32 / 127.0;
        let y = view.y_of(60);
        // 0.4 of a value above 60 rounds back to 60; 0.6 rounds to 61.
        assert_eq!(view.value_at(y - 0.4 * per_value), 60);
        assert_eq!(view.value_at(y - 0.6 * per_value), 61);
        assert_eq!(view.value_at(y + 0.6 * per_value), 59);
    }

    #[test]
    fn names_ranges_and_rest_values() {
        assert_eq!(LaneKind::Velocity.name(), "Velocity");
        assert_eq!(LaneKind::Controller(1).name(), "Mod wheel");
        assert_eq!(LaneKind::Controller(7).name(), "Volume");
        assert_eq!(LaneKind::Controller(10).name(), "Pan");
        assert_eq!(LaneKind::Controller(11).name(), "Expression");
        assert_eq!(LaneKind::Controller(64).name(), "Sustain");
        assert_eq!(LaneKind::Controller(74).name(), "CC 74");
        assert_eq!(LaneKind::PitchBend.name(), "Pitch bend");
        assert_eq!(LaneKind::Pressure.name(), "Pressure");

        assert_eq!(LaneKind::Velocity.range(), (1, 127));
        assert_eq!(LaneKind::Controller(7).range(), (0, 127));
        assert_eq!(LaneKind::PitchBend.range(), (-8192, 8191));
        assert_eq!(LaneKind::Pressure.range(), (0, 127));

        assert_eq!(LaneKind::Velocity.rest(), 1);
        assert_eq!(LaneKind::Controller(7).rest(), 0);
        assert_eq!(LaneKind::PitchBend.rest(), 0);
        assert_eq!(LaneKind::Pressure.rest(), 0);
    }

    #[test]
    fn the_velocity_lane_has_no_event_lane() {
        assert_eq!(LaneKind::Velocity.lane(), None);
        assert_eq!(LaneKind::Controller(7).lane(), Some(Lane::Controller(7)));
        assert_eq!(LaneKind::PitchBend.lane(), Some(Lane::PitchBend));
        assert_eq!(LaneKind::Pressure.lane(), Some(Lane::Pressure));
    }

    /// Where the lane draws `notes[i]`'s head.
    fn head(view: &LaneView, notes: &[Note], i: usize) -> Pos2 {
        pos2(
            view.x_of(notes[i].start()),
            view.y_of(i32::from(notes[i].velocity())),
        )
    }

    #[test]
    fn each_note_of_a_chord_has_its_own_head_to_grab() {
        let view = fixture(LaneKind::Velocity);
        let notes = [
            Note::new(beats(1.0), beats(1.0), 60, 100),
            Note::new(beats(1.0), beats(1.0), 64, 60),
        ];
        let stems = stems_of(&notes);
        assert_eq!(
            view.head_at(&notes, &stems, &[], head(&view, &notes, 0)),
            Some(0)
        );
        assert_eq!(
            view.head_at(&notes, &stems, &[], head(&view, &notes, 1)),
            Some(1)
        );
    }

    #[test]
    fn equal_heads_on_top_of_each_other_give_the_higher_index() {
        let view = fixture(LaneKind::Velocity);
        let notes = [
            Note::new(beats(1.0), beats(1.0), 60, 90),
            Note::new(beats(1.0), beats(1.0), 64, 90),
        ];
        let stems = stems_of(&notes);
        let p = head(&view, &notes, 0);
        assert_eq!(view.head_at(&notes, &stems, &[], p), Some(1));
    }

    #[test]
    fn a_picked_note_whose_head_covers_the_pointer_beats_a_nearer_one() {
        let view = fixture(LaneKind::Velocity);
        // Heads 6 points apart (127 steps over 184 points: about 4 values).
        let notes = [
            Note::new(beats(1.0), beats(1.0), 60, 90),
            Note::new(beats(1.0), beats(1.0), 64, 86),
        ];
        let stems = stems_of(&notes);
        let (upper, lower) = (head(&view, &notes, 0), head(&view, &notes, 1));
        let apart = lower.y - upper.y;
        assert!(apart > HEAD_RADIUS && apart < 2.0 * HEAD_RADIUS);
        // 3.5 points below the upper head: inside its radius, nearer the lower.
        let p = pos2(upper.x, upper.y + 3.5);
        assert!(p.distance(lower) < p.distance(upper));
        assert_eq!(view.head_at(&notes, &stems, &[0], p), Some(0));
        assert_eq!(
            view.head_at(&notes, &stems, &[], p),
            Some(1),
            "unpicked: nearest"
        );
        assert_eq!(view.head_at(&notes, &stems, &[1], p), Some(1));
    }

    #[test]
    fn the_nearest_head_wins_and_none_is_hit_beyond_hit() {
        let view = fixture(LaneKind::Velocity);
        let notes = [
            Note::new(beats(1.0), beats(1.0), 60, 120),
            Note::new(beats(1.0), beats(1.0), 64, 40),
        ];
        let stems = stems_of(&notes);
        let (top, bottom) = (head(&view, &notes, 0), head(&view, &notes, 1));
        assert_eq!(
            view.head_at(&notes, &stems, &[], pos2(top.x + 3.0, top.y + 2.0)),
            Some(0)
        );
        assert_eq!(
            view.head_at(&notes, &stems, &[], pos2(bottom.x - 3.0, bottom.y)),
            Some(1)
        );
        // Between the heads, far from both.
        let middle = pos2(top.x, (top.y + bottom.y) / 2.0);
        assert_eq!(view.head_at(&notes, &stems, &[], middle), None);
        // On the stem, in x, but beyond HIT of the head in y.
        assert_eq!(
            view.head_at(&notes, &stems, &[], pos2(top.x, top.y + HIT + 0.5)),
            None
        );
        // Beside the stem beyond HIT in x.
        assert_eq!(
            view.head_at(&notes, &stems, &[], pos2(top.x + HIT + 0.5, top.y)),
            None
        );
        assert_eq!(view.head_at(&[], &[], &[], top), None);
    }

    #[test]
    fn heads_of_nearby_stems_are_told_apart_by_their_own_position() {
        let view = fixture(LaneKind::Velocity);
        // 0.1 beat is 4 points apart: both stems are within HIT of the press.
        let notes = [
            Note::new(beats(1.0), beats(1.0), 60, 100),
            Note::new(beats(1.1), beats(1.0), 64, 50),
        ];
        let stems = stems_of(&notes);
        assert_eq!(
            view.head_at(&notes, &stems, &[], head(&view, &notes, 0)),
            Some(0)
        );
        assert_eq!(
            view.head_at(&notes, &stems, &[], head(&view, &notes, 1)),
            Some(1)
        );
    }

    #[test]
    fn a_point_is_hit_only_within_hit_points_and_only_in_its_lane() {
        let view = fixture(LaneKind::Controller(7));
        let at = beats(1.0);
        let y = view.y_of(64);
        let events = [
            cc(at, 1, 64),
            cc(at, 7, 64),
            pressure(at, 64),
            cc(at, 7, 100),
        ];
        assert_eq!(view.point_at(&events, pos2(148.0, y)), Some(1), "at HIT");
        assert_eq!(view.point_at(&events, pos2(148.5, y)), None, "beyond HIT");
        // Controller 1 and the pressure point share the spot but are not
        // this lane's, so they never answer. Index 3 is this lane but far off.
        assert_eq!(view.point_at(&events, pos2(140.0, y)), Some(1));
        assert_eq!(view.point_at(&events, pos2(140.0, view.y_of(100))), Some(3));
    }

    #[test]
    fn a_point_hit_measures_screen_distance() {
        let view = fixture(LaneKind::Controller(7));
        let events = [cc(beats(1.0), 7, 64)];
        // Eight points away in x and in y together is more than HIT.
        let y = view.y_of(64);
        assert_eq!(view.point_at(&events, pos2(148.0, y + 8.0)), None);
        assert_eq!(view.point_at(&events, pos2(144.0, y + 6.0)), Some(0));
    }

    #[test]
    fn the_velocity_lane_hits_no_points() {
        let view = fixture(LaneKind::Velocity);
        let events = [cc(beats(1.0), 7, 64)];
        assert_eq!(view.point_at(&events, pos2(140.0, view.y_of(64))), None);
        assert!(view.points_in(&events, view.body).is_empty());
    }

    #[test]
    fn points_in_takes_the_lanes_points_inside_the_rect_ascending() {
        let view = fixture(LaneKind::Controller(7));
        let events = [
            cc(beats(0.5), 7, 10),
            cc(beats(1.0), 7, 60),
            pressure(beats(1.0), 60),
            cc(beats(1.125), 7, 55),
            cc(beats(2.0), 7, 120),
        ];
        let rect = Rect::from_min_max(pos2(110.0, view.y_of(70)), pos2(150.0, view.y_of(50)));
        assert_eq!(view.points_in(&events, rect), [1, 3]);
        assert!(view.points_in(&[], rect).is_empty());
    }

    #[test]
    fn visible_has_a_margin_beside_the_body() {
        let view = fixture(LaneKind::Velocity);
        assert!(view.visible(Ticks::ZERO), "x 100 is the body's left edge");
        assert!(
            view.visible(beats(-0.15)),
            "x 94: inside the 8 point margin"
        );
        assert!(!view.visible(beats(-1.0)), "x 60 is well outside");
        assert!(view.visible(beats(10.0)), "x 500 is the body's right edge");
        assert!(!view.visible(beats(12.0)), "x 580 is well outside");
    }

    #[test]
    fn stems_between_takes_the_stems_inside_the_range() {
        let view = fixture(LaneKind::Velocity);
        let stems = four_stems();
        assert_eq!(view.stems_between(&stems, 92.0, 148.0), 0..2);
        assert_eq!(view.stems_between(&stems, 150.0, 170.0), 2..2);
        assert_eq!(view.stems_between(&stems, 300.0, 100.0), 4..4);
    }
}
