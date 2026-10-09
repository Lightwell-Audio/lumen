//! The audio status at the end of the transport bar: CPU and dropouts,
//! the device readout and its panel, and the settings window.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use lumen_engine::Command;

use crate::audio::Status;
use crate::theme;

use super::Transport;

impl Transport {
    /// CPU load (the peak over the last second) and, if any, dropouts.
    fn health(&mut self, ui: &mut Ui) {
        let Some(meters) = self.audio.meters().cloned() else {
            return;
        };
        if self
            .cpu_taken
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            self.cpu_peak = meters.take_peak_load();
            self.cpu_taken = Some(Instant::now());
        }
        let cpu = (self.cpu_peak * 100.0).round();
        let color = if self.cpu_peak > 0.7 {
            theme::p().warning
        } else {
            theme::p().text_secondary
        };
        ui.label(
            RichText::new(format!("CPU {cpu:.0}%"))
                .small()
                .monospace()
                .color(color),
        )
        .on_hover_text(
            "Audio processing time as a share of the time each buffer allows, \
                 at its peak over the last second. Above 100% the audio drops out",
        );
        let dropouts = meters.dropouts();
        if dropouts > 0 {
            ui.label(
                RichText::new(format!("⚠ {dropouts}"))
                    .small()
                    .color(theme::p().warning),
            )
            .on_hover_text(format!(
                "{dropouts} dropouts: audio that never reached the device. Open the audio \
                 panel (click the status dot) to see more or reset the count"
            ));
        }
    }

    /// The audio panel: device, Audio On/Off, test tone, meters, panic.
    /// The audio settings window, when open; carries out what it asks.
    pub fn windows(&mut self, ctx: &egui::Context) {
        use crate::audio_settings::Action;
        for action in self.settings_window.show(ctx, &self.audio, self.test_tone) {
            match action {
                Action::Apply(settings) => {
                    crate::trace::log(|| format!("audio: apply {settings:?}"));
                    self.audio.apply(settings);
                    self.settle();
                }
                Action::ControlPanel => self.audio.open_control_panel(),
                Action::DismissNotice => self.audio.dismiss_notice(),
                Action::TestTone(on) => {
                    self.test_tone = on;
                    self.send(Command::TestTone(on));
                }
            }
        }
    }

    fn audio_panel(&mut self, ui: &mut Ui) {
        ui.set_min_width(280.0);
        match &self.audio.status {
            Status::Running { device, detail } => {
                ui.label(RichText::new(device).strong());
                ui.label(RichText::new(detail).small());
            }
            Status::Starting => {
                ui.label("Starting audio…");
            }
            Status::Lost(why) => {
                ui.label(RichText::new("Device lost: reconnecting…").color(theme::p().warning));
                ui.label(RichText::new(why).small());
            }
            Status::Off(why) => {
                ui.label(RichText::new("Audio is off").color(theme::p().warning));
                ui.label(RichText::new(why).small());
            }
        }
        let mut dismiss = false;
        if let Some(notice) = &self.audio.notice {
            ui.horizontal(|ui| {
                ui.set_max_width(260.0);
                let label = ui
                    .add(egui::Label::new(RichText::new(notice).color(theme::p().warning)).wrap());
                if let Some(detail) = &self.audio.notice_detail {
                    label.on_hover_text(detail);
                }
                dismiss = ui
                    .small_button("Dismiss")
                    .on_hover_text("Dismiss")
                    .clicked();
            });
        }
        if dismiss {
            self.audio.dismiss_notice();
        }
        ui.separator();
        let mut on = self.audio.is_on();
        if ui
            .add_enabled(
                self.audio.can_switch(),
                egui::Checkbox::new(&mut on, "Audio on"),
            )
            .on_hover_text("Audio on or off (off frees the device)")
            .changed()
        {
            self.audio.set_on(on);
        }
        if ui
            .button("Audio settings…")
            .on_hover_text("Choose the driver, device, sample rate, buffer and channels")
            .clicked()
        {
            self.settings_window.open = true;
            // Close this panel, so the window is not hidden behind it.
            ui.close();
        }
        if ui
            .checkbox(&mut self.test_tone, "Test tone (1 kHz, −20 dBFS)")
            .on_hover_text("A steady tone on every output, to check the audio path")
            .changed()
        {
            self.send(Command::TestTone(self.test_tone));
        }
        ui.separator();
        if let Some(meters) = self.audio.meters().cloned() {
            egui::Grid::new("audio meters")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("CPU now");
                    ui.label(format!("{:.1}%", meters.load() * 100.0));
                    ui.end_row();
                    ui.label("CPU peak (1 s)");
                    ui.label(format!("{:.1}%", self.cpu_peak * 100.0));
                    ui.end_row();
                    ui.label("Overloads")
                        .on_hover_text("Buffers that took longer to process than they last");
                    ui.label(meters.overloads().to_string());
                    ui.end_row();
                    ui.label("Dropouts").on_hover_text(
                        "Audio that never reached the device: buffers the driver skipped, or \
                     audio delivered slower than real time",
                    );
                    ui.label(meters.dropouts().to_string());
                    ui.end_row();
                    ui.label("Clip audio in memory");
                    ui.label(format!("{:.0} MB", self.clip_memory as f64 / 1_048_576.0));
                    ui.end_row();
                });
            // CPU is a live reading; only the counts can be reset.
            let counted = meters.overloads() + meters.dropouts() > 0;
            if ui
                .add_enabled(
                    counted,
                    egui::Button::new("Reset overload and dropout counts"),
                )
                .on_disabled_hover_text("Nothing to reset: both counts are zero")
                .clicked()
            {
                meters.reset_counters();
            }
        }
        ui.separator();
        if ui
            .button(RichText::new("Panic: stop and silence everything").color(theme::p().record))
            .on_hover_text("Silence everything at once")
            .clicked()
        {
            self.send(Command::Panic);
            self.test_tone = false;
        }
    }

    pub(super) fn status(&mut self, ui: &mut Ui) {
        // In a narrow window the CPU readout gives way first; the status dot
        // (the way into the audio panel) always stays.
        if ui.available_width() > 120.0 {
            self.health(ui);
        }
        let (text, color, tip) = match &self.audio.status {
            Status::Starting => (
                "Starting audio…".to_owned(),
                theme::p().text_hint,
                String::new(),
            ),
            Status::Running { device, detail } => (
                format!("{device} · {detail}"),
                theme::p().text_secondary,
                "Audio device, sample rate, buffer and round-trip latency. Choose the device \
                 with --audio until the device selector arrives"
                    .to_owned(),
            ),
            Status::Lost(why) => (
                "Audio device lost: reconnecting…".to_owned(),
                theme::p().warning,
                why.clone(),
            ),
            Status::Off(why) => ("Audio off".to_owned(), theme::p().warning, why.clone()),
        };
        // A dot that always fits shows the state at a glance; the text
        // shortens in narrow windows, and the tooltip always has all of it.
        let dot = match self.audio.status {
            Status::Running { .. } => theme::p().accent,
            Status::Starting => theme::p().text_hint,
            Status::Lost(_) | Status::Off(_) => theme::p().warning,
        };
        let memory = format!(
            "Clip audio in memory: {:.0} MB",
            self.clip_memory as f64 / 1_048_576.0
        );
        let tip = if tip.is_empty() {
            format!("{text}\n\n{memory}")
        } else {
            format!("{text}\n\n{tip}\n\n{memory}")
        };
        let tip = format!("{tip}\n\nClick for the audio panel");
        let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
        ui.painter().circle_filled(rect.center(), 4.0, dot);
        if response.hovered() {
            ui.painter()
                .circle_stroke(rect.center(), 6.0, egui::Stroke::new(1.0, theme::p().text));
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Audio: {text}"))
        });
        let label = ui
            .add(
                egui::Label::new(RichText::new(text).small().color(color))
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_text(tip.as_str());
        let response = response.on_hover_text(tip).union(label);
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.audio_panel(ui));
    }
}
