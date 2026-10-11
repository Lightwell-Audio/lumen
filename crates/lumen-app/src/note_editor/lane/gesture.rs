//! The lane's mouse gestures (M5 slice 4b) as pure arithmetic, in the style
//! of `note_editor::gesture`: a [`LaneGesture`] remembers where its button
//! went down, and [`LaneGesture::resolve`] turns the pointer's place now into
//! what the gesture means ([`Resolved`]). The release commits that same
//! `Resolved` through the `Document` lane methods, so what was drawn is what
//! is made. Nothing here draws or touches the document.
//!
//! Times are clip time. Values are the lane's own units: velocities in the
//! velocity lane, controller values, pitch-bend and pressure elsewhere. A
//! stroke over the velocity lane changes notes; over any other lane it
//! changes events.

use std::borrow::Cow;
use std::collections::BTreeMap;

use eframe::egui::{Pos2, Rect};
use lumen_core::curve::{Curve, DOT_MAX, DOT_MIN, Handle};
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{ClipEvent, Note};

use super::geometry::{HIT, LaneKind, LaneView};

/// Points the pointer must travel from the press before it counts as a drag.
pub(crate) const DRAG_THRESHOLD: f32 = 3.0;

/// The tool the lane is used with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LaneTool {
    #[default]
    Pencil,
    Line,
    Curve,
}

/// What a gesture reads: the lane's view, the clip's notes and events as the
/// gesture found them, the pencil's grid step and the clip's length.
pub(crate) struct Env<'a> {
    pub view: &'a LaneView,
    /// The clip's notes (their starts are clip time).
    pub notes: &'a [Note],
    /// The clip's events, all lanes.
    pub events: &'a [ClipEvent],
    /// The pencil's grid step in ticks; 0 when snapping is off.
    pub step: Ticks,
    /// Snaps clip time to the grid: the identity when snapping is off.
    pub snap: &'a dyn Fn(Ticks) -> Ticks,
    /// The clip's content length: points stay inside `[0, length)`.
    pub length: Ticks,
    /// The notes grouped by start (`velocity::stems`). The caller keeps it
    /// until the notes change, so a stroke does not regroup 100,000 notes at
    /// every pointer event.
    pub stems: std::borrow::Cow<'a, [(Ticks, Vec<usize>)]>,
}

/// What a gesture is doing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// Drawing freehand: the points or velocities along the stroke.
    Pencil,
    /// A line or curve from the press to the pointer.
    Line,
    /// Dragging the picked points of the lane; `grabbed` is the point under
    /// the press (an index into the clip's events), and the one that snaps.
    Move { grabbed: usize, picked: Vec<usize> },
    /// A box from the press; `base` are the picks kept when it adds (Shift).
    Box { base: Vec<usize> },
    /// Dragging the handle dot of a line just drawn from `from` to `to`, to
    /// shape its curve.
    Bend {
        from: (Ticks, i32),
        to: (Ticks, i32),
    },
}

/// One pointer gesture on the lane.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LaneGesture {
    pub kind: Kind,
    /// Where the button went down.
    pub origin: Pos2,
    /// Whether the pointer has gone far enough to count as a drag.
    pub dragging: bool,
    /// Every pointer position seen, the press first: the stroke of the
    /// pencil.
    pub trail: Vec<Pos2>,
}

/// What the pointer's place means now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Resolved {
    /// Nothing to change.
    Nothing,
    /// Velocities to set: `(note index, velocity)`, sorted by note index.
    Velocities(Vec<(usize, u8)>),
    /// Points to draw: `(clip time, value)`, sorted by time, one per tick.
    Points(Vec<(Ticks, i32)>),
    /// A line or curve between two ends, in the lane's units.
    Span {
        from: (Ticks, i32),
        to: (Ticks, i32),
        curve: Curve,
    },
    /// A velocity line: `notes` are the notes whose start lies between the
    /// two ends.
    Ramp {
        from: (Ticks, u8),
        to: (Ticks, u8),
        notes: Vec<usize>,
        curve: Curve,
    },
    /// Moves the picked points by `time` ticks and `value`.
    Shift { time: Ticks, value: i32 },
    /// A box in screen points.
    Box(Rect),
}

impl LaneGesture {
    /// A gesture pressed at `origin`, not yet a drag.
    pub fn new(kind: Kind, origin: Pos2) -> Self {
        Self {
            kind,
            origin,
            dragging: false,
            trail: vec![origin],
        }
    }

    /// Records the pointer's place. It becomes a drag once it is more than
    /// [`DRAG_THRESHOLD`] points from the press, and stays one.
    pub fn feed(&mut self, pointer: Pos2) {
        if self.trail.last() != Some(&pointer) {
            self.trail.push(pointer);
        }
        if pointer.distance(self.origin) > DRAG_THRESHOLD {
            self.dragging = true;
        }
    }

