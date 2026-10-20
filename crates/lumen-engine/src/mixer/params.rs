//! Plugin parameter lanes in the mixer (M7 slice A3, ADR-0084).
//!
//! A lane of a plugin's parameter is baked to the sample clock like any other
//! ([`AutoLane`]); for each block the mixer works out the points the plugin is
//! given: the value at the block's first frame, every point of the lane inside
//! the block on its exact sample, and for a curved segment a value every
//! [`KNOT`] frames, so the plugin ramps along the curve. A straight segment
//! needs only its ends, which the plugin interpolates itself. A step is a hold
//! up to the frame before the point and the new value on it.
//!
//! What a block holds is a pure function of the lane and the block's place on
//! the timeline, so any block size gives the same curve, and a locate or a
//! loop wrap reads the same values again. Nothing is remembered between
//! blocks: the first point of every block says where the parameter is.

use lumen_core::automation::Shape;
use lumen_devices::ParamBlock;

use super::auto::{AutoLane, KNOT};

/// A lane of one parameter of a plugin, on the device's sample clock.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamLane {
    /// The plugin's id for the parameter.
    pub param: u32,
    pub lane: AutoLane,
}

/// Adds `lane`'s points for the `frames` frames from timeline sample `start`
/// to `block` as a queue. A lane that does not fit is cut down to the value
/// at each end of the block, and given up on when even that does not fit;
/// either way the block counts it.
pub fn fill(block: &mut ParamBlock, lane: &ParamLane, start: i64, frames: usize) {
    if frames == 0 {
        return;
    }
    let mark = block.mark();
    if !block.open(lane.param) {
        block.note_cut();
        return;
    }
    if walk(&lane.lane, start, frames, |at, value| {
        block.point(at, value)
    }) {
        return;
    }
    block.rewind(mark);
    block.note_cut();
    let (first, last) = (
        f64::from(lane.lane.value_at(start)),
        f64::from(lane.lane.value_at(start + frames as i64 - 1)),
    );
    let fits = block.open(lane.param)
        && block.point(0, first)
        && (frames < 2 || block.point(frames as u32 - 1, last));
    if !fits {
        block.rewind(mark);
    }
}

/// Calls `put(offset, value)` for each point of the block, in time order.
/// False when `put` refuses one.
fn walk(lane: &AutoLane, start: i64, frames: usize, mut put: impl FnMut(u32, f64) -> bool) -> bool {
    let end = start + frames as i64;
    let last = frames as u32 - 1;
    if !put(0, f64::from(lane.value_at(start))) {
        return false;
    }
    let points = lane.points();
    // The first point after the block's first frame.
    let mut next = points.partition_point(|p| p.at <= start);
    while next < points.len() && points[next].at < end {
        let b = points[next];
        let offset = (b.at - start) as u32;
        if let Some(a) = next.checked_sub(1).map(|i| points[i]) {
            match a.shape {
                Shape::Step => {
                    // Held to the frame before the point, then the jump.
                    if offset > 0 && !put(offset - 1, f64::from(a.value)) {
                        return false;
                    }
                }
                Shape::Curve(_) => {
                    if !knots(lane, a.at, start, b.at.min(end), &mut put) {
                        return false;
                    }
                }
                Shape::Linear => {}
            }
        }
        if !put(offset, f64::from(b.value)) {
            return false;
        }
        next += 1;
    }
    // The segment in force at the block's last frame, if it is still ramping
    // when the block ends: the plugin is told where it has got to.
    if let Some(a) = next.checked_sub(1).map(|i| points[i])
        && points.get(next).is_some()
        && a.shape != Shape::Step
    {
        if matches!(a.shape, Shape::Curve(_)) && !knots(lane, a.at, start, end, &mut put) {
            return false;
        }
        if !put(last, f64::from(lane.value_at(end - 1))) {
            return false;
        }
    }
    true
}

