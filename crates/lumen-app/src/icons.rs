//! Vector icons (owner request, M2 slice 4). Each icon is geometry on a
//! 16 × 16 grid, scaled to the target rectangle when painted, so it is sharp
//! at every UI scale. They are drawn with egui's painter: at icon sizes that
//! is crisp and cheap, and Vello stays reserved for dense canvases (ADR-0018).

use std::f32::consts::{PI, TAU};

use eframe::egui::{
    self, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, vec2,
};
use lumen_project::model::TrackKind;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ToStart,
    Play,
    Pause,
    Stop,
    Record,
    Loop,
    Metronome,
    /// Follow the playhead: a playhead line with an arrow.
    Follow,
    /// Snapping on: a horseshoe magnet.
    Magnet,
    /// Snapping off: the magnet, struck through.
    MagnetOff,
    /// A locked clip (M3 slice 6e): a padlock.
    Lock,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 11] = [
        Icon::ToStart,
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::Record,
        Icon::Loop,
        Icon::Metronome,
        Icon::Follow,
        Icon::Magnet,
        Icon::MagnetOff,
        Icon::Lock,
    ];
}

/// Maps the 16-unit icon grid onto a square centred in `rect`.
struct Grid {
    origin: Pos2,
    unit: f32,
}

impl Grid {
    fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height());
        Self {
            origin: rect.center() - Vec2::splat(side / 2.0),
            unit: side / 16.0,
        }
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.unit
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
}

/// The shapes for `icon` in `rect`.
pub fn shapes(icon: Icon, rect: Rect, color: Color32) -> Vec<Shape> {
    let g = Grid::new(rect);
    let stroke = Stroke::new(1.6 * g.unit, color);
    let none = Stroke::NONE;
    match icon {
        Icon::Play => vec![Shape::convex_polygon(
            vec![g.p(4.0, 2.5), g.p(13.5, 8.0), g.p(4.0, 13.5)],
            color,
            none,
        )],
        Icon::Pause => vec![
            Shape::rect_filled(g.rect(3.5, 3.0, 6.5, 13.0), 0.5 * g.unit, color),
            Shape::rect_filled(g.rect(9.5, 3.0, 12.5, 13.0), 0.5 * g.unit, color),
        ],
        Icon::Stop => vec![Shape::rect_filled(
            g.rect(3.5, 3.5, 12.5, 12.5),
            0.75 * g.unit,
            color,
        )],
        Icon::Record => vec![Shape::circle_filled(g.p(8.0, 8.0), 4.75 * g.unit, color)],
        Icon::ToStart => vec![
            Shape::rect_filled(g.rect(3.0, 3.0, 5.0, 13.0), 0.3 * g.unit, color),
            Shape::convex_polygon(
                vec![g.p(13.0, 3.0), g.p(13.0, 13.0), g.p(5.5, 8.0)],
                color,
                none,
            ),
        ],
        Icon::Loop => {
            // An almost-closed circle with an arrowhead at its end.
            let (centre, radius) = (g.p(8.0, 8.0), 5.0 * g.unit);
            let (start, sweep) = (-PI / 2.0 + 0.55, TAU - 1.1);
            let at = |angle: f32| centre + radius * Vec2::angled(angle);
            let arc: Vec<Pos2> = (0..=32)
                .map(|i| at(start + sweep * i as f32 / 32.0))
                .collect();
            let end_angle = start + sweep;
            let tip = at(end_angle);
            let along = Vec2::angled(end_angle + PI / 2.0); // direction of travel
            let across = Vec2::angled(end_angle);
            let head = vec![
                tip + along * 2.6 * g.unit,
                tip + across * 2.2 * g.unit,
                tip - across * 2.2 * g.unit,
            ];
            vec![
                Shape::line(arc, stroke),
                Shape::convex_polygon(head, color, none),
            ]
        }
        Icon::Metronome => vec![
            Shape::closed_line(
                vec![
                    g.p(6.3, 2.5),
                    g.p(9.7, 2.5),
                    g.p(12.0, 13.5),
                    g.p(4.0, 13.5),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(8.0, 11.0), g.p(12.0, 4.0)], stroke),
            Shape::circle_filled(g.p(10.3, 7.0), 1.3 * g.unit, color),
        ],
        Icon::Magnet | Icon::MagnetOff => {
            // A horseshoe: two legs joined by a half circle on top, with
            // pole tips at the bottom.
            let thick = Stroke::new(2.4 * g.unit, color);
            let mut arch: Vec<Pos2> = (0..=12)
                .map(|k| {
                    let a = PI + PI * k as f32 / 12.0;
                    g.p(8.0 + 4.0 * a.cos(), 7.5 + 4.0 * a.sin())
                })
                .collect();
            // The legs stop short of the tips, so the notch shows at 100%.
            arch.insert(0, g.p(4.0, 10.0));
            arch.push(g.p(12.0, 10.0));
            let mut shapes = vec![
                Shape::line(arch, thick),
                Shape::rect_filled(g.rect(2.6, 11.6, 5.4, 14.4), 0.3 * g.unit, color),
                Shape::rect_filled(g.rect(10.6, 11.6, 13.4, 14.4), 0.3 * g.unit, color),
            ];
            if icon == Icon::MagnetOff {
                shapes.push(Shape::line_segment(
                    [g.p(2.0, 2.0), g.p(14.0, 14.0)],
                    stroke,
                ));
            }
            shapes
        }
        Icon::Lock => vec![
            // A shackle over a solid body.
            Shape::line(
                std::iter::once(g.p(5.0, 8.0))
                    .chain((0..=12).map(|k| {
                        let a = PI + PI * k as f32 / 12.0;
                        g.p(8.0 + 3.0 * a.cos(), 6.5 + 3.5 * a.sin())
                    }))
                    .chain([g.p(11.0, 8.0)])
                    .collect(),
                stroke,
            ),
            Shape::rect_filled(g.rect(3.0, 7.5, 13.0, 14.5), 1.0 * g.unit, color),
        ],
        Icon::Follow => vec![
            Shape::line_segment([g.p(4.0, 2.5), g.p(4.0, 13.5)], stroke),
            Shape::line_segment([g.p(6.5, 8.0), g.p(11.5, 8.0)], stroke),
            Shape::convex_polygon(
                vec![g.p(14.0, 8.0), g.p(10.5, 5.0), g.p(10.5, 11.0)],
                color,
                none,
            ),
        ],
    }
}

