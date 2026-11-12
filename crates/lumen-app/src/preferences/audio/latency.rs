//! The Audio page's Latency section rows: the recording offset and an
//! offset measured with another device (M9 slices R2 and R2b).

use super::*;

/// The recording offset field: milliseconds added to the latency the driver
/// reports, for drivers that report it wrongly; and the loopback helper's
/// Measure… button, which unfolds its card.
pub(super) fn record_offset(
    ui: &mut Ui,
    offset_ms: f32,
    measure: &mut bool,
    actions: &mut Vec<Action>,
) {
    row(ui, "Recording offset", |ui| {
        let mut ms = offset_ms;
        let field = ui
            .add(
                egui::DragValue::new(&mut ms)
                    .range(-500.0..=500.0)
                    .update_while_editing(false)
                    .speed(0.1)
                    .max_decimals(1)
                    .suffix(" ms"),
            )
            .on_hover_text(
                "Moves what you record earlier (positive) or later (negative), after the latency the driver reports has been removed. Double-click to reset.",
            );
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Recording offset")
        });
        if crate::widgets::double_click_reset(ui, &field) {
            ms = 0.0;
        }
        if (ms - offset_ms).abs() > f32::EPSILON {
            actions.push(Action::RecordOffset(ms));
        }
        let button = crate::widgets::raised::button(
            ui,
            if *measure { "Hide measuring" } else { "Measure…" },
        )
        .on_hover_text(
            "Measures the real round trip with a cable from an output to an input, and offers              the offset it asks for",
        );
        if button.clicked() {
            *measure = !*measure;
        }
    });
}

/// Says so when the offset was measured on another device setup than the
/// one running.
pub(super) fn measured_elsewhere(ui: &mut Ui, with: &MeasuredWith, running: &MeasuredWith) {
    if with.same_setup(running) {
        return;
    }
    row(ui, "", |ui| {
        let width = ui.available_width().max(120.0);
        ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
            ui.set_max_width(width);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Measured with {} · {} at {}, {} samples. Measure again.",
                        driver_label(&with.driver),
                        with.device_name,
                        khz(with.sample_rate),
                        with.buffer_frames
                    ))
                    .small()
                    .weak(),
                )
                .wrap(),
            )
        });
    });
}

/// A small filled dot, drawn rather than typed (the UI font has no "●").
pub(super) fn dot(ui: &mut Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}
