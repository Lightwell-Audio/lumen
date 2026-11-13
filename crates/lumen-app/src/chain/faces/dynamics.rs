//! Gravity's and Horizon's screens: what each device is, drawn (M8 slice B2).
//!
//! Gravity's screen is the transfer curve — input across, output up — over the
//! warped grid of the well its signal falls into, with the threshold line, the
//! ratio, and a body sitting where the meter says the signal is. Horizon's is
//! a horizon at the ceiling: the sun behind it, and the land under it shaped by
//! a waveform flattened where it reaches the line.
//!
//! Both show the gain reduction the device publishes, and both draw only what
//! is on their screen. The living parts (a value moving) repaint because the
//! card draws every frame the meter or a drag asks for.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};

use super::Face;
use crate::theme as tokens;
use crate::theme::{self, personality};

/// What the curve and the meter are measured against: the loud end, in dBFS.
const TOP_DB: f32 = 0.0;
/// The quiet end of the curve's screen, in dBFS.
const FLOOR_DB: f32 = -36.0;
/// The most reduction the meter shows, in decibels.
const MOST_REDUCTION: f32 = 24.0;
/// The meter's width.
const METER_W: f32 = 22.0;
/// Air round a screen's drawing.
const PAD: f32 = 8.0;

/// A device's hue at `alpha`, for a drawing inside its screen.
fn tint(hue: Color32, alpha: f32) -> Color32 {
    hue.gamma_multiply(alpha)
}

/// The mono font the screens write in.
fn mono() -> FontId {
    FontId::monospace(tokens::size::MONO_SMALL)
}

/// A parameter's value in its own units, from the face's slots.
fn value_of(face: &Face, id: u32) -> f32 {
    let Some(slot) = face.slots.iter().find(|s| s.id == id) else {
        return 0.0;
    };
    lumen_devices::params::find(face.identity().timing.table, id)
        .map_or(0.0, |info| info.value(slot.value as f32))
}

/// The gain reduction to draw, in decibels, and the level that drove it.
fn reading_of(face: &Face) -> (f32, f32) {
    face.reading.map_or((0.0, -120.0), |reading| {
        (
            reading.reduction_db.clamp(0.0, MOST_REDUCTION),
            reading.input_db,
        )
    })
}

