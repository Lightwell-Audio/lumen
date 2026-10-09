//! Pan laws (M4), shared by the engine that applies them and the
//! interface that shows them. A pan law sets how loud a centred signal is
//! compared with one panned hard to a side: the hard side always plays at
//! unity, and the law decides how far the centre drops.
//!
//! Lumen pans like a balance control on stereo material: each side of the
//! signal gets its own gain from the law, so panning left turns the right
//! side down rather than folding it into the left.

use serde::{Deserialize, Serialize};

/// How a pan position becomes left and right gains. Saved per project
/// (format v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanLaw {
    /// −3 dB at the centre, constant power: the level stays even across
    /// the sweep. The default (owner, M4 plan).
    #[default]
    ConstantPower,
    /// −4.5 dB at the centre: halfway between constant power and linear.
    Compromise,
    /// −6 dB at the centre, linear: the two sides always add up to unity.
    Linear,
    /// 0 dB at the centre: both sides at unity, and panning turns only the
    /// far side down.
    Balance,
}

impl PanLaw {
    pub const ALL: [PanLaw; 4] = [
        Self::ConstantPower,
        Self::Compromise,
        Self::Linear,
        Self::Balance,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ConstantPower => "−3 dB, constant power",
            Self::Compromise => "−4.5 dB",
            Self::Linear => "−6 dB, linear",
            Self::Balance => "0 dB, balance",
        }
    }

    /// The left and right gains for `pan`, from −1 (hard left) through 0
    /// (centre) to +1 (hard right).
    pub fn gains(self, pan: f32) -> [f32; 2] {
        let pan = if pan.is_nan() {
            0.0
        } else {
            pan.clamp(-1.0, 1.0)
        };
        let power = || {
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            [angle.cos(), angle.sin()]
        };
        let linear = || [(1.0 - pan) / 2.0, (1.0 + pan) / 2.0];
        match self {
            Self::ConstantPower => power(),
            Self::Linear => linear(),
            Self::Compromise => {
                let (p, l) = (power(), linear());
                [(p[0] * l[0]).sqrt(), (p[1] * l[1]).sqrt()]
            }
            Self::Balance => [(1.0 - pan).min(1.0), (1.0 + pan).min(1.0)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(gain: f32) -> f32 {
        20.0 * gain.log10()
    }

    #[test]
    fn each_law_drops_the_centre_by_its_amount() {
        let centre = |law: PanLaw| db(law.gains(0.0)[0]);
        assert!((centre(PanLaw::ConstantPower) + 3.01).abs() < 0.01);
        assert!((centre(PanLaw::Compromise) + 4.52).abs() < 0.01);
        assert!((centre(PanLaw::Linear) + 6.02).abs() < 0.01);
        assert_eq!(centre(PanLaw::Balance), 0.0);
    }

    #[test]
    fn the_hard_side_is_unity_and_the_far_side_silent() {
        for law in PanLaw::ALL {
            let [l, r] = law.gains(-1.0);
            assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6, "{law:?}: {l} {r}");
            let [l, r] = law.gains(1.0);
            assert!(l.abs() < 1e-6 && (r - 1.0).abs() < 1e-6, "{law:?}: {l} {r}");
        }
    }

    #[test]
    fn constant_power_keeps_the_power_across_the_sweep() {
        for i in -10..=10 {
            let [l, r] = PanLaw::ConstantPower.gains(i as f32 / 10.0);
            assert!((l * l + r * r - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn out_of_range_and_nan_pans_are_safe() {
        assert_eq!(PanLaw::Balance.gains(5.0), PanLaw::Balance.gains(1.0));
        assert_eq!(PanLaw::Linear.gains(f32::NAN), [0.5, 0.5]);
    }
}
