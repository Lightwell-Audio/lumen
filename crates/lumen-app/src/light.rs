//! The light layer (ADR-0043, the owner's approval of the identity canvas):
//! every glow in Lumen is one of five levels, never a one-off.
//!
//! - **Unlit**: chrome, labels, controls at rest. No light at all.
//! - **Ember** ([`EMBER_WASH`]): under the hand (hover washes).
//! - **[`LIT`]**: chosen (selection, handles, edges being dragged).
//! - **[`BEAM`]**: sounding now (the playhead while playing, Play, meter
//!   heads, the mark).
//! - **[`FLARE`]**: needs you (record arm, clip lights), in record red.
//!
//! The rules (`docs/DESIGN.md` §6a): only what is alive gives light; a light
//! has a source (a core, its colour in the halo, falling off outward); one
//! colour of light (the accent) plus record red; light follows sound; dark
//! is calm. On the light theme halos are halved and the core is the accent
//! ([`strength`], `Palette::core`).

use eframe::egui::{self, Color32, Painter, Pos2, Rangef, Rect, Stroke, pos2};

use crate::theme;

/// One level of light: a crisp line and the soft layers around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// The crisp line's width, in points.
    pub line: f32,
    /// Soft layers, each `(reach, alpha)`: it reaches `reach` points past
    /// the line on each side at `alpha` of the light's colour.
    pub halo: &'static [(f32, f32)],
    /// A white (or, on the light theme, accent) core inside the line.
    pub core: f32,
    /// The light it casts on the surface below: `(width, alpha)` at its
    /// centre, fading to nothing at the edges.
    pub cast: Option<(f32, f32)>,
}

/// Under the hand (Ember): no line, only a wash of the text colour over
/// what the pointer is on.
pub const EMBER_WASH: f32 = 0.08;

/// Chosen: the owner's S3 glow, a 1.5 pt line with two soft layers.
pub const LIT: Light = Light {
    line: 1.5,
    halo: &[(3.0, 0.12), (1.5, 0.28)],
    core: 0.0,
    cast: None,
};

/// Sounding now: a white core in a 2 pt accent line, a halo, and light cast
/// on the surface below.
pub const BEAM: Light = Light {
    line: 2.0,
    halo: &[(3.0, 0.22)],
    core: 1.0,
    cast: Some((56.0, 0.09)),
};

/// Needs you: record red with a wide halo (the arm ring breathes it).
pub const FLARE: Light = Light {
    line: 1.5,
    halo: &[(6.0, 0.18), (2.5, 0.45)],
    core: 0.0,
    cast: None,
};

/// How strong lights are on the theme in use: halved on the light theme,
/// where light on paper is ink, not glow.
pub fn strength() -> f32 {
    if theme::is_light() { 0.5 } else { 1.0 }
}

/// A light's soft layers' alpha, for the theme in use and fade `t`.
fn alpha(a: f32, t: f32) -> f32 {
    a * strength() * t
}

/// A horizontal light across `x` at `y` in `color`, faded by `t` (0 to 1).
pub fn hline(painter: &Painter, light: Light, x: Rangef, y: f32, color: Color32, t: f32) {
    for &(reach, a) in light.halo {
        painter.hline(
            x,
            y,
            Stroke::new(light.line + 2.0 * reach, color.gamma_multiply(alpha(a, t))),
        );
    }
    if light.line > 0.0 {
        painter.hline(x, y, Stroke::new(light.line, color.gamma_multiply(t)));
    }
    if light.core > 0.0 {
        painter.hline(
            x,
            y,
            Stroke::new(light.core, theme::p().core.gamma_multiply(t)),
        );
    }
}

