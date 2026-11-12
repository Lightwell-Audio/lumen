//! The transport bar: its mark, its controls, and the panel toggles that
//! keep their place at its right.

use eframe::egui::{self, vec2};

use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::{App, widgets};

/// The mark's size in the transport bar.
const MARK_SIZE: f32 = 22.0;

impl App {
    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        // The panel toggles keep their place at the right at any width or
        // scale, so their room is set aside first (as wide as they were
        // last frame); the transport takes the rest, and the status text
        // shortens to fit. They are drawn last, so Tab reaches them after
        // the transport, in reading order (slice 8: they came first).
        let full = ui.available_rect_before_wrap();
        let width_id = ui.id().with("panel toggles width");
        let width: f32 = ui.data(|d| d.get_temp(width_id)).unwrap_or(220.0);
        let split = (full.right() - width).max(full.left());
        let left = egui::Rect::from_min_max(full.min, egui::pos2(split, full.bottom()));
        let right = egui::Rect::from_min_max(egui::pos2(split, full.top()), full.max);
        let row = |rect| {
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center))
        };
        let midi = crate::transport::MidiView::gather(
            &self.midi,
            &self.settings.midi,
            self.live_keys
                .as_ref()
                .map_or(0, crate::live::LiveKeys::activity),
            self.live_keys
                .as_ref()
                .is_some_and(crate::live::LiveKeys::held),
        );
        ui.scope_builder(row(left), |ui| {
            // Whatever does not fit is cut off here rather than drawn over
            // the panel toggles.
            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
            self.transport_mark(ui);
            self.app_menu(ui);
            crate::transport::divider(ui);
            self.transport.keyboard_readout =
                self.keyboard.is_on().then(|| self.keyboard.readout());
            self.transport.taps = self.tap.count();
            self.transport
                .ui(ui, &mut self.document, &self.keymap, self.settings.record);
            // What the bar's controls asked for: Record, Overdub, the count-in.
            for command in std::mem::take(&mut self.transport.requests) {
                self.run(ui.ctx(), command);
            }
            if std::mem::take(&mut self.transport.keyboard_clicked) {
                let on = !self.keyboard.is_on();
                self.set_computer_keyboard(on);
            }
        });
        if let Some(info) = self.transport.info.take() {
            self.set_info(info);
        }
        let toggles = ui.scope_builder(row(right), |ui| {
            // The MIDI chip sits with the panel toggles, so it stays in view
            // however narrow the window: the transport is cut off first.
            self.transport.midi_chip(ui, &midi);
            crate::transport::divider(ui);
            let keys = self.keymap.keys(Command::OpenPalette);
            if icons::toggle(
                ui,
                Icon::Search,
                "Command palette",
                widgets::Tip {
                    keys: keys.as_ref(),
                    hint: None,
                    command: Some(Command::OpenPalette),
                },
                self.palette.is_open(),
            )
            .clicked()
            {
                self.toggle_palette();
            }
            // The two panel toggles are quiet like the palette's: an accent
            // icon over a short underline while their panel is open (the
            // interface audit's TB1).
            let panel =
                |ui: &mut egui::Ui, icon, shown: &mut bool, label, hint, (command, keys)| {
                    let tip = widgets::Tip {
                        keys,
                        hint: Some(hint),
                        command: Some(command),
                    };
                    if icons::toggle(ui, icon, label, tip, *shown).clicked() {
                        *shown = !*shown;
                    }
                };
            let keys = self.keymap.keys(Command::ToggleBrowser);
            panel(
                ui,
                Icon::PanelLeft,
                &mut self.show_browser,
                "Browser",
                "Show or hide the browser",
                (Command::ToggleBrowser, keys.as_ref()),
            );
            let keys = self.keymap.keys(Command::ToggleDetail);
            panel(
                ui,
                Icon::PanelBottom,
                &mut self.show_detail,
                "Detail",
                "Show or hide the detail panel",
                (Command::ToggleDetail, keys.as_ref()),
            );
        });
        let used = toggles.response.rect.width();
        if (used - width).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(width_id, used));
            ui.ctx().request_repaint();
        }
    }

    /// The mark before the wordmark (ADR-0043): a meter of the master, its
    /// sun rising with the level. Recording turns it red from M9.
    fn transport_mark(&mut self, ui: &mut egui::Ui) {
        let master = self.document.project().master.id.0;
        let reading = self.arrangement.meters.reading(master);
        let look = widgets::mark::Look::level(reading.rms[0].max(reading.rms[1]), false);
        self.mark = look;
        let (_, rect) = ui.allocate_space(vec2(MARK_SIZE, MARK_SIZE));
        widgets::mark::paint(ui.painter(), rect, look);
    }
}
