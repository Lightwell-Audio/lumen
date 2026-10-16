//! Pill tabs: a row of small rounded-end tabs for switching what a lane or
//! strip shows (the clip editor's controller lane, M5 slice 5; the Segmented
//! card's lane pills, ADR-0052). 24 pt high and fully round. The chosen pill
//! is `accent_muted` with a 38% `accent` hairline and `accent` letters; the
//! others are bare words that take the Ember wash on hover (`FAST`). State is
//! never colour alone: the chosen pill also has its hairline.
//!
//! [`pill_tabs`] is for choosing one of several; [`pill_button`] is the
//! "+ CC" pill, a `border_strong` hairline, that opens a menu instead of
//! choosing.

use eframe::egui::{self, Id, Response, Sense, Stroke, StrokeKind, Ui, vec2};
use eframe::emath::GuiRounding as _;

use super::raised;
use crate::{motion, theme};

/// One pill of [`pill_tabs`].
#[derive(Debug, Clone, Copy)]
pub struct Pill<'a> {
    pub label: &'a str,
    /// Said on hover.
    pub tip: &'a str,
}

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

/// A pill that is on or off in a row where one is on at a time, and several
/// may wrap onto lines (the browser's category chips). Named
/// "<label> filter". True when clicked.
pub fn pill_toggle(ui: &mut Ui, id: Id, label: &str, on: bool, tip: &str) -> bool {
    let response = pill_body(ui, id, label, Some(on), false);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            on,
            format!("{label} filter"),
        )
    });
    super::tip(response, tip, None, None).clicked()
}

/// A hairline pill that does something rather than choosing (it opens a
/// menu): "+ CC". Named `label`.
pub fn pill_button(ui: &mut Ui, id: Id, label: &str) -> Response {
    let response = pill_body(ui, id, label, None, true);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Lays out and draws one pill. `on` is `None` for a button.
fn pill_body(ui: &mut Ui, id: Id, label: &str, on: Option<bool>, outlined: bool) -> Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        palette.text_secondary,
    );
    let size = vec2(
        galley.size().x + 2.0 * theme::space::M,
        theme::size::CONTROL,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let ppp = ui.pixels_per_point();
    let rect = rect.round_to_pixels(ppp);
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let lit = motion::hover(ui.ctx(), id.with("lit"), on == Some(true));
    let round = theme::radius::FULL;
    let painter = ui.painter();
    if lit > 0.0 {
        painter.rect_filled(rect, round, palette.accent_muted.gamma_multiply(lit));
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(
                theme::stroke::HAIRLINE,
                palette.accent.gamma_multiply(theme::ON_HAIRLINE * lit),
            ),
            StrokeKind::Inside,
        );
    } else if outlined {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
            StrokeKind::Inside,
        );
    }
    if hover > 0.0 {
        painter.rect_filled(rect, round, palette.ember.gamma_multiply(hover));
    }
    let ink = palette
        .text_secondary
        .lerp_to_gamma(palette.text, hover)
        .lerp_to_gamma(palette.accent, lit);
    let at = (rect.center() - galley.size() / 2.0).round();
    painter.galley_with_override_text_color(at, galley, ink);
    if response.has_focus() {
        raised::paint_focus(painter, rect, theme::radius::FULL);
    }
    response
}
