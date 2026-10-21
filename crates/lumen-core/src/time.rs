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

/// Tempo from `at` until the next point. A step holds `bpm` to the next
/// point; a ramp moves the tempo in a straight line, in music time, from
/// `bpm` to the next point's `bpm`, arriving as the next point begins. The
/// last point holds its tempo: a ramp on it has nowhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub at: Ticks,
    pub bpm: f64,
    /// Glides to the next point's tempo instead of jumping there.
    #[serde(default, skip_serializing_if = "is_step")]
    pub ramp: bool,
}

fn is_step(ramp: &bool) -> bool {
    !*ramp
}

impl TempoPoint {
    /// A tempo that holds until the next point.
    pub const fn step(at: Ticks, bpm: f64) -> Self {
        Self {
            at,
            bpm,
            ramp: false,
        }
    }
}

/// Tempos closer than this (BPM) are the same tempo: a ramp between them is
/// a step, which keeps the ramp formulas away from dividing by nothing.
const SAME_BPM: f64 = 1e-9;

/// Ticks between the steps a ramp is written as where only steps exist (a
/// MIDI file): a sixteenth note.
pub const RAMP_STEP: Ticks = Ticks(PPQ / 4);

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
    /// How each segment runs, so a lookup reads one entry.
    runs: Vec<Run>,
    /// Whether any segment ramps: a map of steps takes the short way.
    ramps: bool,
}

impl TempoMap {
    pub fn constant(bpm: f64) -> Result<Self, TempoError> {
        Self::new(vec![TempoPoint::step(Ticks::ZERO, bpm)])
    }

    pub fn new(points: Vec<TempoPoint>) -> Result<Self, TempoError> {
        let first = points.first().ok_or(TempoError::Empty)?;
        if first.at != Ticks::ZERO {
            return Err(TempoError::FirstPointNotAtZero);
        }
        let mut seconds = Vec::with_capacity(points.len());
        let mut runs: Vec<Run> = Vec::with_capacity(points.len());
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
                elapsed += runs[index - 1].seconds(point.at - previous.at);
            }
            seconds.push(elapsed);
            runs.push(Run::of(&points, index));
        }
        let ramps = runs.iter().any(|run| run.ramp.is_some());
        Ok(Self {
            points,
            seconds,
            runs,
            ramps,
        })
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    /// Seconds from the start of the timeline. Before zero the first tempo
    /// is extrapolated.
    pub fn seconds_at(&self, t: Ticks) -> f64 {
        let i = self.points.partition_point(|p| p.at <= t).saturating_sub(1);
        let point = self.points[i];
        if !self.ramps {
            return self.seconds[i] + segment_seconds(t - point.at, point.bpm);
        }
        self.seconds[i] + self.runs[i].seconds(t - point.at)
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
        let ticks = if self.ramps {
            self.runs[i].ticks(seconds - self.seconds[i])
        } else {
            (seconds - self.seconds[i]) * point.bpm / 60.0 * PPQ as f64
        };
        point.at + Ticks(ticks.round() as i64)
    }

    /// The tick nearest to the start of `sample`.
    pub fn ticks_at_sample(&self, sample: i64, sample_rate: u32) -> Ticks {
        self.ticks_at_seconds(sample as f64 / f64::from(sample_rate))
    }

    /// The tempo playing at `t`: a step's own, or the point a ramp has
    /// reached.
    pub fn bpm_at(&self, t: Ticks) -> f64 {
        bpm_at(&self.points, t)
    }

    /// Whether a ramp runs anywhere between `from` and `to`, where ticks
    /// and samples are no longer in a straight line.
    pub fn ramps_between(&self, from: Ticks, to: Ticks) -> bool {
        let first = self
            .points
            .partition_point(|p| p.at <= from)
            .saturating_sub(1);
        (first..self.points.len())
            .take_while(|&i| self.points[i].at < to)
            .any(|i| self.runs[i].ramp.is_some())
    }

    /// The map as steps only, for where only steps exist (a MIDI file): each
    /// ramp becomes a step every sixteenth note (fewer, for a very long
    /// ramp), each holding the tempo that takes exactly as long as the ramp
    /// does over that step, so the time at every step's start is the ramp's.
    pub fn stepped(&self) -> Vec<TempoPoint> {
        let mut out = Vec::with_capacity(self.points.len());
        for (i, point) in self.points.iter().enumerate() {
            let run = Run::of(&self.points, i);
            let Some(ramp) = run.ramp else {
                out.push(TempoPoint::step(point.at, point.bpm));
                continue;
            };
            let step = RAMP_STEP
                .0
                .max((ramp.length.0 + MOST_RAMP_STEPS - 1) / MOST_RAMP_STEPS);
            let mut from = 0;
            while from < ramp.length.0 {
                let to = (from + step).min(ramp.length.0);
                let seconds = run.seconds(Ticks(to)) - run.seconds(Ticks(from));
                let bpm = (to - from) as f64 / PPQ as f64 * 60.0 / seconds;
                out.push(TempoPoint::step(
                    point.at + Ticks(from),
                    bpm.clamp(MIN_BPM, MAX_BPM),
                ));
                from = to;
            }
        }
        out
    }
}

