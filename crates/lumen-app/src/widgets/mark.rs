use eframe::egui::{Color32, Painter, Rect, pos2, vec2};
use lumen_brand::{
    COMPACT, COMPACT_HALF_WIDTH, COMPACT_REFLECTIONS, GRID, HALF_WIDTH, HORIZON, REFLECTIONS, SUN,
};

use crate::theme;

pub const SUNK: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub rise: f32,
    pub glow: f32,
    pub recording: bool,
}

impl Look {
    pub const REST: Self = Self {
        rise: 0.0,
        glow: 0.0,
        recording: false,
    };

    pub const WAITING: Self = Self {
        rise: 0.45,
        glow: 0.25,
        recording: false,
    };

    pub const RISEN: Self = Self {
        rise: 1.0,
        glow: 1.0,
        recording: false,
    };

    pub fn level(db: f32, recording: bool) -> Self {
        let t = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
        Self {
            rise: t,
            glow: t,
            recording,
        }
    }
}

pub fn sun_y(rise: f32) -> f32 {
    HORIZON + (1.0 - rise.clamp(0.0, 1.0)) * SUNK
}

pub fn paint(painter: &Painter, rect: Rect, look: Look) {
    let palette = theme::p();
    let size = rect.width().min(rect.height());
    let unit = size / GRID;
    let origin = rect.center() - vec2(size, size) / 2.0;
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let compact = size <= COMPACT;
    let opacity = 0.55 + 0.45 * look.glow;
    let (sun, core) = if look.recording {
        (palette.record, palette.core)
    } else {
        (palette.sun, palette.sun_core)
    };
    let water = palette.accent;
    let fade = |c: Color32, a: f32| c.gamma_multiply(a * opacity);

    let sky = Rect::from_min_max(origin, at(GRID, HORIZON)).intersect(painter.clip_rect());
    let sky_painter = painter.with_clip_rect(sky);
    let center = at(GRID / 2.0, sun_y(look.rise));
    let glow = (0.15 + 0.85 * look.glow) * crate::light::strength();
    for (reach, a) in [(7.0, 0.12), (3.5, 0.24)] {
        sky_painter.circle_filled(center, (SUN + reach) * unit, fade(sun, a * glow));
    }
    const STEPS: usize = 6;
    for i in 0..STEPS {
        let t = i as f32 / (STEPS - 1) as f32;
        let radius = SUN * (1.0 - 0.6 * t) * unit;
        sky_painter.circle_filled(center, radius, fade(sun.lerp_to_gamma(core, t), 1.0));
    }

    let width =
        2.0 * if compact {
            COMPACT_HALF_WIDTH
        } else {
            HALF_WIDTH
        } * unit;
    let half = |x0: f32, x1: f32, left: Color32, right: Color32| {
        let band = Rect::from_min_max(
            pos2(at(x0, 0.0).x, at(0.0, HORIZON).y - width / 2.0),
            pos2(at(x1, 0.0).x, at(0.0, HORIZON).y + width / 2.0),
        );
        crate::widgets::gradient(painter, band, left, right);
    };
    let middle = fade(core, 1.0);
    half(5.0, GRID / 2.0, fade(water, 0.14), middle);
    half(GRID / 2.0, GRID - 5.0, middle, fade(water, 0.14));

    let lines: &[(f32, f32, f32, f32)] = if compact {
        &COMPACT_REFLECTIONS
    } else {
        &REFLECTIONS
    };
    for &(x0, x1, y, a) in lines {
        let pill = Rect::from_min_max(
            at(x0, 0.0) + vec2(-width / 2.0, at(0.0, y).y - origin.y - width / 2.0),
            at(x1, 0.0) + vec2(width / 2.0, at(0.0, y).y - origin.y + width / 2.0),
        );
        painter.rect_filled(pill, width / 2.0, fade(water, a));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_rises_with_the_level() {
        assert_eq!(Look::level(f32::NEG_INFINITY, false), Look::REST);
        assert_eq!(Look::level(-60.0, false).rise, 0.0);
        assert_eq!(Look::level(0.0, false).rise, 1.0);
        assert_eq!(Look::level(6.0, false).rise, 1.0);
        let half = Look::level(-24.0, false);
        assert!((half.rise - 0.5).abs() < 1e-6 && half.glow == half.rise);
        assert!(Look::level(-24.0, true).recording);
    }

    #[test]
    fn at_rest_the_sun_peeks_over_the_horizon() {
        assert_eq!(sun_y(1.0), HORIZON);
        let rest = sun_y(0.0);
        assert!(rest > HORIZON && rest - SUN < HORIZON, "{rest}");
    }
}