/// Gravity's screen: the transfer curve over a warped grid, with the threshold
/// line, the ratio and the gain reduction.
pub(super) fn gravity(ui: &Ui, rect: Rect, _id: egui::Id, face: &Face, hue: Color32) {
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
    if rect.height() < 40.0 {
        return;
    }
    let threshold = value_of(face, lumen_devices::dynamics::compressor::THRESHOLD);
    let ratio = value_of(face, lumen_devices::dynamics::compressor::RATIO);
    let knee = value_of(face, lumen_devices::dynamics::compressor::KNEE);
    let (reduction, level) = reading_of(face);

    let plot = Rect::from_min_max(
        rect.min + vec2(PAD, PAD),
        pos2(
            rect.right() - PAD - METER_W - theme::space::S,
            rect.bottom() - PAD,
        ),
    );
    if plot.width() < 20.0 || plot.height() < 20.0 {
        return;
    }
    // The well: the grid pulled towards a point, so the picture reads as a
    // body falling into it (the specimens' motif).
    let pull = pos2(
        plot.left() + plot.width() * 0.45,
        plot.top() + plot.height() * 0.55,
    );
    let bow = |a: Pos2, b: Pos2, steps: usize, depth: f32| -> Vec<Pos2> {
        (0..=steps)
            .map(|k| {
                let t = k as f32 / steps as f32;
                let mut at = a + (b - a) * t;
                let to_pull = at - pull;
                let weight = (-((to_pull.x / (plot.width() * 0.34)).powi(2)
                    + (to_pull.y / (plot.height() * 0.34)).powi(2)))
                .exp()
                    * depth;
                at -= to_pull * weight * 0.22;
                at
            })
            .collect()
    };
    for k in 0..=6 {
        let x = plot.left() + plot.width() * k as f32 / 6.0;
        let line = bow(pos2(x, plot.top()), pos2(x, plot.bottom()), 12, 1.0);
        painter.add(Shape::line(line, Stroke::new(1.0, tint(hue, 0.14))));
        let y = plot.top() + plot.height() * k as f32 / 6.0;
        let line = bow(pos2(plot.left(), y), pos2(plot.right(), y), 16, 1.0);
        painter.add(Shape::line(line, Stroke::new(1.0, tint(hue, 0.14))));
    }
    // The bottom and left edges, so the curve has an axis.
    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        Stroke::new(theme::stroke::HAIRLINE, tint(hue, 0.30)),
    );
    painter.line_segment(
        [plot.left_bottom(), plot.left_top()],
        Stroke::new(theme::stroke::HAIRLINE, tint(hue, 0.30)),
    );

    let ix = |db: f32| plot.left() + plot.width() * (db - FLOOR_DB) / (TOP_DB - FLOOR_DB);
    let oy = |db: f32| plot.bottom() - plot.height() * (db - FLOOR_DB) / (TOP_DB - FLOOR_DB);

    // Where a signal would be without the device: the straight line.
    painter.extend(Shape::dashed_line(
        &[
            pos2(ix(FLOOR_DB), oy(FLOOR_DB)),
            pos2(ix(TOP_DB), oy(TOP_DB)),
        ],
        Stroke::new(theme::stroke::HAIRLINE, tint(Color32::WHITE, 0.22)),
        3.0,
        3.0,
    ));
    // The curve itself, a sample every two points.
    let mut curve = Vec::new();
    let steps = (plot.width() / 2.0).max(2.0) as usize;
    for k in 0..=steps {
        let db = FLOOR_DB + (TOP_DB - FLOOR_DB) * k as f32 / steps as f32;
        let out = lumen_devices::dynamics::compressor::curve_db(db, threshold, ratio, knee);
        curve.push(pos2(ix(db), oy(out.clamp(FLOOR_DB, TOP_DB))));
    }
    painter.add(Shape::line(
        curve.clone(),
        Stroke::new(theme::stroke::FOCUS * 2.0, tint(hue, 0.18)),
    ));
    painter.add(Shape::line(curve, Stroke::new(theme::stroke::LIT, hue)));
    // The threshold line and its value.
    let at = ix(threshold.clamp(FLOOR_DB, TOP_DB));
    painter.extend(Shape::dashed_line(
        &[pos2(at, plot.top() + 2.0), pos2(at, plot.bottom() - 2.0)],
        Stroke::new(theme::stroke::HAIRLINE, tint(Color32::WHITE, 0.30)),
        3.0,
        4.0,
    ));
    painter.text(
        pos2(at + 4.0, plot.top() + 2.0),
        Align2::LEFT_TOP,
        face.slots
            .first()
            .map_or_else(String::new, |s| s.text.clone()),
        mono(),
        tint(Color32::WHITE, 0.62),
    );
    painter.text(
        pos2(plot.right() - 2.0, plot.top() + 2.0),
        Align2::RIGHT_TOP,
        face.slots
            .get(1)
            .map_or_else(String::new, |s| s.text.clone()),
        mono(),
        tint(Color32::WHITE, 0.55),
    );
    // The body: where the meter says the signal is, or an inviting place when
    // it is silent.
    let live = level > FLOOR_DB + 1.0;
    let shown = if live { level } else { -8.0 };
    let out = lumen_devices::dynamics::compressor::curve_db(shown, threshold, ratio, knee);
    let body = pos2(
        ix(shown.clamp(FLOOR_DB, TOP_DB)),
        oy(out.clamp(FLOOR_DB, TOP_DB)),
    );
    for (ring, alpha) in [(9.0, 0.10), (6.0, 0.22), (4.0, 0.55)] {
        painter.circle_filled(body, ring, tint(hue, alpha));
    }
    painter.circle_filled(body, 2.5, Color32::WHITE);

    // The gain-reduction meter: a well at the right, filled from its top.
    let meter = Rect::from_min_max(
        pos2(rect.right() - PAD - METER_W, plot.top()),
        pos2(rect.right() - PAD, plot.bottom()),
    );
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
            hue,
        );
    }
    let centre = meter.center().x;
    painter.text(
        pos2(centre, meter.top() - 1.0),
        Align2::CENTER_BOTTOM,
        "GR",
        mono(),
        tint(Color32::WHITE, 0.45),
    );
    painter.text(
        pos2(centre, meter.bottom() + PAD - 2.0),
        Align2::CENTER_TOP,
        format!("\u{2212}{reduction:.1}"),
        mono(),
        tint(Color32::WHITE, 0.85),
    );
}

/// Horizon's screen: the ceiling as a horizon, the sun behind it, and the land
/// under it shaped by the wave the limiter holds back.
pub(super) fn horizon(ui: &Ui, rect: Rect, _id: egui::Id, face: &Face, hue: Color32) {
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
    let sun = pos2(left + (right - left) * 0.62, horizon_y);
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
            [pos2(left + t, y), pos2(left + t, rect.bottom() - 2.0)],
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
    // The gain-reduction meter, a slim well at the right of the sky.
    let meter = Rect::from_min_max(
        pos2(rect.right() - PAD * 0.5 - 4.0, rect.top() + PAD),
        pos2(rect.right() - PAD * 0.5, horizon_y - 6.0),
    );
    if meter.height() > 8.0 {
        painter.rect_filled(meter, 2.0, personality::SCREEN.gamma_multiply(0.7));
        let fill = (reduction / MOST_REDUCTION).clamp(0.0, 1.0) * meter.height();
        if fill > 0.5 {
            painter.rect_filled(
                Rect::from_min_max(pos2(meter.left(), meter.bottom() - fill), meter.max),
                2.0,
                personality::HORIZON_LIGHT,
            );
        }
    }
}
