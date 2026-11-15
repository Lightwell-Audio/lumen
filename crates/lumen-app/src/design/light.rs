use eframe::egui::{self, Color32, Painter, Pos2, Rangef, Rect, Stroke, pos2};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub line: f32,
    pub halo: &'static [(f32, f32)],
    pub core: f32,
    pub cast: Option<(f32, f32)>,
}

pub const EMBER_WASH: f32 = 0.08;

pub const LIT: Light = Light {
    line: 1.5,
    halo: &[(3.0, 0.12), (1.5, 0.28)],
    core: 0.0,
    cast: None,
};

pub const BEAM: Light = Light {
    line: 2.0,
    halo: &[(3.0, 0.22)],
    core: 1.0,
    cast: Some((56.0, 0.09)),
};

pub const FLARE: Light = Light {
    line: 1.5,
    halo: &[(6.0, 0.18), (2.5, 0.45)],
    core: 0.0,
    cast: None,
};

pub fn strength() -> f32 {
    if theme::is_light() { 0.5 } else { 1.0 }
}

fn alpha(a: f32, t: f32) -> f32 {
    a * strength() * t
}

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

pub fn halo_around(painter: &Painter, light: Light, rect: Rect, radius: f32, color: Color32) {
    glow(painter, light, rect, radius, color, 1.0);
}

pub fn glow(painter: &Painter, light: Light, rect: Rect, radius: f32, color: Color32, t: f32) {
    if t <= 0.0 {
        return;
    }
    for &(reach, a) in light.halo {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(reach, color.gamma_multiply(alpha(a, t))),
            egui::StrokeKind::Outside,
        );
    }
}

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
        Stroke::new(theme::stroke::HAIRLINE, color),
    ));
}

#[derive(Clone, Copy)]
struct Seen {
    count: u32,
    at: f64,
}

pub fn flash(ctx: &egui::Context, id: egui::Id, count: u32) -> f32 {
    flash_held(ctx, id, count, false)
}

pub fn flash_held(ctx: &egui::Context, id: egui::Id, count: u32, held: bool) -> f32 {
    let now = ctx.input(|i| i.time);
    if held {
        ctx.data_mut(|d| d.insert_temp(id, Seen { count, at: now }));
        return 1.0;
    }
    let seen = match ctx.data(|d| d.get_temp::<Seen>(id)) {
        None => Seen {
            count,
            at: f64::NEG_INFINITY,
        },
        Some(s) if s.count != count => Seen { count, at: now },
        Some(s) => s,
    };
    ctx.data_mut(|d| d.insert_temp(id, seen));
    let elapsed = now - seen.at;
    if crate::motion::reduced() {
        return if elapsed < crate::motion::STEADY_FLASH {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                crate::motion::STEADY_FLASH - elapsed,
            ));
            1.0
        } else {
            0.0
        };
    }
    let t = (elapsed / crate::motion::NOTE.as_secs_f64()) as f32;
    if t >= 1.0 {
        return 0.0;
    }
    ctx.request_repaint_after(crate::motion::FADE_FRAME);
    let left = 1.0 - t;
    left * left
}

pub fn dot(painter: &Painter, light: Light, center: Pos2, radius: f32, color: Color32, t: f32) {
    if t <= 0.0 {
        return;
    }
    for &(reach, a) in light.halo {
        painter.circle_filled(
            center,
            radius + light.line + reach,
            color.gamma_multiply(alpha(a, t)),
        );
    }
    painter.circle_filled(center, radius + light.line, color.gamma_multiply(t));
    painter.circle_filled(center, radius, theme::p().core.gamma_multiply(t));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_has_a_source_or_is_a_wash() {
        for light in [LIT, BEAM, FLARE] {
            assert!(light.line > 0.0);
            assert!(!light.halo.is_empty());
            for &(reach, a) in light.halo {
                assert!(reach > 0.0 && (0.0..1.0).contains(&a));
            }
        }
        const { assert!(EMBER_WASH < 0.1) };
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
