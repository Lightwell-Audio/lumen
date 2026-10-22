//! Envelopes an audio clip carries (format v25, M7 slice A7): a gain and a
//! pan that follow the clip, looping with it.
//!
//! An envelope is a [`Lane`] whose points are measured from the clip's start,
//! as a MIDI clip's notes are (ADR-0056): a move carries them, a trim moves
//! them with the clip's start so they stay where they were on the timeline,
//! and a point outside the clip's window is kept, hidden. Slipping the audio
//! leaves them where they are. A looping clip repeats its envelope every
//! `loop_length`, from the first repeat.
//!
//! Gain is in dB and adds to the clip's own gain; pan is −1 to 1 and pans
//! the clip before its track does.

use lumen_core::automation::{FLOOR_DB, Lane, Point, Shape};
use lumen_core::time::Ticks;
use serde::{Deserialize, Serialize};

use crate::automation::LEVEL_MAX_DB;
use crate::model::Clip;

/// What an envelope drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// The clip's level, in dB, added to its gain.
    Gain,
    /// Pan, −1 (left) to +1 (right).
    Pan,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Gain, Kind::Pan];

    /// The lowest and highest value an envelope of this kind may hold.
    pub fn range(self) -> (f32, f32) {
        match self {
            Kind::Gain => (FLOOR_DB, LEVEL_MAX_DB),
            Kind::Pan => (-1.0, 1.0),
        }
    }

    /// The value that changes nothing.
    pub fn rest(self) -> f32 {
        0.0
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Gain => "Clip gain",
            Kind::Pan => "Clip pan",
        }
    }
}

/// The envelopes of one clip. A kind with no points is `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipEnvelopes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain: Option<Lane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<Lane>,
}

impl ClipEnvelopes {
    pub fn is_empty(&self) -> bool {
        self.gain.is_none() && self.pan.is_none()
    }

    pub fn get(&self, kind: Kind) -> Option<&Lane> {
        match kind {
            Kind::Gain => self.gain.as_ref(),
            Kind::Pan => self.pan.as_ref(),
        }
    }

    /// Puts `lane` as the envelope of `kind`; a lane with no points takes the
    /// envelope away.
    pub fn set(&mut self, kind: Kind, lane: Lane) {
        let slot = match kind {
            Kind::Gain => &mut self.gain,
            Kind::Pan => &mut self.pan,
        };
        *slot = (!lane.is_empty()).then_some(lane);
    }

    /// The envelopes that are there, with their kind.
    pub fn iter(&self) -> impl Iterator<Item = (Kind, &Lane)> {
        Kind::ALL
            .into_iter()
            .filter_map(|kind| self.get(kind).map(|lane| (kind, lane)))
    }

    /// Moves every point by `by` (earlier when negative), as a trim of the
    /// clip's start does.
    pub fn shift(&mut self, by: Ticks) {
        for lane in [&mut self.gain, &mut self.pan].into_iter().flatten() {
            let points = lane
                .points()
                .iter()
                .map(|p| Point {
                    at: p.at + by,
                    ..*p
                })
                .collect();
            // Every point moves by the same amount, so the order holds.
            if let Ok(moved) = Lane::new(points) {
                *lane = moved;
            }
        }
    }
}

/// How long an envelope plays before it repeats or the clip ends: the clip's
/// loop, or its length.
pub fn window(clip: &Clip) -> Ticks {
    clip.loop_length
        .filter(|l| l.0 > 0)
        .unwrap_or(clip.length)
        .min(clip.length)
}

/// How many straight pieces a curved segment is cut into where a window
/// cuts it (a curve cannot start or end partway along itself).
const CURVE_PIECES: i64 = 16;

/// The part of `lane` from clip time 0 to `end`, for what plays: the points
/// inside the window, and where the window cuts a segment, a point at the cut
/// with the value the lane has there. A curved segment that is cut becomes a
/// run of straight ones. A lane that lies wholly outside holds one value.
/// `None` when the lane has no points.
pub fn windowed(lane: &Lane, end: Ticks) -> Option<Lane> {
    let points = lane.points();
    if points.is_empty() {
        return None;
    }
    let end = end.max(Ticks::ZERO);
    let mut out: Vec<Point> = Vec::new();
    fn put(out: &mut Vec<Point>, at: Ticks, value: f32, shape: Shape) {
        if out.last().is_none_or(|last| last.at < at) {
            out.push(Point { at, value, shape });
        }
    }
    let value_at = |t: Ticks| lane.value_at(t).unwrap_or(0.0);
    // Holds the first value before the first point and the last after it.
    if points[0].at >= Ticks::ZERO {
        // Nothing is cut at the start.
    } else {
        let next = points.iter().find(|p| p.at >= Ticks::ZERO);
        let before = points.iter().rev().find(|p| p.at < Ticks::ZERO);
        match (before, next) {
            (Some(a), Some(b)) => {
                let (from, to) = (Ticks::ZERO, b.at.min(end));
                if matches!(a.shape, Shape::Curve(_)) {
                    for k in 0..CURVE_PIECES {
                        let t = from + Ticks((to - from).0 * k / CURVE_PIECES);
                        put(&mut out, t, value_at(t), Shape::Linear);
                    }
                } else {
                    put(&mut out, from, value_at(from), a.shape);
                }
            }
            (Some(a), None) => put(&mut out, Ticks::ZERO, a.value, Shape::Linear),
            _ => {}
        }
    }
    for (i, a) in points.iter().enumerate() {
        if a.at < Ticks::ZERO || a.at > end {
            continue;
        }
        let next = points.get(i + 1);
        match next {
            Some(b) if b.at > end && matches!(a.shape, Shape::Curve(_)) => {
                for k in 0..CURVE_PIECES {
                    let t = a.at + Ticks((end - a.at).0 * k / CURVE_PIECES);
                    put(&mut out, t, value_at(t), Shape::Linear);
                }
                put(&mut out, end, value_at(end), Shape::Linear);
            }
            Some(b) if b.at > end => {
                put(&mut out, a.at, a.value, a.shape);
                put(&mut out, end, value_at(end), Shape::Linear);
            }
            _ => put(&mut out, a.at, a.value, a.shape),
        }
    }
    if out.is_empty() {
        // Every point is past the window: the first value is held.
        put(&mut out, Ticks::ZERO, value_at(Ticks::ZERO), Shape::Linear);
    }
    Lane::new(out).ok()
}

