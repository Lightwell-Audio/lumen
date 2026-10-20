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

/// A drawn shape has at most this many cycles over its span, so its points
/// stay under [`MAX_DRAWN`] whatever the period.
const MOST_CYCLES: i64 = 256;

/// Whether `picks` names points of a lane of `len` points: not empty,
/// ascending and without repeats.
fn picks_valid(len: usize, picks: &[usize]) -> bool {
    !picks.is_empty()
        && picks.windows(2).all(|pair| pair[0] < pair[1])
        && picks.last().is_some_and(|&last| last < len)
}

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
    let points = lane.points();
    if !picks_valid(points.len(), picks) {
        return None;
    }
    // Each run of consecutive picks moves as one block. Its first point may
    // not pass the unpicked point before it (or the song's start), and its
    // last point may not reach the unpicked point after it.
    let (mut floor, mut ceiling) = (i64::MIN, i64::MAX);
    let mut run = 0;
    while run < picks.len() {
        let mut end = run;
        while end + 1 < picks.len() && picks[end + 1] == picks[end] + 1 {
            end += 1;
        }
        let (first, last) = (picks[run], picks[end]);
        let lowest = if first == 0 {
            Ticks::ZERO
        } else {
            points[first - 1].at + Ticks(1)
        };
        floor = floor.max(lowest.0.saturating_sub(points[first].at.0));
        if let Some(next) = points.get(last + 1) {
            let highest = next.at.0.saturating_sub(1);
            ceiling = ceiling.min(highest.saturating_sub(points[last].at.0));
        }
        run = end + 1;
    }
    let moved = dt.0.max(floor).min(ceiling);
    let mut moved_points = points.to_vec();
    for &i in picks {
        let point = &mut moved_points[i];
        point.at = Ticks(point.at.0.saturating_add(moved));
        point.value = value(point.value);
    }
    let lane = Lane::new(moved_points).ok()?;
    Some(Shifted {
        lane,
        moved_by: Ticks(moved),
    })
}

/// `lane` with the segment leaving each picked point given `shape`. `None`
/// if `picks` is empty or names a point that is not there.
pub fn with_shape(lane: &Lane, picks: &[usize], shape: Shape) -> Option<Lane> {
    let points = lane.points();
    if !picks_valid(points.len(), picks) {
        return None;
    }
    let mut shaped = points.to_vec();
    for &i in picks {
        shaped[i].shape = shape;
    }
    Lane::new(shaped).ok()
}

/// `lane` without the points in `[from, to]` and with `new` (sorted, one to
/// a position) in their place. `None` if the result would hold too many
/// points.
pub fn replace_span(lane: &Lane, from: Ticks, to: Ticks, new: &[Point]) -> Option<Lane> {
    let (from, to) = if from > to { (to, from) } else { (from, to) };
    let mut merged: Vec<Point> = lane
        .points()
        .iter()
        .copied()
        .filter(|p| p.at < from || p.at > to)
        .collect();
    merged.extend_from_slice(new);
    // A stable sort keeps `new` after an old point at the same tick, so the
    // last point at each tick, the new one, is the one kept.
    merged.sort_by_key(|p| p.at);
    let mut unique: Vec<Point> = Vec::with_capacity(merged.len());
    for point in merged {
        match unique.last_mut() {
            Some(last) if last.at == point.at => *last = point,
            _ => unique.push(point),
        }
    }
    Lane::new(unique).ok()
}

/// The picked points as something to paste. `None` if `picks` is empty or
/// names a point that is not there.
pub fn copy(lane: &Lane, picks: &[usize]) -> Option<Copied> {
    let points = lane.points();
    if !picks_valid(points.len(), picks) {
        return None;
    }
    let first = points[picks[0]].at;
    let copied: Vec<Point> = picks
        .iter()
        .map(|&i| Point {
            at: points[i].at - first,
            ..points[i]
        })
        .collect();
    let span = copied.last().map_or(Ticks::ZERO, |p| p.at);
    Some(Copied {
        points: copied,
        span,
    })
}