/// The tempo playing at `t` along `points` (sorted, the first at zero): a
/// step's own, or the point a ramp has reached. 120 for no points.
pub fn bpm_at(points: &[TempoPoint], t: Ticks) -> f64 {
    if points.is_empty() {
        return 120.0;
    }
    let i = points.partition_point(|p| p.at <= t).saturating_sub(1);
    Run::of(points, i).bpm(t - points[i].at)
}

fn segment_seconds(duration: Ticks, bpm: f64) -> f64 {
    duration.0 as f64 / PPQ as f64 * 60.0 / bpm
}

/// The most steps one ramp is written as.
const MOST_RAMP_STEPS: i64 = 256;

/// Where a ramp goes: the tempo it ends on and how long it takes in ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ramp {
    to: f64,
    length: Ticks,
}

/// One segment of the map: the tempo it starts at and, if it ramps, where to.
/// All the conversions between ticks and seconds inside a segment are here.
///
/// A ramp is a straight line in music time: the tempo `b` a fraction `f` of
/// the way along is `b0 + (b1 - b0) f`. A quarter note at tempo `b` lasts
/// `60 / b` seconds, so the seconds up to `f` are the integral of that, in
/// closed form: `60 L / (b1 - b0) ln(b / b0)` for a ramp `L` quarters long.
/// Going back is the same turned round: `b = b0 e^(k s)`, with
/// `k = (b1 - b0) / (60 L)`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run {
    from: f64,
    ramp: Option<Ramp>,
}

impl Run {
    fn of(points: &[TempoPoint], index: usize) -> Self {
        let point = points[index];
        let ramp = points
            .get(index + 1)
            .filter(|next| point.ramp && (next.bpm - point.bpm).abs() > SAME_BPM)
            .map(|next| Ramp {
                to: next.bpm,
                length: next.at - point.at,
            });
        Self {
            from: point.bpm,
            ramp,
        }
    }

    /// Seconds from the segment's start to `offset` ticks in. Before the
    /// start the first tempo goes on.
    fn seconds(&self, offset: Ticks) -> f64 {
        match self.ramp {
            Some(ramp) if offset.0 > 0 => {
                let (b0, rise) = (self.from, ramp.to - self.from);
                let f = offset.0 as f64 / ramp.length.0 as f64;
                let quarters = ramp.length.0 as f64 / PPQ as f64;
                60.0 * quarters / rise * (rise / b0 * f).ln_1p()
            }
            _ => offset.0 as f64 / PPQ as f64 * 60.0 / self.from,
        }
    }

