use eframe::egui::{self, RichText, Ui};

use crate::theme;

const LABEL_WIDTH: f32 = theme::size::LABEL_COLUMN;

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(theme::space::M);
    ui.label(RichText::new(title).font(crate::fonts::medium(theme::size::BODY)));
    ui.add_space(theme::space::XS);
}

pub fn row(ui: &mut Ui, label: &str, contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL));
                if !label.is_empty() {
                    ui.label(RichText::new(label).weak());
                }
            },
        );
        ui.add_space(theme::space::S);
        contents(ui);
    });
}

pub fn warning(ui: &mut Ui, text: &str, reserve: f32) -> egui::Response {
    let width = (ui.available_width() - reserve).max(120.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(RichText::new(text).color(theme::p().warning)).wrap())
    })
    .inner
}

pub fn hint(ui: &mut Ui, text: &str) {
    let width = ui.available_width().max(120.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(RichText::new(text).small().weak()).wrap());
    });
}

pub fn khz(rate: u32) -> String {
    format!("{:.1} kHz", f64::from(rate) / 1000.0)
}