    /// What the gesture means with the pointer at `pointer`.
    pub fn resolve(&self, pointer: Pos2, env: &Env<'_>) -> Resolved {
        match &self.kind {
            Kind::Pencil => self.pencil(pointer, env),
            Kind::Box { .. } => Resolved::Box(Rect::from_two_pos(self.origin, pointer)),
            _ if !self.dragging => Resolved::Nothing,
            Kind::Line => line(env, self.origin, pointer),
            Kind::Move { grabbed, .. } => shift_points(env, *grabbed, self.origin, pointer),
            Kind::Bend { from, to } => bend(env, *from, *to, pointer),
        }
    }

    /// The pencil's stroke: the whole trail once dragging, else the press
    /// point alone (so a click sets one stem or one point).
    fn path(&self, pointer: Pos2) -> Cow<'_, [Pos2]> {
        if !self.dragging {
            return Cow::Owned(vec![self.origin]);
        }
        if self.trail.last() == Some(&pointer) {
            Cow::Borrowed(self.trail.as_slice())
        } else {
            let mut path = self.trail.clone();
            path.push(pointer);
            Cow::Owned(path)
        }
    }

    fn pencil(&self, pointer: Pos2, env: &Env<'_>) -> Resolved {
        let path = self.path(pointer);
        match env.view.kind {
            LaneKind::Velocity => {
                let changes = velocity_pencil(env.view, &env.stems, &path);
                if changes.is_empty() {
                    Resolved::Nothing
                } else {
                    Resolved::Velocities(changes)
                }
            }
            _ => {
                let points = points_pencil(env, &path);
                if points.is_empty() {
                    Resolved::Nothing
                } else {
                    Resolved::Points(points)
                }
            }
        }
    }
}

/// The velocity pencil. Each stem the stroke reaches gets a velocity; the
/// stroke's segments are taken in order, so a later one overrides an earlier
/// one for the same stem. `groups` are the notes grouped by start (time
/// order), as `velocity::stems` gives them. Returns `(note, velocity)` for
/// every note of every stem set, sorted by note index.
fn velocity_pencil(
    view: &LaneView,
    groups: &[(Ticks, Vec<usize>)],
    path: &[Pos2],
) -> Vec<(usize, u8)> {
    let mut levels: BTreeMap<usize, u8> = BTreeMap::new();
    for (k, &cur) in path.iter().enumerate() {
        // The first point is a segment from itself.
        let prev = path[k.saturating_sub(1)];
        let lo = prev.x.min(cur.x) - HIT;
        let hi = prev.x.max(cur.x) + HIT;
        for i in view.stems_between(groups, lo, hi) {
            if let Some(level) = stem_level(view, view.x_of(groups[i].0), prev, cur) {
                levels.insert(i, level);
            }
        }
    }
    let mut changes = Vec::new();
    for (i, level) in levels {
        changes.extend(groups[i].1.iter().map(|&note| (note, level)));
    }
    changes.sort_unstable_by_key(|&(note, _)| note);
    changes
}

/// The velocity a stem at `x` takes from the stroke segment `prev` to `cur`,
/// or `None` when the segment does not reach it. Near the latest place
/// (within [`HIT`]) the stem takes that place's velocity; between the two
/// places it takes the velocity interpolated by x (the pointer jumped over
/// it); near only the earlier place, that place's velocity.
fn stem_level(view: &LaneView, x: f32, prev: Pos2, cur: Pos2) -> Option<u8> {
    let (before, after) = (view.value_at(prev.y), view.value_at(cur.y));
    if (x - cur.x).abs() <= HIT {
        return Some(clamp_velocity(after));
    }
    if (prev.x.min(cur.x)..=prev.x.max(cur.x)).contains(&x) {
        // Not both places at one x: that case is caught above.
        let t = f64::from(x - prev.x) / f64::from(cur.x - prev.x);
        return Some(between_velocities(before, after, t));
    }
    ((x - prev.x).abs() <= HIT).then(|| clamp_velocity(before))
}

/// The point pencil over a controller-style lane. Each path point is snapped
/// to the grid (rounded down to a multiple of the step when there is one) and
/// valued by the pointer's height. Between two path points every step is
/// filled with an interpolated value, so a fast stroke leaves no gaps; with no
/// grid, one point a pixel of the view. Times are clamped into the clip. The
/// result is sorted by time with one point per tick (a later one wins).
fn points_pencil(env: &Env<'_>, path: &[Pos2]) -> Vec<(Ticks, i32)> {
    if env.length <= Ticks::ZERO {
        return Vec::new();
    }
    let last = env.length - Ticks(1);
    let samples: Vec<(Ticks, i32)> = path.iter().map(|&p| sample(env, last, p)).collect();
    let Some(&first) = samples.first() else {
        return Vec::new();
    };
    let fill = fill_step(env);
    let mut points = vec![first];
    for pair in samples.windows(2) {
        points.extend(fill_between(pair[0], pair[1], fill));
    }
    // A stable sort keeps a later point after an earlier one on its tick.
    points.sort_by_key(|&(at, _)| at);
    let mut out: Vec<(Ticks, i32)> = Vec::with_capacity(points.len());
    for point in points {
        match out.last_mut() {
            Some(previous) if previous.0 == point.0 => *previous = point,
            _ => out.push(point),
        }
    }
    out
}

