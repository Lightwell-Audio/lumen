//! The parts of a settings form (Preferences, Project settings, Export): a
//! section heading, a row with its label in the left column, hints and
//! warnings that wrap inside the width.

use eframe::egui::{self, RichText, Ui};

use crate::theme;

/// Width of the label column, so every section lines up.
const LABEL_WIDTH: f32 = theme::size::LABEL_COLUMN;

/// A section heading with even spacing above and below.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(theme::space::M);
    ui.label(RichText::new(title).font(crate::fonts::medium(theme::size::BODY)));
    ui.add_space(theme::space::XS);
}

/// A label in the left column and `contents` beside it.
pub fn row(ui: &mut Ui, label: &str, contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL_WIDTH, theme::size::CONTROL_SMALL));
                // A row without a heading has no label at all: an empty
                // one is a nameless node for screen readers (slice 8).
                if !label.is_empty() {
                    ui.label(RichText::new(label).weak());
                }
            },
        );
        ui.add_space(theme::space::S);
        contents(ui);
    });
}

/// Warning text that wraps within the space left (minus `reserve` for a
/// button beside it), so a long driver message never widens the window.
pub fn warning(ui: &mut Ui, text: &str, reserve: f32) -> egui::Response {
    let width = (ui.available_width() - reserve).max(120.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(RichText::new(text).color(theme::p().warning)).wrap())
    })
    .inner
}

/// A quiet line of help, wrapping within the space left so it never widens
/// the window.
pub fn hint(ui: &mut Ui, text: &str) {
    let width = ui.available_width().max(120.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.add(egui::Label::new(RichText::new(text).small().weak()).wrap());
    });
}

/// A sample rate as it reads in settings: "48.0 kHz".
pub fn khz(rate: u32) -> String {
    format!("{:.1} kHz", f64::from(rate) / 1000.0)
}