/// The value every [`KNOT`] frames along the curved segment that leaves the
/// point at `from`, after sample `start` and before `before`.
fn knots(
    lane: &AutoLane,
    from: i64,
    start: i64,
    before: i64,
    put: &mut impl FnMut(u32, f64) -> bool,
) -> bool {
    let first = from + KNOT * ((start.max(from) - from) / KNOT + 1);
    let mut at = first;
    while at < before {
        if !put((at - start) as u32, f64::from(lane.value_at(at))) {
            return false;
        }
        at += KNOT;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::auto::AutoPoint;
    use lumen_core::curve::Curve;
    use lumen_devices::{MOST_POINTS, MOST_QUEUES};

    /// A lane's value as the block holds it.
    fn v(value: f32) -> f64 {
        f64::from(value)
    }

    fn lane(points: &[(i64, f32, Shape)]) -> ParamLane {
        ParamLane {
            param: 7,
            lane: AutoLane::new(
                points
                    .iter()
                    .map(|&(at, value, shape)| AutoPoint { at, value, shape })
                    .collect(),
            )
            .unwrap(),
        }
    }

    fn block(lane: &ParamLane, start: i64, frames: usize) -> Vec<(u32, f64)> {
        let mut b = ParamBlock::new();
        b.start(frames);
        fill(&mut b, lane, start, frames);
        assert!(b.len() <= 1);
        if b.is_empty() {
            return Vec::new();
        }
        assert_eq!(b.queue(0).0, 7);
        b.queue(0).1.to_vec()
    }

    /// The value the plugin would hold at `frame` of the block if it holds a
    /// point's value from its offset and ramps between points.
    fn held(points: &[(u32, f64)], frame: u32) -> f64 {
        let after = points.partition_point(|p| p.0 <= frame);
        let Some(before) = after.checked_sub(1) else {
            return points[0].1;
        };
        let (at, value) = points[before];
        match points.get(after) {
            Some(&(next_at, next)) if next_at > at => {
                value + (next - value) * f64::from(frame - at) / f64::from(next_at - at)
            }
            _ => value,
        }
    }

    #[test]
    fn a_flat_lane_says_where_the_parameter_is_at_the_start_of_each_block() {
        let l = lane(&[(0, 0.25, Shape::Step)]);
        assert_eq!(block(&l, 5_000, 128), vec![(0, v(0.25))]);
    }

    #[test]
    fn a_step_holds_to_the_frame_before_its_point_and_jumps_on_it() {
        let l = lane(&[(0, 0.2, Shape::Step), (1000, 0.9, Shape::Step)]);
        // The point is at offset 40 of this block.
        let points = block(&l, 960, 128);
        assert_eq!(points, vec![(0, v(0.2)), (39, v(0.2)), (40, v(0.9))]);
        assert_eq!(held(&points, 39), v(0.2));
        assert_eq!(held(&points, 40), v(0.9));
    }

    #[test]
    fn a_step_on_the_first_frame_is_the_value_at_the_start() {
        let l = lane(&[(0, 0.2, Shape::Step), (1000, 0.9, Shape::Step)]);
        assert_eq!(block(&l, 1000, 128), vec![(0, v(0.9))]);
    }

    #[test]
    fn a_straight_segment_is_its_ends_and_where_it_has_got_to_at_the_end_of_a_block() {
        let l = lane(&[(0, 0.0, Shape::Linear), (1000, 1.0, Shape::Linear)]);
        let points = block(&l, 200, 100);
        assert_eq!(points.len(), 2);
        assert!((points[0].1 - 0.2).abs() < 1e-6, "{points:?}");
        assert_eq!(points[0].0, 0);
        assert_eq!(points[1].0, 99);
        assert!((points[1].1 - 0.299).abs() < 1e-6, "{points:?}");
    }

    #[test]
    fn a_straight_segment_that_ends_in_the_block_reaches_its_point_on_its_sample() {
        let l = lane(&[(0, 0.0, Shape::Linear), (1000, 1.0, Shape::Step)]);
        let points = block(&l, 950, 100);
        assert!((points[0].1 - 0.95).abs() < 1e-6, "{points:?}");
        assert_eq!(points.last(), Some(&(50, 1.0)));
    }

    #[test]
    fn a_curve_follows_the_lane_to_within_a_thousandth() {
        for curve in [Curve::SLOW, Curve::FAST, Curve::S_CURVE] {
            let l = lane(&[(0, 0.0, Shape::Curve(curve)), (4800, 1.0, Shape::Linear)]);
            let mut worst: f64 = 0.0;
            let mut start = 0;
            while start < 4800 {
                let points = block(&l, start, 256.min(4800 - start as usize));
                for frame in 0..256.min(4800 - start as usize) as u32 {
                    let want = f64::from(l.lane.value_at(start + i64::from(frame)));
                    worst = worst.max((held(&points, frame) - want).abs());
                }
                start += 256;
            }
            assert!(worst < 1e-3, "{curve:?}: {worst}");
        }
    }

    #[test]
    fn the_same_curve_at_any_block_size() {
        let l = lane(&[
            (50, 0.1, Shape::Curve(Curve::S_CURVE)),
            (900, 0.8, Shape::Linear),
            (1500, 0.3, Shape::Step),
            (1800, 0.6, Shape::Step),
        ]);
        let truth: Vec<f64> = (0..2000).map(|k| f64::from(l.lane.value_at(k))).collect();
        for size in [1usize, 7, 32, 33, 64, 128, 256] {
            let mut start = 0;
            while start < 2000 {
                let n = size.min(2000 - start);
                let points = block(&l, start as i64, n);
                for frame in 0..n {
                    let got = held(&points, frame as u32);
                    let want = truth[start + frame];
                    assert!(
                        (got - want).abs() < 2e-3,
                        "size {size} at {}: {got} against {want}",
                        start + frame
                    );
                }
                start += n;
            }
        }
    }

    #[test]
    fn before_the_first_point_the_first_value_is_held() {
        let l = lane(&[(5000, 0.7, Shape::Linear), (6000, 0.1, Shape::Linear)]);
        assert_eq!(block(&l, 0, 128), vec![(0, v(0.7))]);
    }

    #[test]
    fn a_block_with_no_frames_adds_nothing() {
        let l = lane(&[(0, 0.5, Shape::Step)]);
        assert!(block(&l, 0, 0).is_empty());
    }

    #[test]
    fn a_lane_too_busy_for_the_block_is_cut_to_its_ends_and_counted() {
        // A step on every frame cannot fit the points a block holds.
        let wild: Vec<(i64, f32, Shape)> = (0..1500)
            .map(|k| (i64::from(k), (k % 2) as f32, Shape::Step))
            .collect();
        let l = lane(&wild);
        let mut b = ParamBlock::new();
        b.start(1500);
        fill(&mut b, &l, 0, 1500);
        assert_eq!(b.len(), 1);
        assert_eq!(b.queue(0).1.len(), 2, "cut to its ends");
        assert_eq!(b.cut(), 1);
    }

    #[test]
    fn lanes_past_the_queues_a_block_holds_are_given_up_on_and_counted() {
        let l = lane(&[(0, 0.5, Shape::Step)]);
        let mut b = ParamBlock::new();
        b.start(64);
        for k in 0..MOST_QUEUES as u32 + 3 {
            fill(
                &mut b,
                &ParamLane {
                    param: k,
                    lane: l.lane.clone(),
                },
                0,
                64,
            );
        }
        assert_eq!(b.len(), MOST_QUEUES);
        assert_eq!(b.cut(), 3);
    }

    #[test]
    fn the_busiest_real_lanes_fit_the_region() {
        // 64 curved lanes at the longest block a plugin is given: each is a
        // start, a value every 32 frames and an end.
        let l = lane(&[
            (0, 0.0, Shape::Curve(Curve::S_CURVE)),
            (48_000, 1.0, Shape::Linear),
        ]);
        let mut b = ParamBlock::new();
        b.start(256);
        for k in 0..MOST_QUEUES as u32 {
            fill(
                &mut b,
                &ParamLane {
                    param: k,
                    lane: l.lane.clone(),
                },
                1_000,
                256,
            );
        }
        let points: usize = (0..b.len()).map(|i| b.queue(i).1.len()).sum();
        assert_eq!(b.cut(), 0);
        assert!(points < MOST_POINTS, "{points} of {MOST_POINTS}");
    }
}