/// A vertical light down `y` at `x`, as [`hline`].
pub fn vline(painter: &Painter, light: Light, x: f32, y: Rangef, color: Color32, t: f32) {
    for &(reach, a) in light.halo {
        painter.vline(
            x,
            y,
            Stroke::new(light.line + 2.0 * reach, color.gamma_multiply(alpha(a, t))),
        );
    }
    if light.line > 0.0 {
        painter.vline(x, y, Stroke::new(light.line, color.gamma_multiply(t)));
    }
    if light.core > 0.0 {
        painter.vline(
            x,
            y,
            Stroke::new(light.core, theme::p().core.gamma_multiply(t)),
        );
    }
}

/// A light along `points` (a fade's curve), its halo multiplied by `lift`
/// while the pointer is on it.
pub fn polyline(painter: &Painter, light: Light, points: Vec<Pos2>, color: Color32, lift: f32) {
    for &(reach, a) in light.halo {
        painter.line(
            points.clone(),
            Stroke::new(
                light.line + 2.0 * reach,
                color.gamma_multiply(alpha(a, lift)),
            ),
        );
    }
    let width = if lift > 1.0 {
        light.line + 0.5
    } else {
        light.line
    };
    painter.line(points, Stroke::new(width, color));
}

/// A light around `rect`: the line inside its edge, the halo outside it.
pub fn rect(painter: &Painter, light: Light, rect: Rect, radius: f32, color: Color32) {
    halo_around(painter, light, rect, radius, color);
    if light.line > 0.0 {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(light.line, color),
            egui::StrokeKind::Inside,
        );
    }
}

/// Only a light's halo, outside `rect` (a lit button keeps its own fill).
pub fn halo_around(painter: &Painter, light: Light, rect: Rect, radius: f32, color: Color32) {
    for &(reach, a) in light.halo {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(reach, color.gamma_multiply(alpha(a, 1.0))),
            egui::StrokeKind::Outside,
        );
    }
}

/// The light a [`BEAM`] casts on the surface under it: a band `width` wide
/// centred on `x` down `y`, strongest at the middle.
pub fn cast(painter: &Painter, light: Light, x: f32, y: Rangef, color: Color32) {
    let Some((width, a)) = light.cast else {
        return;
    };
    let half = width / 2.0;
    let mid = color.gamma_multiply(alpha(a, 1.0));
    let clear = Color32::TRANSPARENT;
    let band = |from: f32, to: f32, left: Color32, right: Color32| {
        let r = Rect::from_x_y_ranges(from..=to, y);
        crate::widgets::gradient(painter, r, left, right);
    };
    band(x - half, x, clear, mid);
    band(x, x + half, mid, clear);
}

/// Where a vertical [`BEAM`] meets the top of `y`: the cap a playhead
/// hangs from, in the core colour with the light's halo.
pub fn cap(painter: &Painter, x: f32, top: f32, color: Color32) {
    let points = |grow: f32| {
        vec![
            pos2(x - 5.0 - grow, top),
            pos2(x + 5.0 + grow, top),
            pos2(x, top + 7.0 + grow),
        ]
    };
    for &(reach, a) in BEAM.halo {
        painter.add(egui::Shape::convex_polygon(
            points(reach),
            color.gamma_multiply(alpha(a, 1.0)),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::convex_polygon(
        points(0.0),
        theme::p().core,
        Stroke::new(1.0, color),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_has_a_source_or_is_a_wash() {
        // A level with a halo has a line for it to come from.
        for light in [LIT, BEAM, FLARE] {
            assert!(light.line > 0.0);
            assert!(!light.halo.is_empty());
            for &(reach, a) in light.halo {
                assert!(reach > 0.0 && (0.0..1.0).contains(&a));
            }
        }
        const { assert!(EMBER_WASH < 0.1) };
        // Only the beam has a core and casts light.
        assert!(BEAM.core > 0.0 && BEAM.core < BEAM.line && BEAM.cast.is_some());
        assert!(LIT.cast.is_none() && FLARE.cast.is_none());
    }

    #[test]
    fn halos_fall_off_outward() {
        for light in [LIT, FLARE] {
            let mut layers = light.halo.to_vec();
            layers.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in layers.windows(2) {
                assert!(pair[1].1 < pair[0].1, "{light:?}");
            }
        }
    }
}