/// A transport-bar button showing `icon`. `active` marks a toggle that is on
/// (or Play while playing). The accessible name is `label`; the tooltip adds
/// the shortcut.
pub fn button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    shortcut: Option<&str>,
    active: bool,
    enabled: bool,
    disabled_reason: &str,
) -> Response {
    let size = Vec2::new(30.0, 24.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label)
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = theme::p();
        // Hover and press fade in and out (motion: FAST); active is a state,
        // shown at once.
        let hot = enabled && (response.hovered() || response.has_focus());
        let hover = crate::motion::hover(ui.ctx(), response.id.with("hover"), hot);
        let press = crate::motion::hover(
            ui.ctx(),
            response.id.with("press"),
            enabled && response.is_pointer_button_down_on(),
        );
        if active {
            painter.rect_filled(rect, theme::radius::SMALL, c.accent_muted);
        } else if hover > 0.0 {
            let fill = c.control_hover.lerp_to_gamma(c.control_press, press);
            painter.rect_filled(rect, theme::radius::SMALL, fill.gamma_multiply(hover));
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                theme::radius::SMALL,
                Stroke::new(1.5, c.accent),
                egui::StrokeKind::Inside,
            );
        }
        let color = match (enabled, active, icon) {
            (false, _, _) => c.text_disabled,
            (true, _, Icon::Record) => c.record,
            (true, true, _) => c.accent,
            (true, false, _) => c.text,
        };
        for shape in shapes(icon, rect.shrink(4.0), color) {
            painter.add(shape);
        }
    }
    if crate::trace::enabled() {
        let released_here = ui.input(|i| i.pointer.primary_released()) && response.hovered();
        if response.clicked() || response.double_clicked() || released_here {
            crate::trace::log(|| {
                format!(
                    "button {label:?}: clicked {} double {} dragged {} enabled {enabled}",
                    response.clicked(),
                    response.double_clicked(),
                    response.dragged() || response.drag_stopped(),
                )
            });
        }
    }
    let tip = match shortcut {
        Some(keys) => format!("{label} ({keys})"),
        None => label.to_owned(),
    };
    if enabled {
        response.on_hover_text(tip)
    } else {
        response.on_disabled_hover_text(format!("{tip}. {disabled_reason}"))
    }
}

