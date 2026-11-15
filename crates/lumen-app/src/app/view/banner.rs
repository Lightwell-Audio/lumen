use eframe::egui;

use super::notice::{notice_card, notice_panel_frame};
use crate::widgets::raised;
use crate::{App, audio};

impl App {
    pub(crate) fn device_banner(&mut self, ui: &mut egui::Ui) {
        let audio = &self.transport.audio;
        let current = match &audio.status {
            audio::Status::Lost(_) => Some((
                format!(
                    "{} was disconnected. {} reconnects when it is back.",
                    audio.device,
                    lumen_core::product::PRODUCT_NAME
                ),
                false,
            )),
            audio::Status::Off(_) if audio.is_busy() => Some((
                format!(
                    "{} is in use by another application. Close it there, then try again.",
                    audio.device
                ),
                true,
            )),
            audio::Status::Off(why)
                if !audio.is_on() && why != "Audio is off" && !why.is_empty() =>
            {
                Some((format!("Audio could not start: {why}"), true))
            }
            _ => None,
        };
        let mut open = current.is_some();
        if let Some(current) = current {
            self.banner = Some(current);
        }
        let Some((message, retry)) = self.banner.clone() else {
            return;
        };
        let live = open;
        let shown = egui::Panel::top("device banner")
            .frame(notice_panel_frame())
            .show_collapsible(ui, &mut open, |ui| {
                notice_card(ui, |ui| {
                    ui.label(&message);
                    ui.add_enabled_ui(live, |ui| {
                        let can_retry = retry && self.transport.audio.can_switch();
                        if can_retry && raised::primary_button(ui, "Try again").clicked() {
                            self.transport.audio.set_on(true);
                        }
                        let settings = if can_retry {
                            raised::button(ui, "Audio settings…")
                        } else {
                            raised::primary_button(ui, "Audio settings…")
                        };
                        if settings.clicked() {
                            self.preferences.show_page(crate::preferences::Page::Audio);
                        }
                    });
                });
            });
        if shown.is_none() {
            self.banner = None;
        }
    }
}
