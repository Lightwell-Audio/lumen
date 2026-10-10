//! The audio status at the end of the transport bar: CPU and dropouts,
//! the device readout and its panel, and the settings window.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use lumen_engine::Command;
use lumen_project::model::Project;

use crate::app::info::{Held, Info, Mouse};
use crate::audio::Status;
use crate::{theme, widgets};

use super::Transport;
use super::{compensation, dropouts};

/// What dropouts mean, and what helps.
const DROPOUT_HELP: &str =
    "Audio didn't reach the interface in time: you may have heard a click. A larger buffer helps.";

impl Transport {
    /// CPU load (the peak over the last second) and, after dropouts, the
    /// dropout chip; returns the chip, which opens the audio panel.
    fn health(&mut self, ui: &mut Ui) -> Option<egui::Response> {
        let meters = self.audio.meters().cloned()?;
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
        self.dropout_chip(ui)
    }

    /// "Dropout" or "Dropouts ×3" in amber while dropouts are recent; it
    /// fades out after 10 s of clean audio (owner's A).
    fn dropout_chip(&mut self, ui: &mut Ui) -> Option<egui::Response> {
        let now = Instant::now();
        let showing = self.dropouts.showing(now);
        if let Some(left) = self.dropouts.hides_in(now) {
            ui.ctx().request_repaint_after(left);
        }
        let id = ui.id().with("dropout chip");
        let shown = crate::motion::fade(ui.ctx(), id, showing.is_some());
        if let Some(episode) = showing {
            ui.ctx().data_mut(|d| d.insert_temp(id, episode));
        }
        if shown <= 0.0 {
            return None;
        }
        // While fading out, the episode it last showed.
        let episode = showing.or_else(|| ui.ctx().data(|d| d.get_temp::<u64>(id)))?;
        let text = format!("⚠ {}", dropouts::chip(episode));
        let galley = ui.painter().layout_no_wrap(
            text,
            egui::FontId::proportional(theme::size::CAPTION),
            theme::p().warning.gamma_multiply(shown),
        );
        let pad = egui::vec2(theme::space::S - 1.0, 2.0);
        let (rect, response) =
            ui.allocate_exact_size(galley.size() + 2.0 * pad, egui::Sense::click());
        let painter = ui.painter();
        painter.rect(
            rect,
            theme::radius::SMALL,
            theme::warning_fill().gamma_multiply(shown),
            egui::Stroke::new(1.0, theme::warning_edge().gamma_multiply(shown)),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + pad, galley, theme::p().warning);
        let name = format!("Audio: {}", dropouts::chip(episode));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        Some(
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("{DROPOUT_HELP}\n\nClick for the audio panel")),
        )
    }

    /// The audio panel: device, Audio On/Off, test tone, meters, panic.
    /// The audio settings window, when open; carries out what it asks.
    pub fn windows(
        &mut self,
        ctx: &egui::Context,
        midi: &crate::midi::Midi,
    ) -> Vec<crate::midi::Action> {
        use crate::audio_settings::Action;
        let mut asked = Vec::new();
        for action in self
            .settings_window
            .show(ctx, &self.audio, midi, self.test_tone)
        {
            match action {
                Action::Midi(action) => asked.push(action),
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
        asked
    }

    fn audio_panel(&mut self, ui: &mut Ui, project: &Project) {
        ui.set_min_width(280.0);
        // Long lines (the dropout help, notices) wrap rather than widen it.
        ui.set_max_width(300.0);
        match &self.audio.status {
            Status::Running { device, detail } => {
                ui.label(RichText::new(device).strong());
                ui.label(RichText::new(detail).small());
                self.latency(ui, project);
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
        let can = self.audio.can_switch();
        if widgets::switch(ui, &mut on, "Audio on", can)
            .on_hover_text("Audio on or off (off frees the device)")
            .changed()
        {
            self.audio.set_on(on);
        }
        if ui
            .button("Audio and MIDI settings…")
            .on_hover_text("Choose the driver, device, sample rate, buffer and channels")
            .clicked()
        {
            self.settings_window.open = true;
            // Close this panel, so the window is not hidden behind it.
            ui.close();
        }
        if widgets::switch(ui, &mut self.test_tone, "Test tone (1 kHz, −20 dBFS)", true)
            .on_hover_text("A steady tone on every output, to check the audio path")
            .changed()
        {
            self.send(Command::TestTone(self.test_tone));
        }
        ui.separator();
        if let Some(summary) = dropouts::summary(&self.dropouts, Instant::now()) {
            ui.label(RichText::new(format!("⚠ {summary}")).color(theme::p().warning));
            ui.add(egui::Label::new(RichText::new(DROPOUT_HELP).small()).wrap());
            ui.separator();
        }
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
                    // Which check found them: the driver's position
                    // skipping, or audio arriving slower than real time.
                    let total = meters.dropouts();
                    let slow = meters.slow_dropouts().min(total);
                    let causes = format!(
                        "{DROPOUT_HELP}\n\n{} skipped by the driver, {slow} from audio \
                         arriving slower than real time",
                        total - slow
                    );
                    ui.label("Dropouts").on_hover_text(causes.as_str());
                    ui.label(total.to_string()).on_hover_text(causes);
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

    /// The panel's Latency rows: the device's, then delay compensation
    /// (D1: always here, even at 0) and the path that sets it.
    fn latency(&self, ui: &mut Ui, project: &Project) {
        let rate = self.state().sample_rate;
        let found = self.audio.mixer.compensation();
        let frames = found.frames;
        ui.separator();
        ui.label(RichText::new("Latency").small().strong());
        egui::Grid::new("audio latency")
            .num_columns(2)
            .show(ui, |ui| {
                if let Some(info) = &self.audio.info {
                    ui.label("Output");
                    ui.label(format!(
                        "{:.1} ms",
                        compensation::ms(info.output_latency, info.sample_rate)
                    ));
                    ui.end_row();
                    // As the device line shows it.
                    ui.label("Round trip");
                    ui.label(format!("{:.1} ms", info.round_trip_ms()));
                    ui.end_row();
                }
                ui.label("Delay compensation").on_hover_text(
                    "How far the outputs trail the tracks, so devices that add latency stay \
                     in time. The playhead allows for it.",
                );
                ui.label(format!(
                    "{:.1} ms · {frames} samples",
                    compensation::ms(frames, rate)
                ));
                ui.end_row();
            });
        let path = compensation::path(project, found);
        if !path.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Longest path: {path}"))
                        .small()
                        .color(theme::p().text_secondary),
                )
                .wrap(),
            );
        }
    }

    pub(super) fn status(&mut self, ui: &mut Ui, project: &Project) {
        if let Some(meters) = self.audio.meters() {
            self.dropouts.update(meters.dropouts(), Instant::now());
        }
        // In a narrow window the CPU readout gives way first; the status dot
        // (the way into the audio panel) always stays.
        let chip = if ui.available_width() > 120.0 {
            self.health(ui)
        } else {
            None
        };
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
        // Delay compensation ends the device line while there is some
        // (D1), a step brighter than the rest.
        let rate = self.state().sample_rate;
        let found = self.audio.mixer.compensation().clone();
        let extra = match self.audio.status {
            Status::Running { .. } => compensation::readout(found.frames, rate),
            _ => None,
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
        ui.painter().circle_filled(rect.center(), 4.0, dot);
        if response.hovered() {
            ui.painter()
                .circle_stroke(rect.center(), 6.0, egui::Stroke::new(1.0, theme::p().text));
        }
        let name = match &extra {
            Some(extra) => format!("Audio: {text} · {extra}"),
            None => format!("Audio: {text}"),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        // In a narrow bar the device's details shorten first, so the
        // compensation stays readable.
        let tail = extra.as_ref().map(|extra| {
            let mut tail = egui::text::LayoutJob::default();
            let small = egui::TextStyle::Small.resolve(ui.style());
            let section = |colour| egui::TextFormat::simple(small.clone(), colour);
            tail.append(" · ", 0.0, section(theme::p().text_hint));
            tail.append(extra, 0.0, section(theme::p().text));
            ui.fonts_mut(|fonts| fonts.layout_job(tail))
        });
        let label = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // The bar is cut off before the panel toggles: what shows
                // is up to there.
                let shown = ui
                    .available_width()
                    .min(ui.clip_rect().right() - ui.cursor().min.x)
                    .max(0.0);
                let tail_width = tail.as_ref().map_or(0.0, |t| t.size().x);
                // The device keeps at least its ellipsis.
                let room = (shown - tail_width).max(2.0 * theme::space::M);
                let device = ui
                    .scope(|ui| {
                        ui.set_max_width(room);
                        ui.add(
                            egui::Label::new(RichText::new(text).small().color(color))
                                .truncate()
                                .sense(egui::Sense::click()),
                        )
                    })
                    .inner;
                match tail {
                    Some(tail) => {
                        let left = ui.clip_rect().right() - ui.cursor().min.x;
                        let compensation = ui
                            .scope(|ui| {
                                ui.set_max_width(left.max(0.0));
                                ui.add(
                                    egui::Label::new(tail)
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                )
                            })
                            .inner;
                        device.union(compensation)
                    }
                    None => device,
                }
            })
            .inner
            .on_hover_text(tip.as_str());
        if extra.is_some() && label.hovered() {
            self.info = Some(Info::new(compensation::about(project, &found, rate)).on(
                Held::NONE,
                Mouse::Click,
                "open the audio panel",
            ));
        }
        let mut response = response.on_hover_text(tip).union(label);
        if let Some(chip) = chip {
            response = response.union(chip);
        }
        widgets::Menu::below(&response).show(|ui| self.audio_panel(ui, project));
    }
}
