use eframe::egui::{self, Id, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;

use super::raised;
use crate::{motion, theme};

#[derive(Debug, Clone, Copy)]
pub struct Pill<'a> {
    pub label: &'a str,
    pub tip: &'a str,
}

pub fn pill_tabs(ui: &mut Ui, id: Id, chosen: usize, pills: &[Pill<'_>]) -> Option<usize> {
    let mut picked = None;
    ui.spacing_mut().item_spacing.x = theme::space::XS;
    for (i, pill) in pills.iter().enumerate() {
        let on = i == chosen;
        let response = pill_body(ui, id.with(i), pill.label, Some(on), PILL);
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

pub fn pill_toggle(ui: &mut Ui, id: Id, label: &str, on: bool, tip: &str) -> bool {
    toggle_with(ui, id, (label, on, tip), PILL)
}

pub fn pill_switch(ui: &mut Ui, id: Id, label: &str, name: &str, on: bool) -> Response {
    let response = pill_body(ui, id, label, Some(on), PILL);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, name)
    });
    response
}

pub fn chip_toggle(ui: &mut Ui, id: Id, label: &str, on: bool, tip: &str) -> bool {
    toggle_with(ui, id, (label, on, tip), CHIP)
}

pub fn chip_button(ui: &mut Ui, id: Id, label: &str, menu: bool) -> Response {
    let response = pill_body(
        ui,
        id,
        label,
        None,
        Look {
            arrow: menu,
            ..CHIP
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

fn toggle_with(ui: &mut Ui, id: Id, (label, on, tip): (&str, bool, &str), look: Look) -> bool {
    let response = pill_body(ui, id, label, Some(on), look);
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

pub fn pill_button(ui: &mut Ui, id: Id, label: &str) -> Response {
    let response = pill_body(ui, id, label, None, OUTLINED);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

#[derive(Clone, Copy)]
struct Look {
    pad: f32,
    outlined: bool,
    arrow: bool,
}

const PILL: Look = Look {
    pad: theme::space::M,
    outlined: false,
    arrow: false,
};
const OUTLINED: Look = Look {
    outlined: true,
    ..PILL
};
const CHIP: Look = Look {
    pad: theme::space::S,
    ..PILL
};
const ARROW_ROOM: f32 = theme::space::XS + theme::space::S;

fn pill_body(ui: &mut Ui, id: Id, label: &str, on: Option<bool>, look: Look) -> Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::BODY),
        palette.text_secondary,
    );
    let arrow = if look.arrow { ARROW_ROOM } else { 0.0 };
    let size = vec2(
        galley.size().x + arrow + 2.0 * look.pad,
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
    } else if look.outlined {
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
    let at = (rect.center() - (galley.size() + vec2(arrow, 0.0)) / 2.0).round();
    let words = galley.size().x;
    painter.galley_with_override_text_color(at, galley, ink);
    if look.arrow {
        let tip = pos2(
            at.x + words + ARROW_ROOM - theme::space::S / 2.0,
            rect.center().y,
        );
        super::draw_arrow(painter, tip, 1.0, ink);
    }
    if response.has_focus() {
        raised::paint_focus(painter, rect, theme::radius::FULL);
    }
    response
}