/// The close button's size.
pub const CLOSE_SIZE: f32 = 22.0;

/// A window's close button: an X with the same hover fade as the
/// transport buttons.
pub fn close_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CLOSE_SIZE), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    if ui.is_rect_visible(rect) {
        let c = theme::p();
        let hover = crate::motion::hover(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered() || response.has_focus(),
        );
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                theme::radius::SMALL,
                c.control_hover.gamma_multiply(hover),
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                theme::radius::SMALL,
                Stroke::new(1.5, c.accent),
                egui::StrokeKind::Inside,
            );
        }
        let x = rect.shrink(6.5);
        let stroke = Stroke::new(1.5, c.text_secondary.lerp_to_gamma(c.text, hover));
        ui.painter()
            .line_segment([x.left_top(), x.right_bottom()], stroke);
        ui.painter()
            .line_segment([x.right_top(), x.left_bottom()], stroke);
    }
    response.on_hover_text("Close (Esc)")
}

/// A 14 pt icon for the track type: a waveform for audio, keys for MIDI,
/// stacked bars for a group, a return arrow for a return.
pub fn draw_type_icon(painter: &Painter, center: Pos2, kind: TrackKind, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let at = |x: f32, y: f32| center + vec2(x, y);
    match kind {
        TrackKind::Audio => {
            for (x, h) in [(-5.0, 1.0), (-2.5, 3.5), (0.0, 6.0), (2.5, 2.5), (5.0, 1.0)] {
                painter.line_segment([at(x, -h), at(x, h)], stroke);
            }
        }
        TrackKind::Midi => {
            let r = Rect::from_center_size(center, vec2(12.0, 12.0));
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            for x in [-2.0, 2.0] {
                painter.line_segment([at(x, -6.0), at(x, 1.5)], stroke);
            }
        }
        TrackKind::Group | TrackKind::Master => {
            for y in [-4.0, 0.0, 4.0] {
                painter.line_segment([at(-6.0, y), at(6.0, y)], stroke);
            }
        }
        TrackKind::Return => {
            painter.line_segment([at(5.0, -5.0), at(5.0, 1.0)], stroke);
            painter.line_segment([at(5.0, 1.0), at(-5.0, 1.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, -2.0)], stroke);
            painter.line_segment([at(-5.0, 1.0), at(-2.0, 4.0)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn every_icon_stays_inside_its_box_at_every_scale() {
        for scale in [1.0, 1.5, 2.0] {
            let rect = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(16.0 * scale));
            for icon in Icon::ALL {
                let shapes = shapes(icon, rect, Color32::WHITE);
                assert!(!shapes.is_empty(), "{icon:?}");
                for shape in shapes {
                    let bounds = shape.visual_bounding_rect();
                    assert!(
                        rect.expand(0.5).contains_rect(bounds),
                        "{icon:?} at {scale}: {bounds:?} outside {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn icons_scale_with_their_box() {
        let small = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        let large = Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0));
        for icon in Icon::ALL {
            let width = |r| {
                shapes(icon, r, Color32::WHITE)
                    .iter()
                    .map(Shape::visual_bounding_rect)
                    .fold(Rect::NOTHING, |a, b| a.union(b))
                    .width()
            };
            let ratio = width(large) / width(small);
            assert!((1.8..2.2).contains(&ratio), "{icon:?}: {ratio}");
        }
    }
}
