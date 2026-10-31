//! Tap tempo (M9 R7): the tempo from the times of taps. Pure: the times are
//! arguments, so the tests and the E2E scenario give exact ones; the app
//! passes frame time (`app::tap_tempo`).
//!
//! The rules (ADR-0112):
//! - a run starts at a tap and ends after [`GAP`] without one (or, in the
//!   app, at any other edit);
//! - the first two taps set nothing; from the third the tempo is 60 ÷ the
//!   mean of the last [`INTERVALS`] intervals, rounded to a whole BPM and
//!   kept inside 20 to 999;
//! - an interval more than [`STRAY`] away from the mean is ignored, and two
//!   such in a row start the run again from the last tap, so a change of mind
//!   is followed but one stray tap is not.

use lumen_core::time::{MAX_BPM, MIN_BPM};

/// Seconds without a tap that end a run: the slowest tempo that can be
/// tapped is 60 ÷ 3 = 20 BPM.
pub const GAP: f64 = 3.0;
/// The intervals the mean is taken over, at most.
pub const INTERVALS: usize = 4;
/// How far from the mean an interval may be, as a share of it.
pub const STRAY: f64 = 0.4;

/// A run of taps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Taps {
    /// When the last tap was, in seconds; `None` before the first.
    last: Option<f64>,
    /// Taps counted since the run (re)started: what the control shows.
    count: usize,
    /// The last intervals taken, oldest first.
    intervals: Vec<f64>,
    /// Stray intervals in a row.
    strays: usize,
}

/// What a tap did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tap {
    /// A new run started with this tap.
    Started,
    /// The run goes on, with no tempo yet (or a stray interval ignored).
    Counted,
    /// The run's tempo, in whole BPM.
    Tempo(f64),
}

impl Taps {
    /// A tap at `now` seconds.
    pub fn tap(&mut self, now: f64) -> Tap {
        let Some(last) = self.last.filter(|&last| self.running(now) && now >= last) else {
            *self = Taps {
                last: Some(now),
                count: 1,
                ..Taps::default()
            };
            return Tap::Started;
        };
        self.last = Some(now);
        self.count += 1;
        let interval = now - last;
        if let Some(mean) = self.mean()
            && (interval - mean).abs() > STRAY * mean
        {
            self.strays += 1;
            if self.strays >= 2 {
                // A change of mind: the run starts again from this tap.
                self.count = 1;
                self.intervals.clear();
                self.strays = 0;
            }
            return Tap::Counted;
        }
        self.strays = 0;
        self.intervals.push(interval);
        if self.intervals.len() > INTERVALS {
            self.intervals.remove(0);
        }
        match self.mean() {
            Some(mean) if self.intervals.len() >= 2 && mean > 0.0 => {
                Tap::Tempo((60.0 / mean).round().clamp(MIN_BPM, MAX_BPM))
            }
            _ => Tap::Counted,
        }
    }

    /// The run is still open at `now`: a tap less than [`GAP`] ago.
    pub fn running(&self, now: f64) -> bool {
        self.last.is_some_and(|last| now - last <= GAP)
    }

    /// Taps since the run (re)started, for the control; 0 with no run.
    pub fn count(&self) -> usize {
        if self.last.is_some() { self.count } else { 0 }
    }

    /// Ends the run: the next tap starts a new one.
    pub fn end(&mut self) {
        *self = Taps::default();
    }

