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

use lumen_core::automation::{FLOOR_DB, Lane, Point};
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
    pub fn shift(&mut self, by: lumen_core::time::Ticks) {
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
