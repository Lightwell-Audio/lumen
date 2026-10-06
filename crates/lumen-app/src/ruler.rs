//! Ruler geometry and snapping (M2 slice 5, the first part of the owner's
//! Ableton-style scrubbing request). Clicking the ruler moves the playhead
//! to the nearest grid line; the grid adapts to zoom. Holding Alt bypasses
//! snapping.

use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{PPQ, Ticks};

/// The finest snap step must be at least this wide on screen.
pub const MIN_STEP_PX: f32 = 12.0;

/// The musical position under `x`, for a view starting at `left_quarter`
/// (in quarter notes) at `px_per_quarter`. Never before the start.
pub fn ticks_at_x(x: f32, origin_x: f32, left_quarter: f32, px_per_quarter: f32) -> Ticks {
    let quarters = f64::from(left_quarter) + f64::from((x - origin_x) / px_per_quarter);
    Ticks((quarters * PPQ as f64).round() as i64).max(Ticks::ZERO)
}

/// The snap step for `meter` at this zoom: the finest of a bar, a beat and
/// halvings of the beat down to 1/16 of it, that is still at least
/// [`MIN_STEP_PX`] wide. When even a bar is narrower, a bar.
pub fn step(meter: Meter, px_per_quarter: f32) -> Ticks {
    let px = |t: Ticks| t.as_quarters() as f32 * px_per_quarter;
    let beat = meter.beat();
    let mut best = meter.bar();
    if px(beat) >= MIN_STEP_PX {
        best = beat;
        let mut finer = beat;
        for _ in 0..4 {
            finer = Ticks(finer.0 / 2);
            if px(finer) < MIN_STEP_PX {
                break;
            }
            best = finer;
        }
    }
    best
}

/// `t` moved to the nearest grid line. Lines are counted from the start of
/// the bar containing `t`, so they stay right across time-signature changes,
/// and never pass the next bar line.
pub fn snap(meter: &MeterMap, t: Ticks, px_per_quarter: f32) -> Ticks {
    let bar = meter.bbt_at(t).bar;
    let start = meter.bar_start(bar);
    let next = meter.bar_start(bar + 1);
    let step = step(meter.meter_at(start), px_per_quarter).0;
    let lines = ((t - start).0 as f64 / step as f64).round() as i64;
    (start + Ticks(lines * step)).min(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::meter::MeterPoint;

    const FOUR_FOUR: Meter = Meter::FOUR_FOUR;

    #[test]
    fn x_maps_to_ticks_and_clamps_at_zero() {
        assert_eq!(ticks_at_x(100.0, 100.0, 0.0, 24.0), Ticks::ZERO);
        assert_eq!(ticks_at_x(148.0, 100.0, 0.0, 24.0), Ticks::quarters(2));
        assert_eq!(ticks_at_x(100.0, 100.0, 8.0, 24.0), Ticks::quarters(8));
        assert_eq!(ticks_at_x(10.0, 100.0, 0.0, 24.0), Ticks::ZERO);
    }

    #[test]
    fn the_step_follows_the_zoom() {
        // Zoomed far out: a bar is the finest step that fits.
        assert_eq!(step(FOUR_FOUR, 1.0), FOUR_FOUR.bar());
        // 24 px per quarter (the default): beats fit, eighths (12 px) too.
        assert_eq!(step(FOUR_FOUR, 24.0), Ticks(PPQ / 2));
        // Zoomed in: down to 1/16 of the beat (a 64th note) and no finer.
        assert_eq!(step(FOUR_FOUR, 1_000.0), Ticks(PPQ / 16));
        // In 6/8 the beat is an eighth note.
        let six_eight = Meter {
            numerator: 6,
            denominator: 8,
        };
        assert_eq!(step(six_eight, 24.0), Ticks(PPQ / 2));
        assert_eq!(step(six_eight, 48.0), Ticks(PPQ / 4));
    }

    #[test]
    fn clicks_snap_to_the_nearest_line() {
        let map = MeterMap::constant(FOUR_FOUR).unwrap();
        let eighth = PPQ / 2;
        // At 24 px per quarter the step is an eighth.
        assert_eq!(snap(&map, Ticks(eighth / 2 - 1), 24.0), Ticks::ZERO);
        assert_eq!(snap(&map, Ticks(eighth / 2 + 1), 24.0), Ticks(eighth));
        assert_eq!(
            snap(&map, Ticks::quarters(5) + Ticks(10), 24.0),
            Ticks::quarters(5)
        );
        // Zoomed out the step is a bar: just past the middle goes forward.
        assert_eq!(
            snap(&map, Ticks::quarters(2) + Ticks(1), 1.0),
            Ticks::quarters(4)
        );
        assert_eq!(snap(&map, Ticks::quarters(2) - Ticks(1), 1.0), Ticks::ZERO);
    }

    #[test]
    fn snapping_respects_time_signature_changes() {
        // Two bars of 4/4, then 7/8: bar 3 starts at quarter 8 and is 3.5
        // quarters long, so bar 4 starts at quarter 11.5.
        let map = MeterMap::new(vec![
            MeterPoint {
                at: Ticks::ZERO,
                meter: FOUR_FOUR,
            },
            MeterPoint {
                at: Ticks::quarters(8),
                meter: Meter {
                    numerator: 7,
                    denominator: 8,
                },
            },
        ])
        .unwrap();
        let bar4 = Ticks::quarters(11) + Ticks(PPQ / 2);
        // Zoomed out, steps are bars: near the end of bar 3 snaps to bar 4.
        assert_eq!(snap(&map, bar4 - Ticks(PPQ / 4), 1.0), bar4);
        // At beat zoom (eighths in 7/8), lines count from the bar start.
        assert_eq!(
            snap(&map, Ticks::quarters(9) + Ticks(PPQ / 2 - 3), 24.0),
            Ticks::quarters(9) + Ticks(PPQ / 2)
        );
    }
}
