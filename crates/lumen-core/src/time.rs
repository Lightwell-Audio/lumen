//! Musical time base (ADR-0009).
//!
//! Positions are integer [`Ticks`] at [`PPQ`] ticks per quarter note. The
//! resolution is highly composite, so grid values (straight, dotted, triplet,
//! quintuplet, septuplet) are exact integers, and fine enough that every
//! audio sample is addressable up to 999 BPM at 192 kHz. Conversion to time
//! goes through a [`TempoMap`] with one rounding rule:
//! [`TempoMap::sample_at`] rounds to the nearest sample, ties away from zero.
//! A boundary shared by two clips is one tick value, so both sides always map
//! to the same sample.

use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// Ticks per quarter note: 2⁷ · 3² · 5² · 7².
pub const PPQ: i64 = 1_411_200;

pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 999.0;

/// A musical position or duration.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Ticks(pub i64);

impl Ticks {
    pub const ZERO: Ticks = Ticks(0);

    /// `quarters` quarter notes.
    pub const fn quarters(quarters: i64) -> Self {
        Ticks(quarters * PPQ)
    }

    /// `numerator / denominator` of a whole note, if exactly representable.
    /// For example `(1, 12)` is an eighth-note triplet.
    pub fn whole_note_fraction(numerator: i64, denominator: i64) -> Option<Self> {
        if denominator <= 0 {
            return None;
        }
        let scaled = 4 * PPQ * numerator;
        (scaled % denominator == 0).then(|| Ticks(scaled / denominator))
    }

    pub fn as_quarters(self) -> f64 {
        self.0 as f64 / PPQ as f64
    }
}

impl Add for Ticks {
    type Output = Ticks;
    fn add(self, rhs: Ticks) -> Ticks {
        Ticks(self.0 + rhs.0)
    }
}

impl Sub for Ticks {
    type Output = Ticks;
    fn sub(self, rhs: Ticks) -> Ticks {
        Ticks(self.0 - rhs.0)
    }
}

/// Tempo from `at` until the next point (constant segments; ramps come later).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub at: Ticks,
    pub bpm: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TempoError {
    Empty,
    FirstPointNotAtZero,
    NotIncreasing { index: usize },
    BpmOutOfRange { index: usize },
}

impl std::fmt::Display for TempoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TempoError::Empty => write!(f, "the tempo map is empty"),
            TempoError::FirstPointNotAtZero => write!(f, "the first tempo point must be at 0"),
            TempoError::NotIncreasing { index } => {
                write!(f, "tempo point {index} is not after the previous one")
            }
            TempoError::BpmOutOfRange { index } => {
                write!(f, "tempo point {index} is outside {MIN_BPM}–{MAX_BPM} BPM")
            }
        }
    }
}

impl std::error::Error for TempoError {}

/// Maps ticks to seconds and samples.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    points: Vec<TempoPoint>,
    /// Seconds at each point, precomputed for O(log n) lookups.
    seconds: Vec<f64>,
}

impl TempoMap {
    pub fn constant(bpm: f64) -> Result<Self, TempoError> {
        Self::new(vec![TempoPoint {
            at: Ticks::ZERO,
            bpm,
        }])
    }

