use serde::{Deserialize, Serialize};

use crate::curve::Curve;
use crate::time::Ticks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeShape {
    Linear,
    Slow,
    Fast,
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

    pub fn curve(self) -> Curve {
        match self {
            Self::Linear => Curve::LINEAR,
            Self::Slow => Curve::SLOW,
            Self::Fast => Curve::FAST,
            Self::SCurve => Curve::S_CURVE,
        }
    }

    pub fn of(curve: &Curve) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.curve().same(curve))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Fade {
    pub length: Ticks,
    #[serde(default, skip_serializing_if = "ticks_are_zero")]
    pub offset: Ticks,
    #[serde(default, skip_serializing_if = "ticks_are_zero")]
    pub span: Ticks,
    #[serde(default, skip_serializing_if = "Curve::is_linear")]
    pub curve: Curve,
}

impl Fade {
    pub fn is_none(&self) -> bool {
        self.length == Ticks::ZERO && self.curve.is_linear()
    }

    pub fn level(&self, x: f32) -> f32 {
        let span = if self.span == Ticks::ZERO {
            self.length
        } else {
            self.span
        };
        if span == Ticks::ZERO {
            return self.curve.at(x);
        }
        let position = self.offset.0 as f32 + x * self.length.0 as f32;
        self.curve.at((position / span.0 as f32).clamp(0.0, 1.0))
    }
}

fn ticks_are_zero(ticks: &Ticks) -> bool {
    *ticks == Ticks::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_runs_from_silence_to_full_and_is_found_again() {
        for shape in FadeShape::ALL {
            let fade = Fade {
                length: Ticks::quarters(1),
                curve: shape.curve(),
                ..Fade::default()
            };
            assert_eq!(fade.level(0.0), 0.0, "{shape:?}");
            assert!((fade.level(1.0) - 1.0).abs() < 1e-6, "{shape:?}");
            assert_eq!(FadeShape::of(&fade.curve), Some(shape));
        }
        assert!(FadeShape::Slow.curve().at(0.5) < 0.5);
        assert!(FadeShape::Fast.curve().at(0.5) > 0.5);
        assert!((FadeShape::SCurve.curve().at(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_straight_fade_saves_without_its_curve() {
        let plain = Fade {
            length: Ticks(100),
            curve: Curve::LINEAR,
            ..Fade::default()
        };
        assert_eq!(serde_json::to_string(&plain).unwrap(), r#"{"length":100}"#);
        let bent = Fade {
            length: Ticks(100),
            curve: Curve::SLOW,
            ..Fade::default()
        };
        let text = serde_json::to_string(&bent).unwrap();
        assert!(text.contains("\"curve\":["), "{text}");
        assert_eq!(serde_json::from_str::<Fade>(&text).unwrap(), bent);
        assert!(Fade::default().is_none());
    }

    #[test]
    fn a_sliced_fade_keeps_its_original_curve_window() {
        let original = Fade {
            length: Ticks::quarters(4),
            curve: Curve::SLOW,
            ..Fade::default()
        };
        let slice = Fade {
            length: Ticks::quarters(2),
            offset: Ticks::quarters(1),
            span: Ticks::quarters(4),
            ..original
        };
        assert!((slice.level(0.0) - original.level(0.25)).abs() < 1e-6);
        assert!((slice.level(0.5) - original.level(0.5)).abs() < 1e-6);
        assert!((slice.level(1.0) - original.level(0.75)).abs() < 1e-6);
    }
}