/// Why a clip's envelopes cannot be kept.
pub(crate) fn check(clip: &Clip) -> Result<(), String> {
    if clip.envelopes.is_empty() {
        return Ok(());
    }
    if !matches!(clip.content, crate::model::ClipContent::Audio { .. }) {
        return Err("a MIDI clip holds an envelope".to_owned());
    }
    for (kind, lane) in clip.envelopes.iter() {
        let (low, high) = kind.range();
        for point in lane.points() {
            if !(low..=high).contains(&point.value) {
                return Err(format!(
                    "a {kind:?} envelope holds {} outside {low} to {high}",
                    point.value
                ));
            }
            if !crate::validate::within(point.at) {
                return Err(format!("a {kind:?} envelope has a point out of range"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use lumen_core::time::Ticks;

    use super::*;

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
                .collect(),
        )
        .unwrap()
    }

    fn values(lane: &Lane) -> Vec<(i64, f32)> {
        lane.points()
            .iter()
            .map(|p| (p.at.0 / Ticks::quarters(1).0, p.value))
            .collect()
    }

    #[test]
    fn the_window_keeps_the_points_inside_and_the_value_at_each_cut() {
        // Points at −2 (−10) and 2 (0) and 6 (10): the window 0 to 4 cuts the
        // first segment at 0 (value −5) and the second at 4 (value 5).
        let l = lane(&[(-2, -10.0), (2, 0.0), (6, 10.0)]);
        let w = windowed(&l, Ticks::quarters(4)).unwrap();
        assert_eq!(values(&w), vec![(0, -5.0), (2, 0.0), (4, 5.0)]);
    }

    #[test]
    fn a_lane_wholly_outside_the_window_holds_one_value() {
        let before = lane(&[(-4, 3.0), (-1, -3.0)]);
        let w = windowed(&before, Ticks::quarters(4)).unwrap();
        assert_eq!(values(&w), vec![(0, -3.0)]);
        let after = lane(&[(8, 2.0), (9, 4.0)]);
        let w = windowed(&after, Ticks::quarters(4)).unwrap();
        assert_eq!(values(&w), vec![(0, 2.0)]);
        assert!(windowed(&Lane::default(), Ticks::quarters(4)).is_none());
    }

    #[test]
    fn a_curved_segment_that_a_window_cuts_becomes_straight_pieces() {
        let curve = lumen_core::curve::Curve::default();
        let l = Lane::new(vec![
            Point::new(Ticks::quarters(-4), 0.0).with_shape(Shape::Curve(curve)),
            Point::new(Ticks::quarters(4), 8.0),
        ])
        .unwrap();
        let w = windowed(&l, Ticks::quarters(2)).unwrap();
        assert!(w.len() >= 16, "cut into pieces");
        assert!(w.points().iter().all(|p| p.shape == Shape::Linear));
        // Each piece's value is the lane's own value there.
        for p in w.points() {
            assert!((p.value - l.value_at(p.at).unwrap()).abs() < 1e-5);
        }
        assert_eq!(w.points()[0].at, Ticks::ZERO);
    }

    #[test]
    fn an_empty_lane_takes_the_envelope_away() {
        let mut envelopes = ClipEnvelopes::default();
        envelopes.set(Kind::Gain, lane(&[(0, -6.0)]));
        assert!(!envelopes.is_empty());
        envelopes.set(Kind::Gain, Lane::default());
        assert!(envelopes.is_empty());
    }

    #[test]
    fn each_kind_has_the_range_its_control_has() {
        assert_eq!(Kind::Gain.range(), (FLOOR_DB, 6.0));
        assert_eq!(Kind::Pan.range(), (-1.0, 1.0));
    }

    #[test]
    fn shifting_moves_every_point_and_keeps_the_rest() {
        let mut envelopes = ClipEnvelopes::default();
        envelopes.set(Kind::Gain, lane(&[(1, -6.0), (2, 0.0)]));
        envelopes.set(Kind::Pan, lane(&[(0, 1.0)]));
        envelopes.shift(Ticks::quarters(-2));
        let at: Vec<_> = envelopes
            .gain
            .as_ref()
            .unwrap()
            .points()
            .iter()
            .map(|p| p.at)
            .collect();
        assert_eq!(at, vec![Ticks::quarters(-1), Ticks::quarters(0)]);
        assert_eq!(
            envelopes.pan.as_ref().unwrap().points()[0].at,
            Ticks::quarters(-2)
        );
    }
}