    fn mean(&self) -> Option<f64> {
        (!self.intervals.is_empty())
            .then(|| self.intervals.iter().sum::<f64>() / self.intervals.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taps at `times`, answering what the last one did.
    fn taps(taps: &mut Taps, times: &[f64]) -> Tap {
        let mut last = Tap::Started;
        for &t in times {
            last = taps.tap(t);
        }
        last
    }

    #[test]
    fn the_tempo_follows_from_the_third_tap() {
        let mut t = Taps::default();
        assert_eq!(t.count(), 0);
        assert_eq!(t.tap(10.0), Tap::Started);
        assert_eq!(t.count(), 1);
        assert_eq!(t.tap(10.5), Tap::Counted, "one interval sets nothing");
        assert_eq!(t.count(), 2);
        assert_eq!(t.tap(11.0), Tap::Tempo(120.0));
        assert_eq!(t.count(), 3);
    }

    #[test]
    fn steady_taps_are_averaged_over_the_last_four_and_rounded() {
        let mut t = Taps::default();
        // 0.49 and 0.51 s alternate: 120 BPM on average, as a hand taps.
        let times = [0.0, 0.49, 1.0, 1.49, 2.0, 2.49, 3.0];
        assert_eq!(taps(&mut t, &times), Tap::Tempo(120.0));
        // Only the last four count: a run that slows follows the slowing.
        let mut t = Taps::default();
        let times = [0.0, 0.4, 0.8, 1.2, 1.7, 2.2, 2.7, 3.2];
        assert_eq!(taps(&mut t, &times), Tap::Tempo(120.0));
        // 0.47 s is 127.66 BPM: a whole BPM.
        let mut t = Taps::default();
        assert_eq!(taps(&mut t, &[0.0, 0.47, 0.94]), Tap::Tempo(128.0));
    }

    #[test]
    fn one_stray_tap_is_ignored() {
        let mut t = Taps::default();
        assert_eq!(taps(&mut t, &[0.0, 0.5, 1.0, 1.5]), Tap::Tempo(120.0));
        // A late tap: 0.8 s is 60% off the mean, ignored.
        assert_eq!(t.tap(2.3), Tap::Counted);
        // Back in time: the run goes on at 120.
        assert_eq!(t.tap(2.8), Tap::Tempo(120.0));
        assert_eq!(t.count(), 6);
    }

    #[test]
    fn a_change_of_mind_is_followed_after_two_stray_intervals() {
        let mut t = Taps::default();
        assert_eq!(taps(&mut t, &[0.0, 0.5, 1.0, 1.5]), Tap::Tempo(120.0));
        // Now 0.25 s apart: two strays start the run again from the last.
        assert_eq!(t.tap(1.75), Tap::Counted);
        assert_eq!(t.tap(2.0), Tap::Counted);
        assert_eq!(t.count(), 1, "the run starts again");
        assert_eq!(t.tap(2.25), Tap::Counted);
        assert_eq!(t.tap(2.5), Tap::Tempo(240.0));
    }

    #[test]
    fn a_gap_of_three_seconds_ends_the_run() {
        let mut t = Taps::default();
        taps(&mut t, &[0.0, 0.5, 1.0]);
        assert!(t.running(4.0));
        assert!(!t.running(4.01));
        assert_eq!(t.tap(4.5), Tap::Started);
        assert_eq!(t.count(), 1);
        // The slowest tempo a run can reach is 20 BPM: 3 s apart.
        let mut t = Taps::default();
        assert_eq!(taps(&mut t, &[0.0, 3.0, 6.0]), Tap::Tempo(20.0));
        // A clock that went back starts a new run rather than a negative
        // interval.
        let mut t = Taps::default();
        taps(&mut t, &[5.0, 5.5]);
        assert_eq!(t.tap(1.0), Tap::Started);
    }

    #[test]
    fn the_tempo_stays_within_20_and_999() {
        let mut t = Taps::default();
        // 0.05 s apart is 1,200 BPM.
        assert_eq!(taps(&mut t, &[0.0, 0.05, 0.1]), Tap::Tempo(999.0));
        // Two taps at the same moment (a double click) are not a tempo.
        let mut t = Taps::default();
        assert_eq!(taps(&mut t, &[1.0, 1.0, 1.0]), Tap::Counted);
    }

    #[test]
    fn ending_a_run_forgets_it() {
        let mut t = Taps::default();
        taps(&mut t, &[0.0, 0.5, 1.0]);
        t.end();
        assert_eq!(t, Taps::default());
        assert!(!t.running(1.0));
        assert_eq!(t.tap(1.2), Tap::Started);
    }
}
