//! The notice card, a banner's panel frame and the card inside it
//! (ADR-0052), shared by every banner in the window.

use eframe::egui::{self, RichText, Stroke};

use crate::theme;

/// The panel frame of a banner under the transport: `app` ground with room
/// around the notice card inside it.
pub(crate) fn notice_panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::p().app)
        .inner_margin(egui::Margin::symmetric(
            theme::space::M as i8,
            theme::space::S as i8,
        ))
}

/// A notice (the Notices card, ADR-0052): `raised` ground with a 1 pt
/// `warning` hairline at 45%, radius `LARGE`, the warning icon, then
/// `add` on one row. Say what happened and what is safe; buttons are
/// `raised::button`, at most one `primary_button` for the main action.
pub(crate) fn notice_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    notice_card_with(ui, None, add);
}

/// A notice with its own icon in place of the warning sign (safe mode's
/// shield), still in the warning colour: it is a state worth noticing.
pub(crate) fn notice_card_with(
    ui: &mut egui::Ui,
    icon: Option<crate::icons::Icon>,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme::p().raised)
        .stroke(Stroke::new(theme::stroke::HAIRLINE, theme::warning_edge()))
        .corner_radius(theme::radius::LARGE)
        .inner_margin(egui::Margin::symmetric(
            theme::space::M as i8,
            theme::space::S as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match icon {
                    Some(icon) => {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        ui.painter()
                            .extend(crate::icons::shapes(icon, rect, theme::p().warning));
                    }
                    None => {
                        ui.label(RichText::new("\u{26A0}").color(theme::p().warning));
                    }
                }
                add(ui);
            });
        });
}