    pub fn new(points: Vec<TempoPoint>) -> Result<Self, TempoError> {
        let first = points.first().ok_or(TempoError::Empty)?;
        if first.at != Ticks::ZERO {
            return Err(TempoError::FirstPointNotAtZero);
        }
        let mut seconds = Vec::with_capacity(points.len());
        let mut elapsed = 0.0;
        for (index, point) in points.iter().enumerate() {
            if !(MIN_BPM..=MAX_BPM).contains(&point.bpm) {
                return Err(TempoError::BpmOutOfRange { index });
            }
            if index > 0 {
                let previous = points[index - 1];
                if point.at <= previous.at {
                    return Err(TempoError::NotIncreasing { index });
                }
                elapsed += segment_seconds(point.at - previous.at, previous.bpm);
            }
            seconds.push(elapsed);
        }
        Ok(Self { points, seconds })
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    /// Seconds from the start of the timeline. Before zero the first tempo
    /// is extrapolated.
    pub fn seconds_at(&self, t: Ticks) -> f64 {
        let i = self.points.partition_point(|p| p.at <= t).saturating_sub(1);
        let point = self.points[i];
        self.seconds[i] + segment_seconds(t - point.at, point.bpm)
    }

    /// The sample at `t`: nearest sample, ties away from zero.
    pub fn sample_at(&self, t: Ticks, sample_rate: u32) -> i64 {
        (self.seconds_at(t) * f64::from(sample_rate)).round() as i64
    }

    /// The tick nearest to `seconds`.
    pub fn ticks_at_seconds(&self, seconds: f64) -> Ticks {
        let i = self
            .seconds
            .partition_point(|&s| s <= seconds)
            .saturating_sub(1);
        let point = self.points[i];
        let ticks = (seconds - self.seconds[i]) * point.bpm / 60.0 * PPQ as f64;
        point.at + Ticks(ticks.round() as i64)
    }

    /// The tick nearest to the start of `sample`.
    pub fn ticks_at_sample(&self, sample: i64, sample_rate: u32) -> Ticks {
        self.ticks_at_seconds(sample as f64 / f64::from(sample_rate))
    }
}

fn segment_seconds(duration: Ticks, bpm: f64) -> f64 {
    duration.0 as f64 / PPQ as f64 * 60.0 / bpm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grid_value_down_to_the_finest_is_an_exact_integer() {
        // Straight values down to 1/512.
        for denominator in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512] {
            assert!(
                Ticks::whole_note_fraction(1, denominator).is_some(),
                "1/{denominator}"
            );
        }
        for denominator in [1, 2, 4, 8, 16, 32, 64, 128, 256] {
            // Dotted.
            let dotted = Ticks::whole_note_fraction(3, denominator * 2);
            assert!(dotted.is_some(), "dotted 1/{denominator}");
            // Triplets, quintuplets, septuplets, nonuplets.
            for (play, in_time_of) in [(3, 2), (5, 4), (7, 4), (9, 8)] {
                let t = Ticks::whole_note_fraction(in_time_of, denominator * play);
                assert!(t.is_some(), "{play}:{in_time_of} tuplet of 1/{denominator}");
            }
        }
        assert_eq!(Ticks::whole_note_fraction(1, 12), Some(Ticks(PPQ / 3)));
        assert_eq!(
            Ticks::whole_note_fraction(1, 1024),
            None,
            "documented limit"
        );
    }

    #[test]
    fn constant_tempo_conversions_are_exact() {
        let map = TempoMap::constant(120.0).unwrap();
        assert_eq!(map.sample_at(Ticks::quarters(1), 48_000), 24_000);
        assert_eq!(map.sample_at(Ticks(PPQ / 3), 48_000), 8_000); // eighth triplet
        assert_eq!(map.sample_at(Ticks::quarters(-1), 48_000), -24_000);
    }

    #[test]
    fn tempo_changes_accumulate() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
            },
            TempoPoint {
                at: Ticks::quarters(4),
                bpm: 60.0,
            },
        ])
        .unwrap();
        assert_eq!(map.seconds_at(Ticks::quarters(4)), 2.0);
        assert_eq!(map.seconds_at(Ticks::quarters(5)), 3.0);
        assert_eq!(map.ticks_at_seconds(3.0), Ticks::quarters(5));
    }

    #[test]
    fn every_sample_is_addressable_at_the_extremes() {
        let map = TempoMap::constant(MAX_BPM).unwrap();
        let rate = 192_000;
        for sample in (0..2_000_000).step_by(997) {
            let t = map.ticks_at_sample(sample, rate);
            assert_eq!(map.sample_at(t, rate), sample, "sample {sample}");
        }
    }

    #[test]
    fn seconds_round_trip_to_the_same_tick() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 97.3,
            },
            TempoPoint {
                at: Ticks::quarters(33),
                bpm: 174.0,
            },
            TempoPoint {
                at: Ticks::quarters(70),
                bpm: 20.0,
            },
        ])
        .unwrap();
        let mut x = 0x2545_F491_4F6C_DD1D_u64;
        for _ in 0..10_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let t = Ticks((x % (PPQ as u64 * 100_000)) as i64);
            assert_eq!(map.ticks_at_seconds(map.seconds_at(t)), t);
        }
    }

    #[test]
    fn samples_never_go_backwards() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 140.0,
            },
            TempoPoint {
                at: Ticks(1_234_567),
                bpm: 33.0,
            },
        ])
        .unwrap();
        let mut previous = i64::MIN;
        for t in (-PPQ..PPQ * 64).step_by(4_999) {
            let s = map.sample_at(Ticks(t), 44_100);
            assert!(s >= previous);
            previous = s;
        }
    }

    #[test]
    fn invalid_maps_are_rejected() {
        assert_eq!(TempoMap::new(vec![]), Err(TempoError::Empty));
        let p = |at, bpm| TempoPoint { at: Ticks(at), bpm };
        assert_eq!(
            TempoMap::new(vec![p(5, 120.0)]),
            Err(TempoError::FirstPointNotAtZero)
        );
        assert_eq!(
            TempoMap::new(vec![p(0, 1_000.0)]),
            Err(TempoError::BpmOutOfRange { index: 0 })
        );
        assert_eq!(
            TempoMap::new(vec![p(0, 120.0), p(0, 90.0)]),
            Err(TempoError::NotIncreasing { index: 1 })
        );
    }
}
