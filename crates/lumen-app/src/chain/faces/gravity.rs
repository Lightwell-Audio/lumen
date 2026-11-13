//! Gravity's screen: the transfer curve over the well its signal falls into (M8
//! slice B2). The picture is a display: the four controls under it are the
//! strip's own row.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{
    FLOOR_DB, Face, METER_W, MOST_REDUCTION, PAD, TOP_DB, mono, reading_of, tint, value_of,
};
use crate::theme::{self, personality};

/// Gravity's screen: the transfer curve over a warped grid, with the threshold
/// line, the ratio and the gain reduction.
pub(super) fn paint(ui: &Ui, rect: Rect, _id: egui::Id, face: &Face, hue: Color32) {
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
        painter.add(Shape::line(
            line,
            Stroke::new(theme::stroke::HAIRLINE, tint(hue, 0.14)),
        ));
        let y = plot.top() + plot.height() * k as f32 / 6.0;
        let line = bow(pos2(plot.left(), y), pos2(plot.right(), y), 16, 1.0);
        painter.add(Shape::line(
            line,
            Stroke::new(theme::stroke::HAIRLINE, tint(hue, 0.14)),
        ));
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
        pos2(plot.left() + 2.0, plot.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
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
    // Its words inside the well: GR, and what it took away.
    let centre = meter.center().x;
    painter.text(
        pos2(centre, meter.bottom() - 15.0),
        Align2::CENTER_CENTER,
        "GR",
        mono(),
        tint(Color32::WHITE, 0.55),
    );
    painter.text(
        pos2(centre, meter.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        format!("\u{2212}{reduction:.1}"),
        mono(),
        tint(Color32::WHITE, 0.9),
    );
}
