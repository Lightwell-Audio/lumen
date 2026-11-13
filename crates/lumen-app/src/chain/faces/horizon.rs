//! Horizon's screen: the ceiling drawn as a horizon, the sun behind it and the
//! land under it shaped by a waveform flattened where it reaches the line (M8
//! slice B2). The picture is a display: the four controls under it are the
//! strip's own row.

use eframe::egui::{self, Align2, Color32, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Face, MOST_REDUCTION, PAD, mono, reading_of, tint, value_of};
use crate::theme::{self, personality};

/// Horizon's screen: the ceiling as a horizon, the sun behind it, and the land
/// under it shaped by the wave the limiter holds back.
pub(super) fn paint(ui: &Ui, rect: Rect, _id: egui::Id, face: &Face, hue: Color32) {
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    if rect.height() < 40.0 || rect.width() < 40.0 {
        return;
    }
    let ceiling = value_of(face, lumen_devices::dynamics::limiter::CEILING);
    let detect = value_of(face, lumen_devices::dynamics::limiter::DETECT);
    let (reduction, _) = reading_of(face);
    let sky = Rect::from_min_size(rect.min, rect.size());
    // The sky, from the deep top to the light at the horizon.
    let third = sky.height() / 3.0;
    for (k, colour) in personality::HORIZON_SKY.iter().enumerate() {
        let band = Rect::from_min_max(
            pos2(sky.left(), sky.top() + third * k as f32),
            pos2(sky.right(), sky.top() + third * (k as f32 + 1.0)),
        );
        painter.rect_filled(band, 0.0, *colour);
    }
    painter.rect_filled(
        Rect::from_min_max(pos2(sky.left(), sky.bottom() - 2.0), sky.max),
        0.0,
        personality::HORIZON_SKY[2],
    );

    // Where the ceiling sits: a lower ceiling is a lower horizon.
    let frac = ((ceiling - (-24.0)) / 24.0).clamp(0.0, 1.0);
    let horizon_y = rect.top() + rect.height() * (0.66 - 0.42 * frac);
    let left = rect.left() + PAD * 0.5;
    let right = rect.right() - PAD * 0.5;
    // The sun, low, behind the line.
    let sun = pos2(left + (right - left) * 0.74, horizon_y);
    for (radius, alpha) in [(rect.width() * 0.16, 0.16), (rect.width() * 0.11, 0.30)] {
        painter.circle_filled(
            sun,
            radius,
            personality::HORIZON_LIGHT.gamma_multiply(alpha),
        );
    }
    painter.circle_filled(sun, rect.width() * 0.07, personality::HORIZON_LIGHT);
    // The land: a deterministic wave, flattened where it reaches the line.
    // Each column is a line from the wave down to the foot, so a shape that is
    // not convex fills correctly.
    let steps = ((right - left) / 2.0).max(8.0) as usize;
    let mut peaks = Vec::new();
    let mut crest = Vec::new();
    for k in 0..=steps {
        let t = k as f32;
        let wave = 0.62 + 0.3 * (t / 9.0).sin() * (t / 37.0).sin() + 0.08 * (t / 2.1).sin();
        let room = rect.bottom() - 3.0 - horizon_y;
        let mut y = rect.bottom() - 3.0 - wave.max(0.0) * room * 1.25;
        if y < horizon_y + 1.0 {
            y = horizon_y + 1.0;
            if k % 7 == 0 {
                peaks.push(pos2(left + t, horizon_y));
            }
        }
        crest.push(pos2(left + t, y));
        painter.line_segment(
            [pos2(left + t, y), pos2(left + t, rect.bottom())],
            Stroke::new(2.0, personality::SCREEN.gamma_multiply(0.62)),
        );
    }
    painter.add(Shape::line(crest, Stroke::new(theme::stroke::LIT, hue)));
    // The ceiling itself.
    painter.line_segment(
        [pos2(left, horizon_y), pos2(right, horizon_y)],
        Stroke::new(theme::stroke::FOCUS * 2.0, personality::HORIZON_LIGHT),
    );
    painter.text(
        pos2(left + 2.0, horizon_y - 3.0),
        Align2::LEFT_BOTTOM,
        face.slots
            .first()
            .map_or_else(String::new, |s| format!("{} {}", s.name, s.text)),
        mono(),
        personality::HORIZON_LIGHT,
    );
    for peak in peaks.into_iter().take(5) {
        painter.circle_filled(peak, 2.0, Color32::WHITE);
    }
    // True-peak mode says so, where the light is.
    if detect >= 0.5 {
        painter.text(
            pos2(right - 2.0, horizon_y + 3.0),
            Align2::RIGHT_TOP,
            "TP",
            mono(),
            Color32::WHITE,
        );
    }
    // The gain-reduction meter, a well down the right of the screen, as
    // Gravity's: what the limiter took away, and how much.
    let meter = Rect::from_min_max(
        pos2(rect.right() - PAD - 14.0, rect.top() + PAD * 0.5),
        pos2(rect.right() - PAD, rect.bottom() - 4.0),
    );
    if meter.height() > 24.0 {
        painter.rect_filled(meter, 3.0, personality::SCREEN);
        painter.rect_stroke(
            meter,
            3.0,
            Stroke::new(theme::stroke::HAIRLINE, tint(hue, 0.5)),
            StrokeKind::Inside,
        );
        let fill = (reduction / MOST_REDUCTION).clamp(0.0, 1.0) * (meter.height() - 6.0);
        if fill > 0.5 {
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(meter.left() + 3.0, meter.top() + 3.0),
                    vec2(meter.width() - 6.0, fill),
                ),
                2.0,
                personality::HORIZON_LIGHT,
            );
        }
        painter.text(
            pos2(meter.center().x, meter.bottom() - 15.0),
            Align2::CENTER_CENTER,
            "GR",
            mono(),
            tint(Color32::WHITE, 0.55),
        );
        painter.text(
            pos2(meter.center().x, meter.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            format!("\u{2212}{reduction:.1}"),
            mono(),
            Color32::WHITE,
        );
    }
}
