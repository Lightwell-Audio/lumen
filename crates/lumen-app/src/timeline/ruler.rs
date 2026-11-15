use lumen_core::meter::Meter;
use lumen_core::time::{PPQ, Ticks};

pub const MIN_STEP_PX: f32 = 12.0;

pub fn ticks_at(quarters: f64) -> Ticks {
    Ticks((quarters * PPQ as f64).round() as i64).max(Ticks::ZERO)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const FOUR_FOUR: Meter = Meter::FOUR_FOUR;

    #[test]
    fn beats_map_to_the_nearest_tick_and_clamp_at_zero() {
        assert_eq!(ticks_at(0.0), Ticks::ZERO);
        assert_eq!(ticks_at(2.0), Ticks::quarters(2));
        assert_eq!(ticks_at(1.0 + 0.4 / PPQ as f64), Ticks::quarters(1));
        assert_eq!(ticks_at(-3.0), Ticks::ZERO);
    }

    #[test]
    fn the_step_follows_the_zoom() {
        assert_eq!(step(FOUR_FOUR, 1.0), FOUR_FOUR.bar());
        assert_eq!(step(FOUR_FOUR, 24.0), Ticks(PPQ / 2));
        assert_eq!(step(FOUR_FOUR, 1_000.0), Ticks(PPQ / 16));
        let six_eight = Meter {
            numerator: 6,
            denominator: 8,
        };
        assert_eq!(step(six_eight, 24.0), Ticks(PPQ / 2));
        assert_eq!(step(six_eight, 48.0), Ticks(PPQ / 4));
    }
}
