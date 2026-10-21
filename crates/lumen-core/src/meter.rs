//! Time signatures and bar/beat positions.
//!
//! A [`MeterMap`] lists time-signature changes. Each change sits on a bar
//! line of the meter before it, as in every mainstream DAW, so bars are
//! always whole and numbering never jumps. Positions before zero extrapolate
//! the first meter into negative bars (count-in, pre-roll).

use serde::{Deserialize, Serialize};

use crate::time::{PPQ, Ticks};

/// A time signature, such as 4/4 or 7/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Meter {
    /// Beats per bar, 1–99.
    pub numerator: u32,
    /// Beat unit: 1, 2, 4, 8, 16, 32 or 64.
    pub denominator: u32,
}

impl Meter {
    pub const FOUR_FOUR: Meter = Meter {
        numerator: 4,
        denominator: 4,
    };

    pub fn is_valid(self) -> bool {
        (1..=99).contains(&self.numerator)
            && self.denominator.is_power_of_two()
            && self.denominator <= 64
    }

    /// Length of one beat (the denominator's note value).
    pub fn beat(self) -> Ticks {
        Ticks(4 * PPQ / i64::from(self.denominator))
    }

    pub fn bar(self) -> Ticks {
        Ticks(self.beat().0 * i64::from(self.numerator))
    }
}

impl std::fmt::Display for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// `meter` from `at` until the next point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterPoint {
    pub at: Ticks,
    pub meter: Meter,
}

/// A position as bars, beats and ticks, all zero-based. Display adds one to
/// bars and beats, so `Bbt { bar: 0, beat: 0, .. }` reads "1.1".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bbt {
    pub bar: i64,
    pub beat: u32,
    /// Ticks into the beat.
    pub ticks: Ticks,
}

impl std::fmt::Display for Bbt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.bar + 1, self.beat + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeterError {
    Empty,
    FirstPointNotAtZero,
    Invalid { index: usize },
    NotIncreasing { index: usize },
    NotOnBarLine { index: usize },
}

impl std::fmt::Display for MeterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeterError::Empty => write!(f, "the time-signature map is empty"),
            MeterError::FirstPointNotAtZero => {
                write!(f, "the first time signature must be at 0")
            }
            MeterError::Invalid { index } => write!(f, "time signature {index} is not valid"),
            MeterError::NotIncreasing { index } => {
                write!(f, "time signature {index} is not after the previous one")
            }
            MeterError::NotOnBarLine { index } => {
                write!(f, "time signature {index} is not on a bar line")
            }
        }
    }
}

impl std::error::Error for MeterError {}

