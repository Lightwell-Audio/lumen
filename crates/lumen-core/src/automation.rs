//! Automation lanes (M7 slice A0, D1): a value that changes along the
//! timeline, as points joined by segments.
//!
//! A [`Point`] holds a position in ticks, a value in the target's own units
//! and the [`Shape`] of the segment that leaves it, so a lane reads left to
//! right: each point says how the value gets to the next one. Segment shapes
//! are the same [`Curve`]s fades use. Before the first point the lane holds
//! the first value, and after the last it holds the last.
//!
//! This module is pure: the engine plays a lane baked to samples (the
//! `lumen-engine` mixer), and the interface reads [`Lane::value_at`] to show
//! a control where the lane puts it. Both use [`Shape::between`], so they
//! agree.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::time::Ticks;

/// A level lane's value at or below this is silence, in dB. Fader, trim and
/// send lanes run from here up, so a fade to silence ends silent and a lane
/// is always a finite number in a file.
pub const FLOOR_DB: f32 = -100.0;

/// The most points one lane holds. A crafted file past it is refused.
pub const MAX_POINTS: usize = 1 << 16;

/// How a segment gets from its point's value to the next point's.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Holds the point's value, then jumps at the next point.
    Step,
    /// A straight line, in the target's own units.
    #[default]
    Linear,
    /// Along a [`Curve`] between the two values.
    Curve(Curve),
}

impl Shape {
    /// The value `x` of the way (0 to 1) from `from` to `to` along a segment
    /// of this shape.
    pub fn between(self, from: f32, to: f32, x: f32) -> f32 {
        match self {
            Shape::Step => from,
            Shape::Linear => from + (to - from) * x,
            Shape::Curve(curve) => from + (to - from) * curve.at(x),
        }
    }

    fn is_linear(&self) -> bool {
        *self == Shape::Linear
    }
}

/// One point of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub at: Ticks,
    pub value: f32,
    /// The segment leaving this point. Written only when not linear.
    #[serde(default, skip_serializing_if = "Shape::is_linear")]
    pub shape: Shape,
}

impl Point {
    /// A point whose segment to the next is a straight line.
    pub fn new(at: Ticks, value: f32) -> Self {
        Self {
            at,
            value,
            shape: Shape::Linear,
        }
    }

    /// The same point, leaving along `shape`.
    pub fn with_shape(self, shape: Shape) -> Self {
        Self { shape, ..self }
    }
}

/// Why a list of points is not a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneError {
    /// A value or curve control point is not a finite number, or a curve
    /// leaves its square.
    BadValue { index: usize },
    /// Two points share a position, or one comes before the one before it.
    NotIncreasing { index: usize },
    /// More than [`MAX_POINTS`].
    TooMany,
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadValue { index } => write!(f, "point {index} holds a value that is not valid"),
            Self::NotIncreasing { index } => {
                write!(f, "point {index} is not after the point before it")
            }
            Self::TooMany => write!(f, "a lane holds at most {MAX_POINTS} points"),
        }
    }
}

impl std::error::Error for LaneError {}

/// Points in time order, one to a position. Saved as the list of points.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(try_from = "Vec<Point>", into = "Vec<Point>")]
pub struct Lane {
    points: Vec<Point>,
}

impl TryFrom<Vec<Point>> for Lane {
    type Error = LaneError;

    fn try_from(points: Vec<Point>) -> Result<Self, LaneError> {
        Lane::new(points)
    }
}

impl From<Lane> for Vec<Point> {
    fn from(lane: Lane) -> Self {
        lane.points
    }
}