    /// Ticks from the segment's start to `seconds` in.
    fn ticks(&self, seconds: f64) -> f64 {
        match self.ramp {
            Some(ramp) if seconds > 0.0 => {
                let (b0, rise) = (self.from, ramp.to - self.from);
                let quarters = ramp.length.0 as f64 / PPQ as f64;
                let k = rise / (60.0 * quarters);
                b0 * (seconds * k).exp_m1() / rise * ramp.length.0 as f64
            }
            _ => seconds * self.from / 60.0 * PPQ as f64,
        }
    }

    /// The tempo `offset` ticks in.
    fn bpm(&self, offset: Ticks) -> f64 {
        match self.ramp {
            Some(ramp) if offset.0 > 0 => {
                let f = (offset.0 as f64 / ramp.length.0 as f64).min(1.0);
                self.from + (ramp.to - self.from) * f
            }
            _ => self.from,
        }
    }
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
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(4),
                bpm: 60.0,
                ramp: false,
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
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(33),
                bpm: 174.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(70),
                bpm: 20.0,
                ramp: false,
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
                ramp: false,
            },
            TempoPoint {
                at: Ticks(1_234_567),
                bpm: 33.0,
                ramp: false,
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
        let p = |at, bpm| TempoPoint {
            at: Ticks(at),
            bpm,
            ramp: false,
        };
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

    fn ramping(points: &[(i64, f64, bool)]) -> TempoMap {
        TempoMap::new(
            points
                .iter()
                .map(|&(quarters, bpm, ramp)| TempoPoint {
                    at: Ticks::quarters(quarters),
                    bpm,
                    ramp,
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn a_ramp_takes_the_time_its_straight_line_of_tempo_takes() {
        // 120 down to 60 over four quarters: 60 L / (b1 - b0) ln(b1 / b0).
        let map = ramping(&[(0, 120.0, true), (4, 60.0, false)]);
        let expected = 60.0 * 4.0 / (60.0 - 120.0) * (0.5f64).ln();
        assert!((map.seconds_at(Ticks::quarters(4)) - expected).abs() < 1e-12);
        // Between a step's two seconds and four seconds, not either.
        assert!(expected > 2.0 && expected < 4.0);
        assert_eq!(map.bpm_at(Ticks::ZERO), 120.0);
        assert!((map.bpm_at(Ticks::quarters(2)) - 90.0).abs() < 1e-9);
        assert_eq!(map.bpm_at(Ticks::quarters(4)), 60.0);
        assert_eq!(map.bpm_at(Ticks::quarters(9)), 60.0);
        // After the ramp the tempo is the point's own.
        let after = map.seconds_at(Ticks::quarters(5)) - map.seconds_at(Ticks::quarters(4));
        assert!((after - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_ramp_that_goes_up_runs_the_same_way() {
        let map = ramping(&[(0, 60.0, true), (8, 180.0, false)]);
        let at = |q: i64| map.seconds_at(Ticks::quarters(q));
        assert!(at(8) < 8.0 && at(8) > 8.0 / 3.0, "{}", at(8));
        let first = at(4);
        assert!(first > at(8) - first, "the slow half takes longer");
    }

    #[test]
    fn the_last_point_and_equal_tempos_do_not_ramp() {
        let last = ramping(&[(0, 100.0, true)]);
        assert_eq!(last.seconds_at(Ticks::quarters(10)), 6.0);
        let same = ramping(&[(0, 100.0, true), (4, 100.0, false)]);
        assert_eq!(same.seconds_at(Ticks::quarters(4)), 2.4);
        assert!(!same.ramps_between(Ticks::ZERO, Ticks::quarters(4)));
    }

    #[test]
    fn ramps_between_sees_only_the_ones_that_overlap() {
        let map = ramping(&[
            (0, 100.0, false),
            (4, 100.0, true),
            (8, 140.0, false),
            (12, 90.0, false),
        ]);
        assert!(!map.ramps_between(Ticks::ZERO, Ticks::quarters(4)));
        assert!(map.ramps_between(Ticks::ZERO, Ticks::quarters(5)));
        assert!(map.ramps_between(Ticks::quarters(6), Ticks::quarters(7)));
        assert!(!map.ramps_between(Ticks::quarters(8), Ticks::quarters(20)));
    }

    #[test]
    fn ramped_seconds_round_trip_to_the_same_tick() {
        let map = ramping(&[
            (0, 97.3, true),
            (33, 174.0, false),
            (50, 20.0, true),
            (70, 999.0, true),
            (71, 400.0, false),
        ]);
        let mut x = 0x2545_F491_4F6C_DD1D_u64;
        for _ in 0..10_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let t = Ticks((x % (PPQ as u64 * 90)) as i64);
            assert_eq!(map.ticks_at_seconds(map.seconds_at(t)), t);
        }
    }

    #[test]
    fn every_sample_is_addressable_along_a_ramp() {
        let map = ramping(&[(0, 999.0, true), (64, 20.0, false)]);
        let rate = 192_000;
        for sample in (0..2_000_000).step_by(991) {
            let t = map.ticks_at_sample(sample, rate);
            assert_eq!(map.sample_at(t, rate), sample, "sample {sample}");
        }
    }

    #[test]
    fn samples_never_go_backwards_along_ramps() {
        let map = ramping(&[(0, 140.0, true), (7, 33.0, true), (19, 800.0, false)]);
        let mut previous = i64::MIN;
        for t in (-PPQ..PPQ * 40).step_by(4_999) {
            let s = map.sample_at(Ticks(t), 44_100);
            assert!(s >= previous);
            previous = s;
        }
    }

    #[test]
    fn before_the_start_the_first_tempo_goes_on_even_when_it_ramps() {
        let map = ramping(&[(0, 120.0, true), (4, 60.0, false)]);
        assert_eq!(map.seconds_at(Ticks::quarters(-2)), -1.0);
        assert_eq!(map.ticks_at_seconds(-1.0), Ticks::quarters(-2));
    }

    #[test]
    fn stepped_keeps_the_time_at_every_step_and_at_the_next_point() {
        let map = ramping(&[(0, 120.0, false), (4, 80.0, true), (12, 200.0, false)]);
        let steps = map.stepped();
        let plain = TempoMap::new(steps.clone()).unwrap();
        assert!(steps.iter().all(|p| !p.ramp));
        // A ramp of 8 quarters is 32 sixteenths.
        assert_eq!(steps.len(), 1 + 32 + 1);
        for point in &steps {
            let want = map.seconds_at(point.at);
            assert!(
                (plain.seconds_at(point.at) - want).abs() < 1e-9,
                "{point:?}"
            );
        }
        let end = Ticks::quarters(20);
        assert!((plain.seconds_at(end) - map.seconds_at(end)).abs() < 1e-9);
    }

    #[test]
    fn a_very_long_ramp_is_written_as_few_steps() {
        let map = ramping(&[(0, 100.0, true), (100_000, 200.0, false)]);
        let steps = map.stepped();
        assert!(steps.len() <= 258, "{}", steps.len());
        let plain = TempoMap::new(steps).unwrap();
        let end = Ticks::quarters(100_000);
        assert!((plain.seconds_at(end) - map.seconds_at(end)).abs() < 1e-6);
    }

    #[test]
    fn a_step_is_written_without_a_ramp_flag_and_a_ramp_with_one() {
        let step = serde_json::to_string(&TempoPoint::step(Ticks(5), 100.0)).unwrap();
        assert!(!step.contains("ramp"), "{step}");
        let ramp = TempoPoint {
            ramp: true,
            ..TempoPoint::step(Ticks(5), 100.0)
        };
        let text = serde_json::to_string(&ramp).unwrap();
        assert!(text.contains("\"ramp\":true"), "{text}");
        assert_eq!(serde_json::from_str::<TempoPoint>(&text).unwrap(), ramp);
        assert!(!serde_json::from_str::<TempoPoint>(&step).unwrap().ramp);
    }
}