/// The snapped, grid-floored time and the value under a path point, with the
/// time clamped into `[0, last]`.
fn sample(env: &Env<'_>, last: Ticks, p: Pos2) -> (Ticks, i32) {
    let at = (env.snap)(env.view.local_at(p.x));
    let at = if env.step > Ticks::ZERO {
        Ticks(at.0 - at.0.rem_euclid(env.step.0))
    } else {
        at
    };
    (at.clamp(Ticks::ZERO, last), env.view.value_at(p.y))
}

/// The spacing of the points a stroke fills in: the grid step, or with no
/// grid one pixel of the view, so the count follows the pointer's travel.
fn fill_step(env: &Env<'_>) -> Ticks {
    if env.step > Ticks::ZERO {
        return env.step;
    }
    let per_pixel = PPQ as f64 / env.view.time.zoom;
    if per_pixel.is_finite() && per_pixel >= 1.0 {
        Ticks(per_pixel.ceil() as i64)
    } else {
        Ticks(1)
    }
}

/// The points from `a` to `b` every `step` ticks, each interpolated, then `b`.
/// `a` itself is not repeated: the caller already has it. Equal ticks give `b`
/// alone.
fn fill_between(a: (Ticks, i32), b: (Ticks, i32), step: Ticks) -> Vec<(Ticks, i32)> {
    let span = b.0 - a.0;
    if span == Ticks::ZERO {
        return vec![b];
    }
    let dir = span.0.signum();
    let stride = Ticks(dir * step.0.max(1));
    let before = |at: Ticks| if dir > 0 { at < b.0 } else { at > b.0 };
    let mut out = Vec::new();
    let mut at = a.0 + stride;
    while before(at) {
        out.push((at, lerp(a, b, at)));
        at = at + stride;
    }
    out.push(b);
    out
}

/// The value between `a` and `b` at time `at`, rounded.
fn lerp(a: (Ticks, i32), b: (Ticks, i32), at: Ticks) -> i32 {
    let t = (at - a.0).0 as f64 / (b.0 - a.0).0 as f64;
    (f64::from(a.1) + (f64::from(b.1) - f64::from(a.1)) * t).round() as i32
}

/// A line from the press to the pointer, the ends in time order: a span in
/// the controller lanes, a ramp in the velocity lane.
fn line(env: &Env<'_>, origin: Pos2, pointer: Pos2) -> Resolved {
    let view = env.view;
    let mut from = ((env.snap)(view.local_at(origin.x)), view.value_at(origin.y));
    let mut to = (
        (env.snap)(view.local_at(pointer.x)),
        view.value_at(pointer.y),
    );
    if from.0 == to.0 {
        return Resolved::Nothing;
    }
    if to.0 < from.0 {
        std::mem::swap(&mut from, &mut to);
    }
    shaped(env, from, to, Curve::LINEAR)
}

/// The two ends with a curve, as the lane's own kind takes them: a span, or
/// a ramp in the velocity lane.
fn shaped(env: &Env<'_>, from: (Ticks, i32), to: (Ticks, i32), curve: Curve) -> Resolved {
    match env.view.kind {
        LaneKind::Velocity => Resolved::Ramp {
            from: (from.0, clamp_velocity(from.1)),
            to: (to.0, clamp_velocity(to.1)),
            notes: notes_between(env.notes, from.0, to.0),
            curve,
        },
        _ => Resolved::Span { from, to, curve },
    }
}

/// Moves the grabbed point by the pointer's travel. Its time snaps; the other
/// picks keep their offsets from it (the Document clamps the group).
fn shift_points(env: &Env<'_>, grabbed: usize, origin: Pos2, pointer: Pos2) -> Resolved {
    let Some(event) = env.events.get(grabbed) else {
        return Resolved::Nothing;
    };
    let view = env.view;
    let delta = view.local_at(pointer.x) - view.local_at(origin.x);
    let target = (env.snap)(event.at + delta);
    Resolved::Shift {
        time: target - event.at,
        value: view.value_at(pointer.y) - view.value_at(origin.y),
    }
}