impl Lane {
    /// A lane of `points`, which must be in time order with one to a
    /// position and hold only finite numbers.
    pub fn new(points: Vec<Point>) -> Result<Self, LaneError> {
        if points.len() > MAX_POINTS {
            return Err(LaneError::TooMany);
        }
        for (index, point) in points.iter().enumerate() {
            let curve_ok = match point.shape {
                Shape::Curve(curve) => curve.is_valid(),
                _ => true,
            };
            if !point.value.is_finite() || !curve_ok {
                return Err(LaneError::BadValue { index });
            }
            if index > 0 && point.at <= points[index - 1].at {
                return Err(LaneError::NotIncreasing { index });
            }
        }
        Ok(Self { points })
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// The lane's value at `t`; `None` for a lane with no points.
    pub fn value_at(&self, t: Ticks) -> Option<f32> {
        let after = self.points.partition_point(|p| p.at <= t);
        let Some(&before) = after.checked_sub(1).and_then(|i| self.points.get(i)) else {
            return self.points.first().map(|p| p.value);
        };
        let Some(&next) = self.points.get(after) else {
            return Some(before.value);
        };
        let x = (t.0 - before.at.0) as f64 / (next.at.0 - before.at.0) as f64;
        Some(before.shape.between(before.value, next.value, x as f32))
    }

    /// The lane with every point that a straight line between its
    /// neighbours replaces to within `tolerance` (in the lane's own units)
    /// taken out, for a freehand drawing or a recorded move. Only runs of
    /// straight segments are thinned; a step or a curve and the points
    /// either side of it stay, and so do the first and last point. The
    /// result is never further than `tolerance` from this lane anywhere.
    pub fn simplify(&self, tolerance: f32) -> Lane {
        let points = &self.points;
        let n = points.len();
        let mut keep = vec![true; n];
        let mut start = 0;
        while start < n {
            let mut end = start;
            while end + 1 < n && points[end].shape == Shape::Linear {
                end += 1;
            }
            if end > start + 1 {
                thin(points, start, end, f64::from(tolerance.max(0.0)), &mut keep);
            }
            start = end + 1;
        }
        Lane {
            points: points
                .iter()
                .zip(&keep)
                .filter_map(|(p, &keep)| keep.then_some(*p))
                .collect(),
        }
    }
}

/// Ramer–Douglas–Peucker over `points[from..=to]`, all joined by straight
/// segments: clears `keep` for the points between that the chord replaces
/// within `tolerance`. The deviation is measured upright, at the point's own
/// time, which is exactly how far the two lines are apart there; two lines
/// of straight pieces are furthest apart at a piece's end, so no time
/// between is further than the points checked.
fn thin(points: &[Point], from: usize, to: usize, tolerance: f64, keep: &mut [bool]) {
    let mut ranges = vec![(from, to)];
    while let Some((a, b)) = ranges.pop() {
        if b <= a + 1 {
            continue;
        }
        let (p, q) = (points[a], points[b]);
        let span = (q.at.0 - p.at.0) as f64;
        let mut worst = (0.0f64, a);
        for (i, point) in points.iter().enumerate().take(b).skip(a + 1) {
            let x = (point.at.0 - p.at.0) as f64 / span;
            let chord = f64::from(p.value) + (f64::from(q.value) - f64::from(p.value)) * x;
            let off = (f64::from(point.value) - chord).abs();
            if off > worst.0 {
                worst = (off, i);
            }
        }
        if worst.0 > tolerance {
            ranges.push((a, worst.1));
            ranges.push((worst.1, b));
        } else {
            keep[a + 1..b].fill(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(quarters: i64) -> Ticks {
        Ticks::quarters(quarters)
    }

    fn lane(points: &[(i64, f32, Shape)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(t, v, s)| Point::new(at(t), v).with_shape(s))
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn an_empty_lane_has_no_value() {
        assert_eq!(Lane::default().value_at(at(3)), None);
    }

    #[test]
    fn it_holds_its_first_value_before_and_its_last_after() {
        let l = lane(&[(4, -6.0, Shape::Linear), (8, 0.0, Shape::Linear)]);
        assert_eq!(l.value_at(at(0)), Some(-6.0));
        assert_eq!(l.value_at(at(4)), Some(-6.0));
        assert_eq!(l.value_at(at(8)), Some(0.0));
        assert_eq!(l.value_at(at(100)), Some(0.0));
    }

    #[test]
    fn a_linear_segment_is_a_straight_line_in_the_targets_units() {
        let l = lane(&[(0, -20.0, Shape::Linear), (4, 0.0, Shape::Linear)]);
        assert_eq!(l.value_at(at(2)), Some(-10.0));
        assert_eq!(l.value_at(Ticks(at(1).0)), Some(-15.0));
    }

    #[test]
    fn a_step_holds_until_the_next_point_and_then_jumps() {
        let l = lane(&[(0, 0.0, Shape::Step), (4, 1.0, Shape::Linear)]);
        assert_eq!(l.value_at(at(2)), Some(0.0));
        assert_eq!(l.value_at(Ticks(at(4).0 - 1)), Some(0.0));
        assert_eq!(l.value_at(at(4)), Some(1.0));
    }

    #[test]
    fn a_curve_segment_follows_the_curve_between_the_two_values() {
        let l = lane(&[(0, 0.0, Shape::Curve(Curve::SLOW)), (4, 8.0, Shape::Linear)]);
        let mid = l.value_at(at(2)).unwrap();
        assert!((mid - 8.0 * Curve::SLOW.at(0.5)).abs() < 1e-5);
        assert!(mid < 4.0, "slow at first: {mid}");
    }

    #[test]
    fn points_must_come_in_time_order_one_to_a_position() {
        let p = |t, v| Point::new(at(t), v);
        assert_eq!(
            Lane::new(vec![p(1, 0.0), p(1, 1.0)]),
            Err(LaneError::NotIncreasing { index: 1 })
        );
        assert_eq!(
            Lane::new(vec![p(2, 0.0), p(1, 1.0)]),
            Err(LaneError::NotIncreasing { index: 1 })
        );
        assert_eq!(
            Lane::new(vec![p(0, f32::NAN)]),
            Err(LaneError::BadValue { index: 0 })
        );
        assert_eq!(
            Lane::new(vec![p(0, 0.0).with_shape(Shape::Curve(Curve {
                p1: [0.9, 0.0],
                p2: [0.1, 1.0],
            }))]),
            Err(LaneError::BadValue { index: 0 })
        );
    }

    #[test]
    fn a_lane_is_saved_as_its_points_and_a_bad_file_is_refused() {
        let l = lane(&[
            (0, -6.0, Shape::Step),
            (4, 0.0, Shape::Linear),
            (8, 3.0, Shape::Curve(Curve::FAST)),
            (12, 0.0, Shape::Linear),
        ]);
        let text = serde_json::to_string(&l).unwrap();
        assert!(text.starts_with('['), "{text}");
        assert!(
            text.contains("\"step\"") && text.contains("\"curve\""),
            "{text}"
        );
        // A linear point carries no shape.
        assert_eq!(text.matches("shape").count(), 2, "{text}");
        assert_eq!(serde_json::from_str::<Lane>(&text).unwrap(), l);
        let backwards = r#"[{"at":5,"value":0.0},{"at":2,"value":1.0}]"#;
        assert!(serde_json::from_str::<Lane>(backwards).is_err());
    }

    /// A small deterministic generator, so a failing case can be replayed.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn unit(&mut self) -> f32 {
            (self.next() % 10_000) as f32 / 10_000.0
        }
    }

    fn random_lane(rng: &mut Rng, linear_only: bool) -> Lane {
        let n = 2 + (rng.next() % 40) as usize;
        let mut t = 0i64;
        let mut points = Vec::new();
        for _ in 0..n {
            t += 1 + (rng.next() % 5_000_000) as i64;
            let shape = if linear_only {
                Shape::Linear
            } else {
                match rng.next() % 4 {
                    0 => Shape::Step,
                    1 => Shape::Curve(Curve::S_CURVE),
                    _ => Shape::Linear,
                }
            };
            points.push(Point::new(Ticks(t), rng.unit() * 40.0 - 30.0).with_shape(shape));
        }
        Lane::new(points).unwrap()
    }

    #[test]
    fn simplifying_never_moves_the_lane_further_than_the_tolerance() {
        let mut rng = Rng(7);
        for case in 0..200 {
            // Dense freehand-like lanes thin the most.
            let mut points = Vec::new();
            let mut v = 0.0f32;
            for k in 0..(50 + rng.next() % 200) as i64 {
                v += rng.unit() - 0.5;
                points.push(Point::new(Ticks(k * 40_000), v));
            }
            let l = Lane::new(points).unwrap();
            let tolerance = 0.05 + rng.unit();
            let thin = l.simplify(tolerance);
            assert!(thin.len() <= l.len());
            assert_eq!(thin.points().first(), l.points().first());
            assert_eq!(thin.points().last(), l.points().last());
            let end = l.points().last().unwrap().at.0;
            for k in 0..=2_000 {
                let t = Ticks(end * k / 2_000);
                let (a, b) = (l.value_at(t).unwrap(), thin.value_at(t).unwrap());
                assert!(
                    (a - b).abs() <= tolerance + 1e-4,
                    "case {case} at {t:?}: {a} against {b}, tolerance {tolerance}"
                );
            }
        }
    }

    #[test]
    fn simplifying_keeps_steps_curves_and_their_neighbours() {
        let mut rng = Rng(11);
        for _ in 0..100 {
            let l = random_lane(&mut rng, false);
            let thin = l.simplify(1.0);
            // The shape of every segment that is not straight survives, and
            // so does the point it ends on.
            for (i, p) in l.points().iter().enumerate() {
                if p.shape != Shape::Linear {
                    assert!(thin.points().contains(p), "kept {i}");
                    if let Some(next) = l.points().get(i + 1) {
                        assert!(thin.points().contains(next), "kept {}", i + 1);
                    }
                }
            }
        }
    }

    #[test]
    fn a_straight_line_of_many_points_is_two() {
        let points = (0..100)
            .map(|k| Point::new(Ticks(k * 1000), k as f32 * 0.25))
            .collect();
        let l = Lane::new(points).unwrap();
        assert_eq!(l.simplify(0.001).len(), 2);
        assert_eq!(
            l.simplify(1e-9).len(),
            2,
            "only rounding is left to forgive"
        );
    }

    #[test]
    fn a_lane_over_the_limit_is_refused() {
        let points = (0..=MAX_POINTS as i64)
            .map(|k| Point::new(Ticks(k), 0.0))
            .collect();
        assert_eq!(Lane::new(points), Err(LaneError::TooMany));
    }
}