/// Makes a list of time-signature changes into a valid map, moving as little
/// as it can: sorted by position (the last of two at one position wins), the
/// first at zero (a first change later than zero leaves 4/4 before it), each
/// later change moved to the nearest bar line of the meter before it, and a
/// change that lands on the bar of the one before it, or repeats its meter,
/// dropped. Invalid meters are dropped.
///
/// Content never moves with a change: only the bar lines do, so the same
/// list is what an edit, a file import and a migration all end up with.
pub fn tidy(points: &[MeterPoint]) -> Vec<MeterPoint> {
    let mut sorted: Vec<MeterPoint> = points
        .iter()
        .copied()
        .filter(|p| p.meter.is_valid())
        .collect();
    sorted.sort_by_key(|p| p.at);
    // The last of several at one position wins.
    let mut single: Vec<MeterPoint> = Vec::with_capacity(sorted.len());
    for point in sorted {
        match single.last_mut() {
            Some(last) if last.at == point.at => *last = point,
            _ => single.push(point),
        }
    }
    match single.first_mut() {
        Some(first) if first.at <= Ticks::ZERO => first.at = Ticks::ZERO,
        _ => single.insert(
            0,
            MeterPoint {
                at: Ticks::ZERO,
                meter: Meter::FOUR_FOUR,
            },
        ),
    }
    let mut out: Vec<MeterPoint> = Vec::with_capacity(single.len());
    for point in single {
        let Some(&previous) = out.last() else {
            out.push(point);
            continue;
        };
        let bar = previous.meter.bar().0;
        let bars = ((point.at - previous.at).0 + bar / 2).div_euclid(bar);
        if bars <= 0 || point.meter == previous.meter {
            continue;
        }
        out.push(MeterPoint {
            at: previous.at + Ticks(bars * bar),
            meter: point.meter,
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterMap {
    points: Vec<MeterPoint>,
    /// Bar number at each point, precomputed for O(log n) lookups.
    bars: Vec<i64>,
}

impl MeterMap {
    pub fn constant(meter: Meter) -> Result<Self, MeterError> {
        Self::new(vec![MeterPoint {
            at: Ticks::ZERO,
            meter,
        }])
    }

    pub fn new(points: Vec<MeterPoint>) -> Result<Self, MeterError> {
        let first = points.first().ok_or(MeterError::Empty)?;
        if first.at != Ticks::ZERO {
            return Err(MeterError::FirstPointNotAtZero);
        }
        let mut bars = Vec::with_capacity(points.len());
        let mut bar = 0;
        for (index, point) in points.iter().enumerate() {
            if !point.meter.is_valid() {
                return Err(MeterError::Invalid { index });
            }
            if index > 0 {
                let previous = points[index - 1];
                let span = point.at - previous.at;
                if span <= Ticks::ZERO {
                    return Err(MeterError::NotIncreasing { index });
                }
                let length = previous.meter.bar().0;
                if span.0 % length != 0 {
                    return Err(MeterError::NotOnBarLine { index });
                }
                bar += span.0 / length;
            }
            bars.push(bar);
        }
        Ok(Self { points, bars })
    }

    pub fn points(&self) -> &[MeterPoint] {
        &self.points
    }

    /// The meter in effect at `t`.
    pub fn meter_at(&self, t: Ticks) -> Meter {
        self.points[self.index_at(t)].meter
    }

    fn index_at(&self, t: Ticks) -> usize {
        self.points.partition_point(|p| p.at <= t).saturating_sub(1)
    }

    pub fn bbt_at(&self, t: Ticks) -> Bbt {
        let i = self.index_at(t);
        let point = self.points[i];
        let into = t.0 - point.at.0;
        let (bar_length, beat_length) = (point.meter.bar().0, point.meter.beat().0);
        let bars = into.div_euclid(bar_length);
        let in_bar = into.rem_euclid(bar_length);
        Bbt {
            bar: self.bars[i] + bars,
            beat: (in_bar / beat_length) as u32,
            ticks: Ticks(in_bar % beat_length),
        }
    }

    /// The tick at the start of `bar` (zero-based; negative before zero).
    pub fn bar_start(&self, bar: i64) -> Ticks {
        let i = self.bars.partition_point(|&b| b <= bar).saturating_sub(1);
        let point = self.points[i];
        point.at + Ticks((bar - self.bars[i]) * point.meter.bar().0)
    }

    pub fn ticks_at(&self, bbt: Bbt) -> Ticks {
        let start = self.bar_start(bbt.bar);
        start + Ticks(i64::from(bbt.beat) * self.meter_at(start).beat().0) + bbt.ticks
    }

    /// The bar line nearest `t`; half way goes to the later one.
    pub fn nearest_bar_start(&self, t: Ticks) -> Ticks {
        let bar = self.bbt_at(t).bar;
        let (before, after) = (self.bar_start(bar), self.bar_start(bar + 1));
        if t - before < after - t {
            before
        } else {
            after
        }
    }

    /// The first beat at or after `t`.
    pub fn next_beat(&self, t: Ticks) -> Ticks {
        let bbt = self.bbt_at(t);
        if bbt.ticks == Ticks::ZERO {
            return t;
        }
        let beat = self.meter_at(t).beat();
        let candidate = t + Ticks(beat.0 - bbt.ticks.0);
        // A meter change lands on a bar line, which is also a beat.
        let i = self.index_at(t);
        match self.points.get(i + 1) {
            Some(next) if next.at < candidate => next.at,
            _ => candidate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(points: &[(i64, u32, u32)]) -> MeterMap {
        MeterMap::new(
            points
                .iter()
                .map(|&(at, numerator, denominator)| MeterPoint {
                    at: Ticks(at),
                    meter: Meter {
                        numerator,
                        denominator,
                    },
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn four_four_bars_and_beats() {
        let m = MeterMap::constant(Meter::FOUR_FOUR).unwrap();
        assert_eq!(m.bbt_at(Ticks::ZERO).to_string(), "1.1");
        assert_eq!(m.bbt_at(Ticks::quarters(5)).to_string(), "2.2");
        assert_eq!(m.bbt_at(Ticks::quarters(5) + Ticks(7)).ticks, Ticks(7));
        assert_eq!(m.bar_start(3), Ticks::quarters(12));
        assert_eq!(m.bbt_at(Ticks::quarters(-1)).to_string(), "0.4");
        assert_eq!(m.bar_start(-1), Ticks::quarters(-4));
    }

    #[test]
    fn changes_continue_bar_numbering() {
        // Two bars of 4/4, then 7/8 (bars of 3.5 quarters), then 3/4.
        let seven_eight = Ticks::quarters(8);
        let three_four = seven_eight + Ticks(PPQ * 7 / 2 * 2);
        let m = map(&[(0, 4, 4), (seven_eight.0, 7, 8), (three_four.0, 3, 4)]);
        assert_eq!(m.bbt_at(seven_eight).to_string(), "3.1");
        assert_eq!(
            m.bbt_at(seven_eight + Ticks(PPQ / 2 * 6)).to_string(),
            "3.7"
        );
        assert_eq!(m.bbt_at(three_four).to_string(), "5.1");
        assert_eq!(m.bar_start(4), three_four);
        assert_eq!(m.bar_start(5), three_four + Ticks::quarters(3));
        assert_eq!(m.meter_at(three_four - Ticks(1)).to_string(), "7/8");
    }

    #[test]
    fn bbt_round_trips() {
        let m = map(&[
            (0, 4, 4),
            (Ticks::quarters(8).0, 5, 8),
            (Ticks::quarters(13).0, 3, 4),
        ]);
        for t in (-PPQ * 8..PPQ * 40).step_by(9_973) {
            assert_eq!(m.ticks_at(m.bbt_at(Ticks(t))), Ticks(t), "tick {t}");
        }
    }

    #[test]
    fn next_beat_snaps_forward_and_respects_changes() {
        let m = map(&[(0, 4, 4), (Ticks::quarters(4).0, 6, 8)]);
        assert_eq!(m.next_beat(Ticks::quarters(2)), Ticks::quarters(2));
        assert_eq!(
            m.next_beat(Ticks::quarters(2) + Ticks(1)),
            Ticks::quarters(3)
        );
        // In 6/8 beats are eighths.
        let in_six_eight = Ticks::quarters(4) + Ticks(1);
        assert_eq!(
            m.next_beat(in_six_eight),
            Ticks::quarters(4) + Ticks(PPQ / 2)
        );
    }

    #[test]
    fn the_nearest_bar_line_is_found_across_changes() {
        let m = map(&[(0, 4, 4), (Ticks::quarters(8).0, 6, 8)]);
        assert_eq!(m.nearest_bar_start(Ticks::quarters(1)), Ticks::ZERO);
        // Half way (2 quarters into a 4-quarter bar) goes to the later line.
        assert_eq!(m.nearest_bar_start(Ticks::quarters(2)), Ticks::quarters(4));
        assert_eq!(m.nearest_bar_start(Ticks::quarters(7)), Ticks::quarters(8));
        // 6/8 bars are 3 quarters long.
        assert_eq!(
            m.nearest_bar_start(Ticks::quarters(10)),
            Ticks::quarters(11)
        );
        assert_eq!(m.nearest_bar_start(Ticks::quarters(9)), Ticks::quarters(8));
    }

    #[test]
    fn tidy_keeps_a_valid_map_as_it_is() {
        let m = [(0, 4, 4), (Ticks::quarters(8).0, 7, 8)].map(|(at, n, d)| MeterPoint {
            at: Ticks(at),
            meter: Meter {
                numerator: n,
                denominator: d,
            },
        });
        assert_eq!(tidy(&m), m.to_vec());
    }

    #[test]
    fn tidy_moves_a_change_to_the_nearest_bar_line() {
        let p = |at, n, d| MeterPoint {
            at: Ticks(at),
            meter: Meter {
                numerator: n,
                denominator: d,
            },
        };
        // 4/4 bars are 4 quarters: 5.2 quarters is closer to 4 than 8.
        let out = tidy(&[p(0, 4, 4), p(Ticks::quarters(5).0 + 200, 3, 4)]);
        assert_eq!(out[1].at, Ticks::quarters(4));
        // 6 quarters is half way: the later bar line.
        let out = tidy(&[p(0, 4, 4), p(Ticks::quarters(6).0, 3, 4)]);
        assert_eq!(out[1].at, Ticks::quarters(8));
        // The result is always a valid map.
        MeterMap::new(out).unwrap();
    }

    #[test]
    fn tidy_drops_what_collapses_or_repeats_and_fills_the_start() {
        let p = |at, n, d| MeterPoint {
            at: Ticks(at),
            meter: Meter {
                numerator: n,
                denominator: d,
            },
        };
        // Less than half a bar from the start, so on the first bar line.
        assert_eq!(tidy(&[p(0, 4, 4), p(PPQ, 3, 4)]).len(), 1);
        // Repeating the meter before it adds nothing.
        assert_eq!(tidy(&[p(0, 4, 4), p(Ticks::quarters(8).0, 4, 4)]).len(), 1);
        // The last of two at one place wins.
        let out = tidy(&[p(0, 4, 4), p(0, 3, 4)]);
        assert_eq!(out, vec![p(0, 3, 4)]);
        // A first change after zero leaves 4/4 before it; none gives 4/4.
        let out = tidy(&[p(Ticks::quarters(8).0, 7, 8)]);
        assert_eq!(out[0], p(0, 4, 4));
        assert_eq!(out[1].at, Ticks::quarters(8));
        assert_eq!(tidy(&[]), vec![p(0, 4, 4)]);
        // Invalid meters are dropped.
        assert_eq!(tidy(&[p(0, 4, 3)]), vec![p(0, 4, 4)]);
    }

    #[test]
    fn invalid_maps_are_rejected() {
        let p = |at, numerator, denominator| MeterPoint {
            at: Ticks(at),
            meter: Meter {
                numerator,
                denominator,
            },
        };
        assert_eq!(MeterMap::new(vec![]), Err(MeterError::Empty));
        assert_eq!(
            MeterMap::new(vec![p(1, 4, 4)]),
            Err(MeterError::FirstPointNotAtZero)
        );
        assert_eq!(
            MeterMap::new(vec![p(0, 4, 3)]),
            Err(MeterError::Invalid { index: 0 })
        );
        assert_eq!(
            MeterMap::new(vec![p(0, 4, 4), p(PPQ * 3, 3, 4)]),
            Err(MeterError::NotOnBarLine { index: 1 })
        );
        assert_eq!(
            MeterMap::new(vec![p(0, 4, 4), p(0, 3, 4)]),
            Err(MeterError::NotIncreasing { index: 1 })
        );
    }
}
