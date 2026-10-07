//! Design-system controls shared across the app (audit M2): the value
//! slider (drag, Shift for fine, double-click to reset, click and type,
//! arrow keys), the fold arrow, gradients and the accent glow line. The
//! track headers use them now; the mixer (M4) and devices reuse them.

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Key, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2,
    vec2,
};

use crate::{motion, theme};

/// "−3.0 dB", "−∞ dB".
pub fn db_text(db: f32) -> String {
    if db <= -59.9 {
        "\u{2212}\u{221E} dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// "C", "L 30", "R 100".
pub fn pan_text(pan: f32) -> String {
    let n = (pan * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SliderKind {
    /// −60 to +6 dB, filled from the left; resets to 0 dB.
    Volume,
    /// −1 (left) to +1 (right), filled from the centre; resets to centre.
    Pan,
}

impl SliderKind {
    pub fn range(self) -> (f32, f32) {
        match self {
            SliderKind::Volume => (-60.0, 6.0),
            SliderKind::Pan => (-1.0, 1.0),
        }
    }

    pub fn text(self, value: f32) -> String {
        match self {
            SliderKind::Volume => db_text(value),
            SliderKind::Pan => pan_text(value),
        }
    }

    /// Reads typed text: "-6", "−6 dB"; "C", "L 30", "R30", "-30".
    pub fn parse(self, text: &str) -> Option<f32> {
        let text = text.trim().replace('\u{2212}', "-").to_ascii_uppercase();
        match self {
            SliderKind::Volume => text.trim_end_matches("DB").trim().parse().ok(),
            SliderKind::Pan => {
                if text == "C" {
                    return Some(0.0);
                }
                let number = |s: &str| s.trim().parse::<f32>().ok().map(|n| n / 100.0);
                if let Some(rest) = text.strip_prefix('L') {
                    number(rest).map(|n| -n)
                } else if let Some(rest) = text.strip_prefix('R') {
                    number(rest)
                } else {
                    number(&text)
                }
            }
        }
    }
}

/// A slider with its value inside it. Drag (Shift for fine steps),
/// double-click to reset, click then type a value (Enter applies, Esc
/// cancels), or use the arrow keys when it has focus. Returns the value.
pub fn value_slider(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
) -> f32 {
    let palette = *theme::p();
    let (lo, hi) = kind.range();
    let typing_id = id.with("typing");
    let typing: Option<String> = ui.data(|d| d.get_temp(typing_id));
    if let Some(mut text) = typing {
        // Typing a value.
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::proportional(theme::size::CAPTION))
                .horizontal_align(egui::Align::Center)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel && let Some(v) = kind.parse(&text) {
                return v.clamp(lo, hi);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return value;
    }

    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text(match kind {
            SliderKind::Volume => {
                "Volume: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Pan => {
                "Pan: drag (Shift for fine), double-click to centre, or click and type L 30 or R 30"
            }
        });
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            f64::from(value),
            format!("{name}: {}", kind.text(value)),
        )
    });
    let mut out = value;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        let dx = response.drag_delta().x;
        out += dx / rect.width() * (hi - lo) * if fine { 0.1 } else { 1.0 };
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        out = 0.0;
    }
    if response.has_focus() {
        let (left, right, typed) = ui.input(|i| {
            let typed = i.events.iter().find_map(|e| match e {
                egui::Event::Text(t)
                    if t.chars()
                        .all(|c| c.is_ascii_digit() || "-.,lLrRcC".contains(c)) =>
                {
                    Some(t.clone())
                }
                _ => None,
            });
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
                typed,
            )
        });
        let step = (hi - lo) / if fine { 660.0 } else { 66.0 };
        if left {
            out -= step;
        }
        if right {
            out += step;
        }
        if let Some(text) = typed {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
    }
    let out = out.clamp(lo, hi);

    // Drawing.
    let p = ui.painter();
    let hover = motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.dragged(),
    );
    let radius = theme::radius::SMALL;
    p.rect_filled(
        rect,
        radius,
        palette.control.lerp_to_gamma(palette.control_hover, hover),
    );
    let x_at = |v: f32| rect.left() + (v - lo) / (hi - lo) * rect.width();
    match kind {
        SliderKind::Volume => {
            let x = x_at(out);
            p.rect_filled(
                Rect::from_min_max(rect.min, pos2(x, rect.bottom())),
                radius,
                palette.control_press,
            );
            p.vline(
                x,
                rect.shrink2(vec2(0.0, 3.0)).y_range(),
                Stroke::new(2.0, palette.start_marker),
            );
        }
        SliderKind::Pan => {
            let center = rect.center().x;
            let x = x_at(out);
            // Grey at the centre, the accent at the edges (owner request).
            let strength = out.abs().powf(1.2);
            let fill = palette
                .control_press
                .lerp_to_gamma(palette.accent, strength);
            p.rect_filled(
                Rect::from_x_y_ranges(center.min(x)..=center.max(x), rect.y_range()),
                0.0,
                fill,
            );
            p.vline(
                center,
                rect.shrink2(vec2(0.0, 4.0)).y_range(),
                Stroke::new(1.0, palette.text_hint),
            );
        }
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.text(out),
        FontId::proportional(theme::size::CAPTION),
        palette.text,
    );
    if response.has_focus() {
        p.rect_stroke(
            rect,
            radius,
            Stroke::new(1.5, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    out
}

/// The fold arrow: pointing right when folded, down when unfolded (`t`
/// glides from 0 to 1).
pub fn draw_arrow(painter: &Painter, center: Pos2, t: f32, color: Color32) {
    let angle = t * std::f32::consts::FRAC_PI_2;
    let (s, c) = angle.sin_cos();
    let rotate = |x: f32, y: f32| center + vec2(x * c - y * s, x * s + y * c);
    painter.add(egui::Shape::convex_polygon(
        vec![rotate(-2.5, -4.0), rotate(3.5, 0.0), rotate(-2.5, 4.0)],
        color,
        Stroke::NONE,
    ));
}

/// The glowing accent line on a track's resize handle (`t` fades it).
pub fn draw_edge_glow(painter: &Painter, x: egui::Rangef, y: f32, t: f32) {
    let accent = theme::p().accent;
    for (width, alpha) in [(6.0, 0.12), (3.0, 0.25)] {
        painter.hline(x, y, Stroke::new(width, accent.gamma_multiply(alpha * t)));
    }
    painter.hline(x, y, Stroke::new(1.5, accent.gamma_multiply(t)));
}

/// A horizontal gradient from `left` to `right` across `rect`.
pub fn gradient(painter: &Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    let i = mesh.vertices.len() as u32;
    for (pos, color) in [
        (rect.left_top(), left),
        (rect.right_top(), right),
        (rect.right_bottom(), right),
        (rect.left_bottom(), left),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_naturally() {
        assert_eq!(db_text(0.0), "0.0 dB");
        assert_eq!(db_text(-3.0), "\u{2212}3.0 dB");
        assert_eq!(db_text(6.0), "+6.0 dB");
        assert_eq!(db_text(-60.0), "\u{2212}\u{221E} dB");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.3), "L 30");
        assert_eq!(pan_text(1.0), "R 100");
    }

    #[test]
    fn typed_values_are_understood() {
        let v = SliderKind::Volume;
        assert_eq!(v.parse("-6"), Some(-6.0));
        assert_eq!(v.parse("\u{2212}4.5 dB"), Some(-4.5));
        assert_eq!(v.parse("loud"), None);
        let p = SliderKind::Pan;
        assert_eq!(p.parse("c"), Some(0.0));
        assert_eq!(p.parse("L 30"), Some(-0.3));
        assert_eq!(p.parse("r100"), Some(1.0));
        assert_eq!(p.parse("-50"), Some(-0.5));
    }
}
