//! Pill tabs: a row of small rounded-end tabs for switching what a lane or
//! strip shows (the clip editor's controller lane, M5 slice 5; the owner's
//! approved mock-up). The chosen pill is `accent_muted` with a 1 px `accent`
//! edge and `accent` letters; the others are bare words that wash on hover
//! (`FAST`). State is never colour alone: the chosen pill also has its edge.
//!
//! [`pill_tabs`] is for choosing one of several; [`pill_button`] is the
//! dashed "+ CC" pill that opens a menu instead of choosing.

use eframe::egui::{
    self, Id, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;

use crate::{motion, theme};

/// One pill of [`pill_tabs`].
#[derive(Debug, Clone, Copy)]
pub struct Pill<'a> {
    pub label: &'a str,
    /// Said on hover.
    pub tip: &'a str,
}

/// Room either side of a pill's letters.
const PAD: f32 = 11.0;

/// A row of pills, one chosen. Each is named "<label> tab" for assistive
/// technology (as [`super::tabs`] names its tabs). Returns the index clicked,
/// if it is another than `chosen`.
pub fn pill_tabs(ui: &mut Ui, id: Id, chosen: usize, pills: &[Pill<'_>]) -> Option<usize> {
    let mut picked = None;
    ui.spacing_mut().item_spacing.x = theme::space::XS;
    for (i, pill) in pills.iter().enumerate() {
        let on = i == chosen;
        let response = pill_body(ui, id.with(i), pill.label, Some(on), false);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                on,
                format!("{} tab", pill.label),
            )
        });
        if super::tip(response.clone(), pill.tip, None, None).clicked() && !on {
            picked = Some(i);
            super::note_change(ui);
        }
    }
    picked
}

/// A dashed pill that does something rather than choosing (it opens a menu):
/// "+ CC". Named `label`.
pub fn pill_button(ui: &mut Ui, id: Id, label: &str) -> Response {
    let response = pill_body(ui, id, label, None, true);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Lays out and draws one pill. `on` is `None` for a button.
fn pill_body(ui: &mut Ui, id: Id, label: &str, on: Option<bool>, dashed: bool) -> Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        palette.text_secondary,
    );
    let size = vec2(galley.size().x + 2.0 * PAD, theme::size::CONTROL_SMALL);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let ppp = ui.pixels_per_point();
    let rect = rect.round_to_pixels(ppp);
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let lit = motion::hover(ui.ctx(), id.with("lit"), on == Some(true));
    let round = rect.height() / 2.0;
    let painter = ui.painter();
    if hover > 0.0 && lit < 1.0 {
        painter.rect_filled(
            rect,
            round,
            palette.control_hover.gamma_multiply(hover * (1.0 - lit)),
        );
    }
    if lit > 0.0 {
        painter.rect_filled(rect, round, palette.accent_muted.gamma_multiply(lit));
        painter.rect_stroke(
            rect.shrink(0.5),
            round,
            Stroke::new(1.0, palette.accent.gamma_multiply(lit)),
            StrokeKind::Inside,
        );
    } else if dashed {
        dashed_capsule(
            painter,
            rect.shrink(0.5),
            Stroke::new(1.0, palette.border_strong),
        );
    }
    let ink = palette
        .text_secondary
        .lerp_to_gamma(palette.text, hover)
        .lerp_to_gamma(palette.accent, lit);
    let at = (rect.center() - galley.size() / 2.0).round();
    painter.galley_with_override_text_color(at, galley, ink);
    super::raised::focus_ring(painter, &response, rect);
    response
}

/// A dashed outline with fully rounded ends.
fn dashed_capsule(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    const STEPS: usize = 6;
    let r = rect.height() / 2.0;
    let mut points: Vec<Pos2> = Vec::new();
    // Top edge left to right, the right cap, the bottom edge back, the left cap.
    let right = pos2(rect.right() - r, rect.center().y);
    let left = pos2(rect.left() + r, rect.center().y);
    for i in 0..=STEPS {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / STEPS as f32;
        points.push(right + vec2(a.cos(), a.sin()) * r);
    }
    for i in 0..=STEPS {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / STEPS as f32;
        points.push(left + vec2(a.cos(), a.sin()) * r);
    }
    points.push(points[0]);
    painter.extend(Shape::dashed_line(&points, stroke, 3.0, 2.5));
}