/// `lane` with `copied` pasted so its first point lands on `at`: the points
/// already in the pasted span are replaced. Also the indices of the pasted
/// points in the new lane. `None` if the result would hold too many points
/// or `at` is before the start of the song.
pub fn paste(lane: &Lane, copied: &Copied, at: Ticks) -> Option<(Lane, Vec<usize>)> {
    if at < Ticks::ZERO {
        return None;
    }
    if copied.points.is_empty() {
        return Some((lane.clone(), Vec::new()));
    }
    let placed: Vec<Point> = copied
        .points
        .iter()
        .map(|p| Point {
            at: p.at + at,
            ..*p
        })
        .collect();
    let out = replace_span(lane, at, at + copied.span, &placed)?;
    let indices = placed
        .iter()
        .map(|p| out.points().binary_search_by_key(&p.at, |q| q.at).ok())
        .collect::<Option<Vec<usize>>>()?;
    Some((out, indices))
}

/// The picked points repeated right after themselves: pasted so the copy
/// begins where the picked run ends (on its last point's tick when the run
/// starts and ends at the same value, so a repeating shape joins up; one tick
/// later otherwise, so a rising run repeated is a saw, not a lost point).
/// Also the indices of the new points.
pub fn duplicate(lane: &Lane, picks: &[usize]) -> Option<(Lane, Vec<usize>)> {
    let copied = copy(lane, picks)?;
    let points = lane.points();
    let (first, last) = (points[picks[0]], points[picks[picks.len() - 1]]);
    // A single point, or a run that starts and ends at different values,
    // repeats one tick later. Repeating on the same tick would paste over
    // the point it came from.
    let joins = picks.len() > 1 && (first.value - last.value).abs() <= 1e-6;
    let at = if joins { last.at } else { last.at + Ticks(1) };
    paste(lane, &copied, at)
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
    let (from, to, start, end) = if from > to {
        (to, from, end, start)
    } else {
        (from, to, start, end)
    };
    if from == to {
        return vec![Point::new(from, map(start))];
    }
    let span = (to - from).0;
    let period = period.0.max((span / MOST_CYCLES).max(1));
    let (start, end) = (f64::from(start), f64::from(end));
    // The height at the start of each half cycle: start on even cycles.
    let level = |cycle: i64| if cycle % 2 == 0 { start } else { end };
    let raw: Vec<(i64, f64)> = match kind {
        Wave::Ramp => vec![(0, start), (span, end)],
        Wave::Sine => {
            let height = |rel: i64| {
                let phase = std::f64::consts::TAU * rel as f64 / period as f64;
                (start + end) / 2.0 + (start - end) / 2.0 * phase.cos()
            };
            let step = (period + 15) / 16;
            let mut raw = Vec::new();
            let mut rel = 0;
            while rel < span {
                raw.push((rel, height(rel)));
                rel += step;
            }
            raw.push((span, height(span)));
            raw
        }
        Wave::Triangle => {
            let half = (period + 1) / 2;
            let mut raw = Vec::new();
            let mut cycle = 0;
            while cycle * half < span {
                raw.push((cycle * half, level(cycle)));
                cycle += 1;
            }
            // The last point lies on the straight run from the extreme before
            // `to` to the one after it.
            let at = span / half;
            let x = (span - at * half) as f64 / half as f64;
            let (a, b) = (level(at), level(at + 1));
            raw.push((span, a + (b - a) * x));
            raw
        }
        Wave::Saw => {
            // The rise takes all but one tick of the period; the drop is that
            // last tick, so the next start is one period after this one.
            let rise = period - 1;
            let mut raw = Vec::new();
            let mut cycle = 0;
            while cycle * period < span {
                let begin = cycle * period;
                raw.push((begin, start));
                if begin + rise < span {
                    raw.push((begin + rise, end));
                }
                cycle += 1;
            }
            let r = span % period;
            let height = if rise == 0 {
                start
            } else {
                start + (end - start) * r as f64 / rise as f64
            };
            raw.push((span, height));
            raw
        }
        Wave::Square => {
            let half = (period + 1) / 2;
            let mut raw = Vec::new();
            let mut cycle = 0;
            while cycle * half < span {
                raw.push((cycle * half, level(cycle)));
                cycle += 1;
            }
            raw.push((span, level(span / half)));
            raw
        }
    };
    let shape = if kind == Wave::Square {
        Shape::Step
    } else {
        Shape::Linear
    };
    let mut points: Vec<Point> = Vec::with_capacity(raw.len());
    for (rel, height) in raw {
        let at = from + Ticks(rel);
        if points.last().is_some_and(|p| p.at >= at) {
            continue;
        }
        points.push(Point::new(at, map(height as f32)).with_shape(shape));
    }
    points
}

