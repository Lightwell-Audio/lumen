//! Pure edits of an automation lane's points (M7 slice A2): moving a set of
//! points, giving segments a shape, replacing a span, copying, pasting and
//! duplicating points, drawing a shape (a ramp, sine, triangle, saw or
//! square) and turning a freehand stroke into points. Every function takes a
//! [`Lane`] and returns a new one (or `None` when the edit is refused), so
//! the app wraps each in one undo step and every rule is tested here.
//!
//! `picks` are indices into the lane's points, ascending and without
//! repeats. Values are in the target's own units; where an edit needs the
//! screen's idea of height (a drawn shape, a vertical move) the caller passes
//! a map from a height in `0..=1` to a value, so the arithmetic is done in
//! the space the user sees and written in the space the engine plays.

use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;

/// How many points a drawn shape or a stroke may make at most.
pub const MAX_DRAWN: usize = 8_192;

/// A set of points moved: the lane and where the moved points are in it.
/// Moving never reorders points, so `picks` are the same indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Shifted {
    pub lane: Lane,
    /// How far the points really moved: `dt` after it was held back by a
    /// neighbour or the start of the song.
    pub moved_by: Ticks,
}

/// Points taken out of a lane to put somewhere else: positions relative to
/// the first point, and the span from the first to the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Copied {
    /// Sorted, the first at [`Ticks::ZERO`].
    pub points: Vec<Point>,
    /// Last point's position (relative); zero for a single point.
    pub span: Ticks,
}

/// The kinds of shape the shape tools draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    /// One straight run from where it starts to where it ends.
    #[default]
    Ramp,
    Sine,
    Triangle,
    /// Rises over the period, then drops at its end.
    Saw,
    /// Holds one end for half the period, then the other (step segments).
    Square,
}

/// `lane` with the `picks` moved `dt` later (earlier if negative) and every
/// picked value passed through `value`. The block stops against the nearest
/// point that is not picked (a tick clear of it) and at the start of the
/// song, so points never pass one another: `Shifted::moved_by` says how far
/// it got. `None` if `picks` is empty or names a point that is not there, or
/// the values are not finite.
pub fn shifted(
    lane: &Lane,
    picks: &[usize],
    dt: Ticks,
    value: impl Fn(f32) -> f32,
) -> Option<Shifted> {
    let _ = (lane, picks, dt, &value);
    todo!("A2 unit 1")
}

/// `lane` with the segment leaving each picked point given `shape`. `None`
/// if `picks` is empty or names a point that is not there.
pub fn with_shape(lane: &Lane, picks: &[usize], shape: Shape) -> Option<Lane> {
    let _ = (lane, picks, shape);
    todo!("A2 unit 1")
}

/// `lane` without the points in `[from, to]` and with `new` (sorted, one to
/// a position) in their place. `None` if the result would hold too many
/// points.
pub fn replace_span(lane: &Lane, from: Ticks, to: Ticks, new: &[Point]) -> Option<Lane> {
    let _ = (lane, from, to, new);
    todo!("A2 unit 1")
}

/// The picked points as something to paste. `None` if `picks` is empty or
/// names a point that is not there.
pub fn copy(lane: &Lane, picks: &[usize]) -> Option<Copied> {
    let _ = (lane, picks);
    todo!("A2 unit 1")
}

/// `lane` with `copied` pasted so its first point lands on `at`: the points
/// already in the pasted span are replaced. Also the indices of the pasted
/// points in the new lane. `None` if the result would hold too many points
/// or `at` is before the start of the song.
pub fn paste(lane: &Lane, copied: &Copied, at: Ticks) -> Option<(Lane, Vec<usize>)> {
    let _ = (lane, copied, at);
    todo!("A2 unit 1")
}

/// The picked points repeated right after themselves: pasted so the copy
/// begins where the picked run ends (on its last point's tick when the run
/// starts and ends at the same value, so a repeating shape joins up; one tick
/// later otherwise, so a rising run repeated is a saw, not a lost point).
/// Also the indices of the new points.
pub fn duplicate(lane: &Lane, picks: &[usize]) -> Option<(Lane, Vec<usize>)> {
    let _ = (lane, picks);
    todo!("A2 unit 1")
}

/// The points of a shape drawn from `from` to `to` (either order). `start`
/// and `end` are the heights (0 to 1) of the press and the release: the
/// shape starts at `start` and heads for `end`. A ramp runs between them. The
/// others swing between the two with one cycle every `period` (at least
/// `(to - from) / 256`, so a drawn shape never makes more than
/// [`MAX_DRAWN`] points): a sine and a triangle start at `start` and reach
/// `end` half a period later, a saw rises from `start` to `end` and drops
/// back at the end of each period, a square holds `start` for half a period,
/// then `end`. `map` turns a height into the value to write. The last point
/// is on `to`. A square's points are steps; every other shape is made of
/// straight segments (a sine has 16 points to a period).
pub fn wave(
    kind: Wave,
    from: Ticks,
    to: Ticks,
    period: Ticks,
    start: f32,
    end: f32,
    map: impl Fn(f32) -> f32,
) -> Vec<Point> {
    let _ = (kind, from, to, period, start, end, &map);
    todo!("A2 unit 1")
}

/// A freehand stroke written into `base`: `raw` are the places the pointer
/// went, in time order (the caller drops samples when the pointer goes
/// back), each a position and a value. The stroke replaces the lane between
/// its first and last position. Straight strokes are thinned so the lane is
/// never further than `tolerance` (in the target's units) from the stroke;
/// with `steps` every point is a step and only repeated values are dropped
/// (for a mute lane). `None` if the result would hold too many points.
pub fn stroke(base: &Lane, raw: &[(Ticks, f32)], steps: bool, tolerance: f32) -> Option<Lane> {
    let _ = (base, raw, steps, tolerance);
    todo!("A2 unit 1")
}
