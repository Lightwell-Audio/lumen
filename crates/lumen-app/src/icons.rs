//! Vector icons (owner request, M2 slice 4). Each icon is geometry on a
//! 16 × 16 grid, scaled to the target rectangle when painted, so it is sharp
//! at every UI scale. They are drawn with egui's painter: at icon sizes that
//! is crisp and cheap, and Vello stays reserved for dense canvases (ADR-0018).

use std::f32::consts::{PI, TAU};

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

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
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 7] = [
        Icon::ToStart,
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::Record,
        Icon::Loop,
        Icon::Metronome,
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
        let background = if active {
            Some(theme::BUTTON_ACTIVE)
        } else if enabled && (response.hovered() || response.has_focus()) {
            Some(theme::BUTTON_HOVER)
        } else {
            None
        };
        if let Some(fill) = background {
            painter.rect_filled(rect, 4.0, fill);
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let color = match (enabled, active, icon) {
            (false, _, _) => theme::ICON_DISABLED,
            (true, _, Icon::Record) => theme::RECORD,
            (true, true, _) => theme::ACCENT,
            (true, false, _) => theme::ICON,
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