/// A freehand stroke written into `base`: `raw` are the places the pointer
/// went, in time order (the caller drops samples when the pointer goes
/// back), each a position and a value. The stroke replaces the lane between
/// its first and last position. Straight strokes are thinned so the lane is
/// never further than `tolerance` (in the target's units) from the stroke;
/// with `steps` every point is a step and only repeated values are dropped
/// (for a mute lane). `None` if the result would hold too many points.
pub fn stroke(base: &Lane, raw: &[(Ticks, f32)], steps: bool, tolerance: f32) -> Option<Lane> {
    if raw.is_empty() {
        return Some(base.clone());
    }
    let mut samples = raw.to_vec();
    // A stable sort keeps a later sample after an earlier one at the same
    // tick, so the merge below keeps the later value.
    samples.sort_by_key(|&(at, _)| at);
    samples.dedup_by(|later, kept| {
        if later.0 == kept.0 {
            kept.1 = later.1;
            true
        } else {
            false
        }
    });
    let (first, last) = (samples[0].0, samples[samples.len() - 1].0);
    let mut points: Vec<Point> = Vec::with_capacity(samples.len());
    for &(at, value) in &samples {
        if steps {
            if points.last().is_some_and(|p| p.value == value) {
                continue;
            }
            points.push(Point::new(at, value).with_shape(Shape::Step));
        } else {
            points.push(Point::new(at, value));
        }
    }
    let stroked = Lane::new(points).ok()?;
    let stroked = if steps {
        stroked
    } else {
        stroked.simplify(tolerance)
    };
    replace_span(base, first, last, stroked.points())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::PPQ;

    fn q(quarters: i64) -> Ticks {
        Ticks::quarters(quarters)
    }

    fn lane(points: &[(i64, f32, Shape)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(t, v, s)| Point::new(q(t), v).with_shape(s))
                .collect(),
        )
        .unwrap()
    }

    fn positions(lane: &Lane) -> Vec<Ticks> {
        lane.points().iter().map(|p| p.at).collect()
    }

    fn values(lane: &Lane) -> Vec<f32> {
        lane.points().iter().map(|p| p.value).collect()
    }

    fn ramp_lane() -> Lane {
        lane(&[
            (0, 0.0, Shape::Linear),
            (1, 1.0, Shape::Linear),
            (2, 2.0, Shape::Linear),
            (3, 3.0, Shape::Linear),
            (4, 4.0, Shape::Linear),
        ])
    }

    const KINDS: [Wave; 5] = [
        Wave::Ramp,
        Wave::Sine,
        Wave::Triangle,
        Wave::Saw,
        Wave::Square,
    ];

    fn scaled(height: f32) -> f32 {
        height * 10.0 - 5.0
    }

    /// A small deterministic generator, so a failing case can be replayed.
    struct Rng(u64);
    impl Rng {
        fn word(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn unit(&mut self) -> f32 {
            (self.word() % 10_000) as f32 / 10_000.0
        }
    }

    #[test]
    fn shifted_moves_a_block_until_it_meets_an_unpicked_neighbour() {
        let s = shifted(&ramp_lane(), &[1, 2], q(1), |v| v).unwrap();
        assert_eq!(s.moved_by, Ticks(PPQ - 1));
        assert_eq!(
            positions(&s.lane),
            vec![
                Ticks::ZERO,
                Ticks(2 * PPQ - 1),
                Ticks(3 * PPQ - 1),
                q(3),
                q(4)
            ]
        );
    }

    #[test]
    fn shifted_holds_the_first_point_at_the_start_of_the_song() {
        let s = shifted(&ramp_lane(), &[0], Ticks(-2 * PPQ), |v| v).unwrap();
        assert_eq!(s.moved_by, Ticks::ZERO);
        assert_eq!(positions(&s.lane)[0], Ticks::ZERO);
    }

    #[test]
    fn shifted_maps_only_the_picked_values() {
        let s = shifted(&ramp_lane(), &[1, 3], Ticks::ZERO, |v| v * 10.0).unwrap();
        assert_eq!(s.moved_by, Ticks::ZERO);
        assert_eq!(values(&s.lane), vec![0.0, 10.0, 2.0, 30.0, 4.0]);
    }

    #[test]
    fn shifted_keeps_a_block_a_tick_clear_of_the_point_after_it() {
        let s = shifted(&ramp_lane(), &[2], q(5), |v| v).unwrap();
        assert_eq!(s.moved_by, Ticks(PPQ - 1));
        assert_eq!(positions(&s.lane)[2], q(3) - Ticks(1));
    }

    #[test]
    fn shifted_refuses_picks_it_cannot_read_and_values_that_are_not_finite() {
        let l = ramp_lane();
        assert!(shifted(&l, &[], q(1), |v| v).is_none());
        assert!(shifted(&l, &[3, 1], q(1), |v| v).is_none());
        assert!(shifted(&l, &[9], q(1), |v| v).is_none());
        assert!(shifted(&l, &[1], q(1), |_| f32::NAN).is_none());
    }

    #[test]
    fn with_shape_changes_only_the_picked_segments() {
        let l = with_shape(&ramp_lane(), &[1, 3], Shape::Step).unwrap();
        let shapes: Vec<Shape> = l.points().iter().map(|p| p.shape).collect();
        assert_eq!(
            shapes,
            vec![
                Shape::Linear,
                Shape::Step,
                Shape::Linear,
                Shape::Step,
                Shape::Linear
            ]
        );
        assert!(with_shape(&ramp_lane(), &[], Shape::Step).is_none());
    }

    #[test]
    fn replace_span_takes_out_the_inclusive_span_and_keeps_the_points_sorted() {
        let new = [Point::new(q(6), 6.0), Point::new(q(2), 9.0)];
        let expected = lane(&[
            (0, 0.0, Shape::Linear),
            (2, 9.0, Shape::Linear),
            (4, 4.0, Shape::Linear),
            (6, 6.0, Shape::Linear),
        ]);
        assert_eq!(
            replace_span(&ramp_lane(), q(1), q(3), &new).unwrap(),
            expected
        );
        assert_eq!(
            replace_span(&ramp_lane(), q(3), q(1), &new).unwrap(),
            expected
        );
    }

    #[test]
    fn a_new_point_wins_where_a_position_repeats() {
        let out = replace_span(&ramp_lane(), q(1), q(1), &[Point::new(q(4), 7.0)]).unwrap();
        assert_eq!(
            out,
            lane(&[
                (0, 0.0, Shape::Linear),
                (2, 2.0, Shape::Linear),
                (3, 3.0, Shape::Linear),
                (4, 7.0, Shape::Linear),
            ])
        );
    }

    #[test]
    fn a_copy_pasted_elsewhere_keeps_its_shapes_and_replaces_what_was_there() {
        let l = lane(&[
            (0, 0.0, Shape::Linear),
            (1, 1.0, Shape::Step),
            (2, 2.0, Shape::Linear),
            (3, 3.0, Shape::Linear),
            (8, 8.0, Shape::Linear),
        ]);
        let copied = copy(&l, &[1, 2]).unwrap();
        assert_eq!(
            copied.points,
            vec![
                Point::new(Ticks::ZERO, 1.0).with_shape(Shape::Step),
                Point::new(Ticks(PPQ), 2.0),
            ]
        );
        assert_eq!(copied.span, Ticks(PPQ));

        let (pasted, indices) = paste(&l, &copied, q(3)).unwrap();
        assert_eq!(
            pasted,
            lane(&[
                (0, 0.0, Shape::Linear),
                (1, 1.0, Shape::Step),
                (2, 2.0, Shape::Linear),
                (3, 1.0, Shape::Step),
                (4, 2.0, Shape::Linear),
                (8, 8.0, Shape::Linear),
            ])
        );
        assert_eq!(indices, vec![3, 4]);

        let (same, _) = paste(&l, &copied, q(1)).unwrap();
        assert_eq!(same, l);
        assert!(paste(&l, &copied, Ticks(-PPQ)).is_none());
    }

    #[test]
    fn a_rising_run_duplicated_makes_a_saw_and_loses_no_point() {
        let l = lane(&[
            (0, 0.0, Shape::Linear),
            (1, 1.0, Shape::Linear),
            (4, 4.0, Shape::Linear),
        ]);
        let (out, indices) = duplicate(&l, &[0, 1]).unwrap();
        assert_eq!(
            positions(&out),
            vec![Ticks::ZERO, q(1), q(1) + Ticks(1), q(2) + Ticks(1), q(4)]
        );
        assert_eq!(indices, vec![2, 3]);
        assert_eq!(out.value_at(q(1)), Some(1.0));
        assert_eq!(out.value_at(q(1) + Ticks(1)), Some(0.0));
    }

    #[test]
    fn a_closed_shape_duplicated_joins_on_one_tick() {
        let l = lane(&[
            (0, 0.5, Shape::Linear),
            (1, 0.5, Shape::Linear),
            (4, 4.0, Shape::Linear),
        ]);
        let (out, indices) = duplicate(&l, &[0, 1]).unwrap();
        assert_eq!(positions(&out), vec![Ticks::ZERO, q(1), q(2), q(4)]);
        assert_eq!(values(&out), vec![0.5, 0.5, 0.5, 4.0]);
        assert_eq!(indices, vec![1, 2]);
    }

    #[test]
    fn a_single_point_is_repeated_one_tick_later() {
        let l = lane(&[(0, 0.0, Shape::Linear), (4, 4.0, Shape::Linear)]);
        let (out, indices) = duplicate(&l, &[0]).unwrap();
        assert_eq!(positions(&out), vec![Ticks::ZERO, Ticks(1), q(4)]);
        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn every_kind_is_a_lane_that_starts_at_its_start_and_ends_on_to() {
        for kind in KINDS {
            let points = wave(kind, q(0), q(8), q(2), 0.25, 0.75, scaled);
            assert!(Lane::new(points.clone()).is_ok(), "{kind:?}");
            assert_eq!(points[0].at, q(0), "{kind:?}");
            assert!((points[0].value - scaled(0.25)).abs() < 1e-5, "{kind:?}");
            assert_eq!(points.last().unwrap().at, q(8), "{kind:?}");
        }
    }

    #[test]
    fn a_drawn_shape_over_a_point_is_just_that_point() {
        assert_eq!(
            wave(Wave::Sine, q(3), q(3), q(1), 0.2, 0.8, |h| h),
            vec![Point::new(q(3), 0.2)]
        );
    }

    #[test]
    fn a_sine_reaches_its_end_at_half_a_period_and_stays_between_its_heights() {
        let points = wave(Wave::Sine, q(0), q(16), q(4), 0.2, 0.8, |h| h);
        let half = points.iter().find(|p| p.at == q(2)).unwrap();
        assert!((half.value - 0.8).abs() < 1e-5, "{}", half.value);
        assert!(
            points
                .iter()
                .all(|p| (0.2 - 1e-5..=0.8 + 1e-5).contains(&p.value))
        );
    }

    #[test]
    fn a_square_is_all_steps_that_hold_one_level_then_the_other() {
        let points = wave(Wave::Square, q(0), q(8), q(2), 0.0, 1.0, |h| h);
        assert!(points.iter().all(|p| p.shape == Shape::Step));
        let value_at = |t: Ticks| points.iter().find(|p| p.at == t).unwrap().value;
        assert_eq!(value_at(q(0)), 0.0);
        assert_eq!(value_at(q(1)), 1.0);
        assert_eq!(value_at(q(2)), 0.0);
    }

    #[test]
    fn a_period_of_nothing_is_the_least_period() {
        for kind in KINDS {
            assert_eq!(
                wave(kind, q(0), q(8), Ticks(-3), 0.0, 1.0, |h| h),
                wave(kind, q(0), q(8), Ticks::ZERO, 0.0, 1.0, |h| h),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_huge_span_with_a_tiny_period_never_makes_more_than_the_limit() {
        for kind in KINDS {
            let points = wave(kind, Ticks::ZERO, q(10_000_000), Ticks(1), 0.0, 1.0, |h| h);
            assert!(points.len() <= MAX_DRAWN, "{kind:?}: {}", points.len());
            assert!(Lane::new(points.clone()).is_ok(), "{kind:?}");
            assert_eq!(points.last().unwrap().at, q(10_000_000), "{kind:?}");
        }
    }

    #[test]
    fn a_straight_stroke_thins_to_its_two_ends() {
        let raw: Vec<(Ticks, f32)> = (0..500i64)
            .map(|k| (Ticks(k * 1000), k as f32 * 0.001))
            .collect();
        let out = stroke(&Lane::default(), &raw, false, 0.001).unwrap();
        assert_eq!(positions(&out), vec![Ticks(0), Ticks(499_000)]);
        assert!((out.points()[1].value - 0.499).abs() < 1e-5);
    }

    #[test]
    fn a_stroke_is_never_further_than_its_tolerance_from_its_samples() {
        let mut rng = Rng(5);
        for case in 0..50 {
            let mut value = 0.0f32;
            let raw: Vec<(Ticks, f32)> = (0..300i64)
                .map(|k| {
                    value += rng.unit() - 0.5;
                    (Ticks(k * 700), value)
                })
                .collect();
            let tolerance = 0.02 + rng.unit() * 0.5;
            let out = stroke(&Lane::default(), &raw, false, tolerance).unwrap();
            for &(t, v) in &raw {
                let got = out.value_at(t).unwrap();
                assert!(
                    (got - v).abs() <= tolerance + 1e-4,
                    "case {case} at {t:?}: {got} against {v}, tolerance {tolerance}"
                );
            }
        }
    }

    #[test]
    fn a_step_stroke_drops_repeated_values_and_keeps_its_steps() {
        let raw: Vec<(Ticks, f32)> = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0]
            .iter()
            .enumerate()
            .map(|(k, &v)| (Ticks(k as i64 * 1000), v))
            .collect();
        let out = stroke(&Lane::default(), &raw, true, 0.0).unwrap();
        assert_eq!(
            positions(&out),
            vec![Ticks(0), Ticks(3000), Ticks(5000), Ticks(7000)]
        );
        assert_eq!(values(&out), vec![0.0, 1.0, 0.0, 2.0]);
        assert!(out.points().iter().all(|p| p.shape == Shape::Step));
    }

    #[test]
    fn a_stroke_leaves_the_lane_outside_its_span_alone() {
        let base = lane(&[
            (0, 0.0, Shape::Linear),
            (1, 1.0, Shape::Step),
            (2, 2.0, Shape::Linear),
            (3, 3.0, Shape::Linear),
            (4, 4.0, Shape::Linear),
        ]);
        let raw = [(q(1), 9.0), (q(2), 9.0), (q(3), 9.0)];
        let out = stroke(&base, &raw, false, 0.001).unwrap();
        assert_eq!(
            out,
            lane(&[
                (0, 0.0, Shape::Linear),
                (1, 9.0, Shape::Linear),
                (3, 9.0, Shape::Linear),
                (4, 4.0, Shape::Linear),
            ])
        );
    }
}
