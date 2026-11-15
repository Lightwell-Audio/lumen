use eframe::egui::{self, FontId, Sense, Ui, pos2, vec2};

use crate::widgets::{note_change, raised};
use crate::{motion, theme};

pub(super) fn menu_inset(ui: &Ui) -> f32 {
    if egui::containers::menu::is_in_menu(ui) {
        ui.spacing().button_padding.x
    } else {
        0.0
    }
}
pub fn menu_caption(ui: &mut Ui, text: &str) {
    let side = menu_inset(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: side.round() as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| ui.label(egui::RichText::new(text).small().weak()));
}

pub fn menu_header(ui: &mut Ui, text: &str) {
    let side = menu_inset(ui);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: side.round() as i8,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .small()
                        .color(theme::p().text_secondary),
                )
                .truncate(),
            )
        });
    ui.separator();
}

pub fn choice_row(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        palette.text,
    );
    let pad = theme::space::S;
    let needed = galley.size().x + 2.0 * pad;
    let width = if ui.is_sizing_pass() {
        needed
    } else {
        needed.max(ui.available_width())
    };
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::size::CONTROL), Sense::click());
    if response.clicked() && !on {
        note_change(ui);
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, on, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let painter = ui.painter();
    let radius = theme::radius::for_height(rect.height());
    if on {
        painter.rect_filled(rect, f32::from(radius), palette.accent_muted);
    } else if hover > 0.0 {
        painter.rect_filled(rect, f32::from(radius), palette.ember.gamma_multiply(hover));
    }
    let colour = match (enabled, on) {
        (false, _) => palette.text_disabled,
        (true, true) => palette.accent,
        (true, false) => palette.text,
    };
    painter.galley_with_override_text_color(
        pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    raised::focus_ring(painter, &response, rect);
    response
}

pub fn list_row(ui: &mut Ui, label: &str, on: bool, dim: bool) -> egui::Response {
    let palette = theme::p();
    let enabled = ui.is_enabled();
    let pad = theme::space::S;
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::size::CONTROL),
        Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let radius = theme::radius::for_height(rect.height());
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, f32::from(radius), palette.accent_muted);
    } else if hover > 0.0 {
        painter.rect_filled(rect, f32::from(radius), palette.ember.gamma_multiply(hover));
    }
    let colour = match (enabled, on, dim) {
        (false, ..) => palette.text_disabled,
        (true, true, _) => palette.accent,
        (true, false, true) => palette.text_hint,
        (true, false, false) => palette.text,
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        colour,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (rect.width() - 2.0 * pad).max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    painter.galley_with_override_text_color(
        pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    raised::focus_ring(painter, &response, rect);
    response
}