/// The curve of a line from `from` to `to` that passes through the dot the
/// pointer drags (C3's handle, with no S-shape). The dot's position is taken
/// along the line's ends, unrounded, and kept in the handle's range.
fn bend(env: &Env<'_>, from: (Ticks, i32), to: (Ticks, i32), pointer: Pos2) -> Resolved {
    if from.0 == to.0 || from.1 == to.1 {
        return Resolved::Nothing;
    }
    let view = env.view;
    let along = (view.local_at(pointer.x) - from.0).0 as f64 / (to.0 - from.0).0 as f64;
    let level = (view.value_exact(pointer.y) - f64::from(from.1)) / f64::from(to.1 - from.1);
    let dot = [along as f32, level as f32].map(|v| v.clamp(DOT_MIN, DOT_MAX));
    let curve = Curve::from_handle(Handle { dot, s: 0.0 });
    shaped(env, from, to, curve)
}

/// A velocity brought into the lane's range (1 to 127).
fn clamp_velocity(value: i32) -> u8 {
    let (low, high) = LaneKind::Velocity.range();
    value.clamp(low, high) as u8
}

/// The velocity `t` of the way from `a` to `b`, rounded and in range.
fn between_velocities(a: i32, b: i32, t: f64) -> u8 {
    clamp_velocity((f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as i32)
}

/// The indices of the notes whose start lies between `a` and `b`, both ends
/// included, ascending. Order of the two does not matter.
pub(crate) fn notes_between(notes: &[Note], a: Ticks, b: Ticks) -> Vec<usize> {
    let range = a.min(b)..=a.max(b);
    notes
        .iter()
        .enumerate()
        .filter(|(_, note)| range.contains(&note.start()))
        .map(|(i, _)| i)
        .collect()
}

/// The notes of the stems a box catches: a stem is caught when its x lies in
/// the box and its head, the first note's velocity, lies in the box's height.
/// Returns every note of the caught stems, ascending.
pub(crate) fn boxed_stems(
    view: &LaneView,
    notes: &[Note],
    stems: &[(Ticks, Vec<usize>)],
    rect: Rect,
) -> Vec<usize> {
    let mut picked: Vec<usize> = view
        .stems_between(stems, rect.left(), rect.right())
        .filter(|&i| {
            stems[i]
                .1
                .first()
                .and_then(|&note| notes.get(note))
                .is_some_and(|note| {
                    let y = view.y_of(i32::from(note.velocity()));
                    (rect.top()..=rect.bottom()).contains(&y)
                })
        })
        .flat_map(|i| stems[i].1.iter().copied())
        .collect();
    picked.sort_unstable();
    picked
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;
    use lumen_project::edit::velocity::stems as stems_of;
    use lumen_project::model::EventKind;

    use super::*;
    use crate::timeview::TimeView;
    use lumen_project::edit::velocity;

    /// Beats to clip time.
    fn beats(n: f64) -> Ticks {
        Ticks((n * PPQ as f64) as i64)
    }

    /// The velocity lane or a controller lane over beats 0 to 10, 40 points a
    /// beat, with the plotting area from y 58 to 242.
    fn lane(kind: LaneKind) -> LaneView {
        LaneView {
            body: Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 250.0)),
            time: TimeView {
                left: 0.0,
                zoom: 40.0,
            },
            clip_start: Ticks::ZERO,
            kind,
        }
    }

    /// The screen point at `beat` and `value` in `view`.
    fn at(view: &LaneView, beat: f64, value: i32) -> Pos2 {
        pos2(view.x_of(beats(beat)), view.y_of(value))
    }

    fn note(start: f64, pitch: u8, velocity: u8) -> Note {
        Note::new(beats(start), beats(1.0), pitch, velocity)
    }

    fn cc(at: Ticks, number: u8, value: u8) -> ClipEvent {
        ClipEvent {
            at,
            kind: EventKind::Controller { number, value },
        }
    }

    /// What a gesture reads: owned, so each test can set its parts.
    struct World {
        view: LaneView,
        notes: Vec<Note>,
        events: Vec<ClipEvent>,
        step: Ticks,
        length: Ticks,
    }

    impl World {
        fn new(view: LaneView) -> Self {
            Self {
                view,
                notes: Vec::new(),
                events: Vec::new(),
                step: Ticks::ZERO,
                length: beats(8.0),
            }
        }

        fn env<'a>(&'a self, snap: &'a dyn Fn(Ticks) -> Ticks) -> Env<'a> {
            Env {
                view: &self.view,
                notes: &self.notes,
                events: &self.events,
                step: self.step,
                snap,
                length: self.length,
                stems: std::borrow::Cow::Owned(velocity::stems(&self.notes)),
            }
        }
    }

    /// Notes at beats 0, 1, 2, 3 and 6: stems at x 100, 140, 180, 220, 340.
    fn stroke_notes() -> Vec<Note> {
        [0.0, 1.0, 2.0, 3.0, 6.0]
            .into_iter()
            .map(|b| note(b, 60, 64))
            .collect()
    }

    /// Snapping that does nothing.
    fn free(t: Ticks) -> Ticks {
        t
    }

    /// Snapping to the nearest quarter.
    fn quarters(t: Ticks) -> Ticks {
        Ticks((t.as_quarters().round() as i64) * PPQ)
    }

    fn span_curve(resolved: Resolved) -> Curve {
        let Resolved::Span { curve, .. } = resolved else {
            panic!("expected a span, got {resolved:?}");
        };
        curve
    }

    #[test]
    fn the_default_tool_is_the_pencil() {
        assert_eq!(LaneTool::default(), LaneTool::Pencil);
    }

    #[test]
    fn a_press_becomes_a_drag_only_past_the_threshold() {
        let mut gesture = LaneGesture::new(Kind::Pencil, pos2(100.0, 100.0));
        assert_eq!(gesture.trail, [pos2(100.0, 100.0)]);
        gesture.feed(pos2(103.0, 100.0));
        assert!(!gesture.dragging, "exactly the threshold is not past it");
        gesture.feed(pos2(103.5, 100.0));
        assert!(gesture.dragging);
        gesture.feed(pos2(100.0, 100.0));
        assert!(gesture.dragging, "once dragging, it stays");
        assert_eq!(gesture.trail.len(), 4, "every new place is recorded");
    }

    #[test]
    fn a_repeated_place_is_not_recorded_twice() {
        let mut gesture = LaneGesture::new(Kind::Pencil, pos2(100.0, 100.0));
        gesture.feed(pos2(100.0, 100.0));
        assert_eq!(gesture.trail.len(), 1);
    }

    #[test]
    fn a_velocity_click_sets_the_chord_under_the_press_to_one_value() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = vec![
            note(1.0, 60, 40),
            note(1.0, 64, 40),
            note(1.0, 67, 40),
            note(2.0, 62, 40),
        ];
        let snap = free;
        let press = at(&world.view, 1.0, 90);
        let gesture = LaneGesture::new(Kind::Pencil, press);
        assert_eq!(
            gesture.resolve(press, &world.env(&snap)),
            Resolved::Velocities(vec![(0, 90), (1, 90), (2, 90)])
        );
    }

    #[test]
    fn a_velocity_click_away_from_every_stem_changes_nothing() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = vec![note(1.0, 60, 40), note(2.0, 60, 40)];
        let snap = free;
        let press = at(&world.view, 0.5, 90);
        let gesture = LaneGesture::new(Kind::Pencil, press);
        assert_eq!(gesture.resolve(press, &world.env(&snap)), Resolved::Nothing);
    }

    #[test]
    fn a_press_that_has_not_become_a_drag_sets_only_the_press_point() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = stroke_notes();
        let snap = free;
        let press = at(&world.view, 1.0, 30);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        let pointer = pos2(press.x + 2.0, press.y + 1.0);
        gesture.feed(pointer);
        assert!(!gesture.dragging, "under the threshold: still a click");
        // Only the press point is resolved, whatever the pointer has done.
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Velocities(vec![(1, 30)])
        );
    }

    #[test]
    fn the_velocity_pencil_does_not_skip_stems_the_pointer_jumps_over() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = stroke_notes();
        let snap = free;
        let press = at(&world.view, 0.0, 20);
        let pointer = pos2(230.0, world.view.y_of(120));
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        // Stems at x 100, 140, 180 and 220 lie between the two places; the
        // one at x 340 is out of reach.
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Velocities(vec![(0, 20), (1, 51), (2, 82), (3, 112)])
        );
    }

    #[test]
    fn a_later_point_of_the_stroke_overrides_an_earlier_one_for_the_same_stem() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = vec![note(1.0, 60, 40)];
        let snap = free;
        let press = pos2(140.0, world.view.y_of(30));
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        let pointer = pos2(141.0, world.view.y_of(80));
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Velocities(vec![(0, 80)])
        );
    }

    #[test]
    fn a_velocity_stroke_resolves_every_note_of_each_stem_it_reaches() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = vec![note(1.0, 60, 40), note(1.0, 64, 40), note(2.0, 60, 40)];
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let pointer = at(&world.view, 2.0, 100);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        let resolved = gesture.resolve(pointer, &world.env(&snap));
        // Stem at beat 1 (x 140) at 10 + 90 * 0.5 = 55; stem at beat 2 at 100.
        assert_eq!(
            resolved,
            Resolved::Velocities(vec![(0, 55), (1, 55), (2, 100)])
        );
    }

    #[test]
    fn the_controller_pencil_fills_the_gaps_of_a_fast_stroke() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.step = Ticks(PPQ / 4);
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let pointer = at(&world.view, 1.0, 110);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Points(vec![
                (beats(0.0), 10),
                (beats(0.25), 35),
                (beats(0.5), 60),
                (beats(0.75), 85),
                (beats(1.0), 110),
            ])
        );
    }

    #[test]
    fn the_controller_pencil_clamps_points_into_the_clip() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.step = Ticks(PPQ / 4);
        world.length = beats(2.0);
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let pointer = at(&world.view, 5.0, 100);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        let Resolved::Points(points) = gesture.resolve(pointer, &world.env(&snap)) else {
            panic!("expected points");
        };
        let last = world.length - Ticks(1);
        assert!(
            points
                .iter()
                .all(|&(at, _)| at >= Ticks::ZERO && at <= last)
        );
        assert_eq!(points.last(), Some(&(last, 100)));
        assert!(
            points.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted, one per tick"
        );
    }

    #[test]
    fn with_no_grid_the_controller_pencil_fills_a_point_a_pixel() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.step = Ticks::ZERO;
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let pointer = at(&world.view, 1.0, 10);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        let Resolved::Points(points) = gesture.resolve(pointer, &world.env(&snap)) else {
            panic!("expected points");
        };
        // One point a pixel: a beat is 40 pixels, so 40 steps of PPQ / 40 and
        // the origin, which makes 41 points.
        assert_eq!(points.len(), 41);
        assert_eq!(points.first(), Some(&(Ticks::ZERO, 10)));
        assert_eq!(points.last(), Some(&(beats(1.0), 10)));
        let pixel = Ticks(PPQ / 40);
        assert!(points.windows(2).all(|w| w[1].0 - w[0].0 <= pixel));
    }

    #[test]
    fn a_later_point_on_the_same_tick_wins() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.step = Ticks(PPQ / 4);
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let pointer = at(&world.view, 0.0, 90);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Points(vec![(Ticks::ZERO, 90)])
        );
    }

    #[test]
    fn a_controller_click_sets_one_point_at_the_grid() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.step = Ticks(PPQ / 4);
        let snap = free;
        let press = at(&world.view, 1.0, 64);
        let gesture = LaneGesture::new(Kind::Pencil, press);
        assert_eq!(
            gesture.resolve(press, &world.env(&snap)),
            Resolved::Points(vec![(beats(1.0), 64)])
        );
    }

    #[test]
    fn a_pitch_bend_stroke_is_points_too() {
        let world = World::new(lane(LaneKind::PitchBend));
        let snap = free;
        let press = at(&world.view, 1.0, 0);
        let gesture = LaneGesture::new(Kind::Pencil, press);
        assert_eq!(
            gesture.resolve(press, &world.env(&snap)),
            Resolved::Points(vec![(beats(1.0), 0)])
        );
    }

    #[test]
    fn a_line_in_a_controller_lane_is_a_linear_span_in_time_order() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = at(&world.view, 0.0, 0);
        let pointer = at(&world.view, 1.0, 64);
        let mut gesture = LaneGesture::new(Kind::Line, origin);
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Span {
                from: (Ticks::ZERO, 0),
                to: (beats(1.0), 64),
                curve: Curve::LINEAR,
            }
        );
    }

    #[test]
    fn a_line_dragged_right_to_left_keeps_its_ends_in_time_order() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = at(&world.view, 1.0, 10);
        let pointer = at(&world.view, 0.0, 90);
        let mut gesture = LaneGesture::new(Kind::Line, origin);
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Span {
                from: (Ticks::ZERO, 90),
                to: (beats(1.0), 10),
                curve: Curve::LINEAR,
            }
        );
    }

    #[test]
    fn a_velocity_line_is_a_ramp_over_the_notes_between_its_ends() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = [0.0, 1.0, 2.0, 3.0, 4.0]
            .into_iter()
            .map(|b| note(b, 60, 64))
            .collect();
        let snap = free;
        let expected = Resolved::Ramp {
            from: (beats(1.0), 30),
            to: (beats(3.0), 100),
            notes: vec![1, 2, 3],
            curve: Curve::LINEAR,
        };
        let forward = {
            let origin = at(&world.view, 1.0, 30);
            let pointer = at(&world.view, 3.0, 100);
            let mut gesture = LaneGesture::new(Kind::Line, origin);
            gesture.feed(pointer);
            gesture.resolve(pointer, &world.env(&snap))
        };
        assert_eq!(forward, expected);
        let backward = {
            let origin = at(&world.view, 3.0, 100);
            let pointer = at(&world.view, 1.0, 30);
            let mut gesture = LaneGesture::new(Kind::Line, origin);
            gesture.feed(pointer);
            gesture.resolve(pointer, &world.env(&snap))
        };
        assert_eq!(backward, expected);
    }

    #[test]
    fn a_line_whose_ends_fall_on_one_tick_changes_nothing() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = quarters;
        let origin = at(&world.view, 1.0, 10);
        // 1 beat plus 1/40 of a beat: both ends snap to beat 1.
        let pointer = pos2(origin.x + 1.0, world.view.y_of(90));
        let mut gesture = LaneGesture::new(Kind::Line, origin);
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Nothing
        );
    }

    #[test]
    fn a_line_that_has_not_been_dragged_changes_nothing() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = at(&world.view, 1.0, 10);
        let gesture = LaneGesture::new(Kind::Line, origin);
        assert_eq!(
            gesture.resolve(origin, &world.env(&snap)),
            Resolved::Nothing
        );
    }

    #[test]
    fn a_bend_with_its_dot_in_the_middle_is_a_straight_line() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 0),
            to: (beats(4.0), 100),
        };
        let origin = at(&world.view, 2.0, 50);
        let mut gesture = LaneGesture::new(kind, origin);
        gesture.feed(pos2(origin.x, origin.y + 10.0));
        let curve = span_curve(gesture.resolve(origin, &world.env(&snap)));
        assert!((curve.at(0.5) - 0.5).abs() < 1e-3, "{curve:?}");
    }

    #[test]
    fn a_bend_dragged_below_the_straight_line_curves_below_it() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 0),
            to: (beats(4.0), 100),
        };
        let pointer = at(&world.view, 2.0, 25);
        let mut gesture = LaneGesture::new(kind, pointer);
        gesture.feed(pos2(pointer.x, pointer.y - 10.0));
        let resolved = gesture.resolve(pointer, &world.env(&snap));
        let Resolved::Span { from, to, curve } = resolved else {
            panic!("expected a span");
        };
        assert_eq!((from, to), ((Ticks::ZERO, 0), (beats(4.0), 100)));
        // The curve passes through the dot: at the middle, 25 of 100.
        assert!((curve.at(0.5) - 0.25).abs() < 1e-3, "{curve:?}");
    }

    #[test]
    fn a_bend_dragged_above_the_straight_line_curves_above_it() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 0),
            to: (beats(4.0), 100),
        };
        let pointer = at(&world.view, 2.0, 75);
        let mut gesture = LaneGesture::new(kind, pointer);
        gesture.feed(pos2(pointer.x, pointer.y + 10.0));
        let curve = span_curve(gesture.resolve(pointer, &world.env(&snap)));
        assert!((curve.at(0.5) - 0.75).abs() < 1e-3, "{curve:?}");
    }

    #[test]
    fn a_bend_passes_through_its_dot_wherever_the_dot_is() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 0),
            to: (beats(4.0), 100),
        };
        // A quarter of the way along, at 80 of 100.
        let pointer = at(&world.view, 1.0, 80);
        let mut gesture = LaneGesture::new(kind, pointer);
        gesture.feed(pos2(pointer.x, pointer.y + 10.0));
        let curve = span_curve(gesture.resolve(pointer, &world.env(&snap)));
        assert!((curve.at(0.25) - 0.8).abs() < 2e-3, "{curve:?}");
        assert!(curve.is_valid());
    }

    #[test]
    fn a_bend_far_outside_the_line_keeps_its_curve_valid() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 0),
            to: (beats(4.0), 100),
        };
        let pointer = pos2(10_000.0, -10_000.0);
        let mut gesture = LaneGesture::new(kind, pointer);
        gesture.feed(pos2(10_000.0, -9_990.0));
        let curve = span_curve(gesture.resolve(pointer, &world.env(&snap)));
        assert!(curve.is_valid(), "{curve:?}");
    }

    #[test]
    fn a_bend_of_a_flat_line_or_of_no_time_changes_nothing() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let pointer = at(&world.view, 2.0, 50);
        for kind in [
            Kind::Bend {
                from: (Ticks::ZERO, 40),
                to: (beats(4.0), 40),
            },
            Kind::Bend {
                from: (beats(1.0), 0),
                to: (beats(1.0), 100),
            },
        ] {
            let mut gesture = LaneGesture::new(kind, pointer);
            gesture.feed(pos2(pointer.x, pointer.y + 10.0));
            assert_eq!(
                gesture.resolve(pointer, &world.env(&snap)),
                Resolved::Nothing
            );
        }
    }

    #[test]
    fn a_bend_in_the_velocity_lane_is_a_ramp_with_its_curve() {
        let mut world = World::new(lane(LaneKind::Velocity));
        world.notes = [0.0, 1.0, 2.0, 3.0, 4.0]
            .into_iter()
            .map(|b| note(b, 60, 64))
            .collect();
        let snap = free;
        let kind = Kind::Bend {
            from: (Ticks::ZERO, 20),
            to: (beats(4.0), 120),
        };
        let pointer = at(&world.view, 2.0, 40);
        let mut gesture = LaneGesture::new(kind, pointer);
        gesture.feed(pos2(pointer.x, pointer.y + 10.0));
        let Resolved::Ramp {
            from,
            to,
            notes,
            curve,
        } = gesture.resolve(pointer, &world.env(&snap))
        else {
            panic!("expected a ramp");
        };
        assert_eq!((from, to), ((Ticks::ZERO, 20), (beats(4.0), 120)));
        assert_eq!(notes, [0, 1, 2, 3, 4]);
        // The dot is at half the time and a fifth of the way up.
        assert!((curve.at(0.5) - 0.2).abs() < 2e-3, "{curve:?}");
    }

    #[test]
    fn a_move_shifts_the_group_by_the_grabbed_points_snapped_time() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.events = vec![cc(beats(0.25), 7, 50), cc(beats(1.0), 7, 20)];
        let snap = quarters;
        let origin = at(&world.view, 0.0, 50);
        let pointer = at(&world.view, 0.5, 80);
        let mut gesture = LaneGesture::new(
            Kind::Move {
                grabbed: 0,
                picked: vec![0],
            },
            origin,
        );
        gesture.feed(pointer);
        // 0.25 + 0.5 = 0.75 beats, which snaps to beat 1: a move of 0.75.
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Shift {
                time: beats(0.75),
                value: 30,
            }
        );
    }

    #[test]
    fn a_move_without_a_grabbed_point_changes_nothing() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = at(&world.view, 0.0, 50);
        let pointer = at(&world.view, 0.5, 80);
        let mut gesture = LaneGesture::new(
            Kind::Move {
                grabbed: 3,
                picked: vec![],
            },
            origin,
        );
        gesture.feed(pointer);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Nothing
        );
    }

    #[test]
    fn a_move_that_has_not_been_dragged_changes_nothing() {
        let mut world = World::new(lane(LaneKind::Controller(7)));
        world.events = vec![cc(beats(0.25), 7, 50)];
        let snap = free;
        let origin = at(&world.view, 0.0, 50);
        let gesture = LaneGesture::new(
            Kind::Move {
                grabbed: 0,
                picked: vec![0],
            },
            origin,
        );
        assert_eq!(
            gesture.resolve(origin, &world.env(&snap)),
            Resolved::Nothing
        );
    }

    #[test]
    fn a_box_is_the_rectangle_between_its_corners_whichever_way_it_is_dragged() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = pos2(300.0, 100.0);
        let pointer = pos2(200.0, 50.0);
        let gesture = LaneGesture::new(Kind::Box { base: vec![] }, origin);
        assert_eq!(
            gesture.resolve(pointer, &world.env(&snap)),
            Resolved::Box(Rect::from_min_max(pos2(200.0, 50.0), pos2(300.0, 100.0)))
        );
    }

    #[test]
    fn a_box_from_a_click_is_empty() {
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let origin = pos2(300.0, 100.0);
        let gesture = LaneGesture::new(Kind::Box { base: vec![2] }, origin);
        assert_eq!(
            gesture.resolve(origin, &world.env(&snap)),
            Resolved::Box(Rect::from_min_max(origin, origin))
        );
    }

    #[test]
    fn notes_between_takes_the_starts_in_the_range_whichever_way_round() {
        let notes = stroke_notes();
        assert_eq!(notes_between(&notes, beats(3.0), beats(1.0)), [1, 2, 3]);
        assert_eq!(notes_between(&notes, beats(1.0), beats(3.0)), [1, 2, 3]);
        assert_eq!(
            notes_between(&notes, beats(3.5), beats(5.0)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_box_over_stems_takes_the_notes_of_stems_whose_head_is_inside() {
        let view = lane(LaneKind::Velocity);
        let notes = vec![
            note(1.0, 60, 90),
            note(1.0, 64, 90),
            note(2.0, 60, 40),
            note(3.0, 60, 90),
        ];
        let stems = stems_of(&notes);
        // x 130 to 190 (beats 1 and 2), velocities 60 to 100.
        let rect = Rect::from_min_max(pos2(130.0, view.y_of(100)), pos2(190.0, view.y_of(60)));
        assert_eq!(boxed_stems(&view, &notes, &stems, rect), [0, 1]);
    }

    #[test]
    fn a_stem_is_caught_by_its_first_note_only() {
        let view = lane(LaneKind::Velocity);
        let notes = vec![note(1.0, 60, 40), note(1.0, 64, 100)];
        let stems = stems_of(&notes);
        let rect = Rect::from_min_max(pos2(130.0, view.y_of(100)), pos2(190.0, view.y_of(60)));
        assert!(boxed_stems(&view, &notes, &stems, rect).is_empty());
    }

    #[test]
    fn the_stroke_of_a_gesture_is_on_the_resolve_path_only_once_dragging() {
        // Not dragging: the path is the press alone, whatever the trail holds.
        let world = World::new(lane(LaneKind::Controller(7)));
        let snap = free;
        let press = at(&world.view, 0.0, 10);
        let mut gesture = LaneGesture::new(Kind::Pencil, press);
        gesture.trail.push(at(&world.view, 2.0, 100));
        assert_eq!(
            gesture.resolve(press, &world.env(&snap)),
            Resolved::Points(vec![(Ticks::ZERO, 10)])
        );
    }
}
