//! Fade curves (M3 slice 6), shared by the engine that plays them and the
//! arrangement that draws them, so a fade sounds as it is drawn.

use serde::{Deserialize, Serialize};

use crate::time::Ticks;

/// How a fade's level moves from silence to full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeShape {
    /// Straight.
    #[default]
    Linear,
    /// Slow at first, then quick (x²): gentle starts.
    Slow,
    /// Quick at first, then slow: punchy starts, smooth tails.
    Fast,
    /// Slow at both ends (smoothstep): the smoothest crossfades.
    SCurve,
}

impl FadeShape {
    pub const ALL: [FadeShape; 4] = [Self::Linear, Self::Slow, Self::Fast, Self::SCurve];

    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Slow => "Slow",
            Self::Fast => "Fast",
            Self::SCurve => "S-curve",
        }
    }

    /// The level `x` of the way through a fade-in (0 to 1), from 0 to 1.
    /// A fade-out plays it backwards: `gain(1 - x)`.
    #[inline]
    pub fn gain(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::Slow => x * x,
            Self::Fast => 1.0 - (1.0 - x) * (1.0 - x),
            Self::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// A clip's fade in or out: how long, and its shape. A zero length is no
/// fade (the 2 ms edge fade still applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Fade {
    pub length: Ticks,
    pub shape: FadeShape,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_runs_from_silence_to_full_and_never_falls() {
        for shape in FadeShape::ALL {
            assert_eq!(shape.gain(0.0), 0.0, "{shape:?}");
            assert_eq!(shape.gain(1.0), 1.0, "{shape:?}");
            let mut last = 0.0;
            for k in 0..=100 {
                let g = shape.gain(k as f32 / 100.0);
                assert!(g >= last, "{shape:?} falls at {k}");
                last = g;
            }
        }
        assert!(FadeShape::Slow.gain(0.5) < 0.5);
        assert!(FadeShape::Fast.gain(0.5) > 0.5);
        assert_eq!(FadeShape::SCurve.gain(0.5), 0.5);
    }
}
