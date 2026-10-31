//! The audio status at the end of the transport bar: CPU and dropouts,
//! the device readout and its panel, and the settings window.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use eframe::emath::GuiRounding as _;

use lumen_engine::Command;
use lumen_project::model::Project;

use crate::app::info::{Held, Info, Mouse};
use crate::audio::Status;
use crate::{theme, widgets};

use super::Transport;
use super::{compensation, dropouts};

/// The audio light's radius, in the status chip.
const DOT_RADIUS: f32 = theme::size::STATUS_DOT;

/// What dropouts mean, and what helps.
const DROPOUT_HELP: &str =
    "Audio didn't reach the interface in time: you may have heard a click. A larger buffer helps.";

/// What a late disk read means, and what helps (M9 slice R6).
const DISK_HELP: &str = "A file playing from disk was read too late, so it was silent for a \
     moment. A faster disk helps, as does turning off Play audio from disk in the audio settings.";

impl Transport {
    /// Shows `peak` as the CPU load until the next reading, a second on.
    pub(crate) fn show_cpu_load(&mut self, peak: f32) {
        self.cpu_peak = peak;
        self.cpu_taken = Some(Instant::now());
    }

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
        if self.fit.plan.cpu {
            let before = ui.cursor().left();
            let cpu = (self.cpu_peak * 100.0).round();
            let color = if self.cpu_peak > 0.7 {
                theme::p().warning
            } else {
                theme::p().text_secondary
            };
            // Martian Mono like the other readouts (the Transport card), in
            // a slot as wide as "CPU 100%" with the figure on its right, so
            // the audio chip after it never moves as the load changes
            // (up to 999%, the most the slot holds).
            let font = egui::FontId::monospace(theme::size::BODY);
            let slot = super::fit::text_width(ui, "CPU 100%", font.clone());
            let text = format!("CPU {:.0}%", cpu.min(999.0));
            let galley = ui.painter().layout_no_wrap(text.clone(), font, color);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(slot, galley.size().y), egui::Sense::hover());
            let at = egui::pos2(rect.right() - galley.size().x, rect.top());
            ui.painter().galley(at, galley, color);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text.clone())
            });
            response.on_hover_text(
                "Audio processing time as a share of the time each buffer allows, \
                 at its peak over the last second. Above 100% the audio drops out",
            );
            self.fit.took(ui, before);
        }
        if !self.fit.plan.dropout {
            return None;
        }
        let before = ui.cursor().left();
        let chip = self.dropout_chip(ui);
        self.fit.took(ui, before);
        chip
    }

    /// The widths the CPU readout, the dropout chip and the delay
    /// compensation readout want in the bar (0 when one has nothing to
    /// show), for `fit`.
    pub(super) fn status_wants(&self, ui: &Ui) -> (f32, f32, f32) {
        let gap = super::fit::gap(ui);
        let small = egui::TextStyle::Small.resolve(ui.style());
        let cpu = if self.audio.meters().is_some() {
            let font = egui::FontId::monospace(theme::size::BODY);
            super::fit::text_width(ui, "CPU 100%", font) + gap
        } else {
            0.0
        };
        let dropout = self.warning(Instant::now()).map_or(0.0, |(text, _)| {
            let font = egui::FontId::proportional(theme::size::CAPTION);
            let text = format!("\u{26A0} {text}");
            super::fit::text_width(ui, &text, font) + 2.0 * theme::space::S + gap
        });
        let compensation = match self.audio.status {
            Status::Running { .. } => {
                let rate = self.state().sample_rate;
                let frames = self.audio.mixer.compensation().frames;
                compensation::readout(frames, rate).map_or(0.0, |extra| {
                    super::fit::text_width(ui, &format!(" \u{b7} {extra}"), small)
                })
            }
            _ => 0.0,
        };
        (cpu, dropout, compensation)
    }

    /// What the warning chip says while dropouts or late disk reads are
    /// recent, and what it means: dropouts first, as the worse news.
    fn warning(&self, now: Instant) -> Option<(String, &'static str)> {
        if let Some(episode) = self.dropouts.showing(now) {
            return Some((dropouts::chip(episode), DROPOUT_HELP));
        }
        let episode = self.late.showing(now)?;
        Some((late_chip(episode), DISK_HELP))
    }

    /// "Dropout" or "Dropouts ×3" in amber while dropouts are recent, or
    /// "Disk late" after an underrun (M9 slice R6); it fades out after 10 s
    /// of clean audio (owner's A).
    fn dropout_chip(&mut self, ui: &mut Ui) -> Option<egui::Response> {
        let now = Instant::now();
        let showing = self.warning(now);
        for left in [self.dropouts.hides_in(now), self.late.hides_in(now)]
            .into_iter()
            .flatten()
        {
            ui.ctx().request_repaint_after(left);
        }
        let id = ui.id().with("dropout chip");
        let shown = crate::motion::fade(ui.ctx(), id, showing.is_some());
        if let Some(warning) = &showing {
            ui.ctx().data_mut(|d| d.insert_temp(id, warning.clone()));
        }
        if shown <= 0.0 {
            return None;
        }
        // While fading out, what it last showed.
        let (words, help) =
            showing.or_else(|| ui.ctx().data(|d| d.get_temp::<(String, &'static str)>(id)))?;
        let text = format!("⚠ {words}");
        let galley = ui.painter().layout_no_wrap(
            text,
            egui::FontId::proportional(theme::size::CAPTION),
            theme::p().warning.gamma_multiply(shown),
        );
        // A pill, 20 pt tall (the Notices card).
        let size = egui::vec2(
            galley.size().x + 2.0 * theme::space::S,
            theme::size::CONTROL_SMALL,
        );
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter();
        painter.rect(
            rect,
            theme::radius::FULL,
            theme::warning_fill().gamma_multiply(shown),
            egui::Stroke::new(
                theme::stroke::HAIRLINE,
                theme::warning_edge().gamma_multiply(shown),
            ),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            (rect.center() - galley.size() / 2.0).round(),
            galley,
            theme::p().warning,
        );
        let name = format!("Audio: {words}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        Some(
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(format!("{help}\n\nClick for the audio panel")),
        )
    }

    /// What the MIDI chip's popover asked for since the last frame.
    pub fn take_midi_asked(&mut self) -> Vec<crate::midi::Action> {
        std::mem::take(&mut self.midi_asked)
    }

    /// The test tone is on.
    pub fn test_tone(&self) -> bool {
        self.test_tone
    }

    /// Why the loopback helper cannot measure now, if it cannot.
    pub fn measure_held(&self) -> Option<&'static str> {
        let snap = self.audio.engine.transport();
        (snap.playing || snap.recording || snap.count_in_left > 0)
            .then_some("Stop playback to measure")
    }

    /// Carries out what the Audio page of Preferences asked of the device:
    /// the app keeps the rest (the offset, Play audio from disk), and gets
    /// those back.
    pub fn audio_action(
        &mut self,
        action: crate::preferences::audio::Action,
    ) -> Option<crate::preferences::audio::Action> {
        use crate::preferences::audio::Action;
        match action {
            Action::Apply(settings) => {
                crate::trace::log(|| format!("audio: apply {settings:?}"));
                self.audio.apply(settings);
                self.settle();
            }
            Action::ControlPanel => self.audio.open_control_panel(),
            Action::DismissNotice => self.audio.dismiss_notice(),
            Action::Measure(cable) => {
                crate::trace::log(|| format!("audio: measure the round trip {cable:?}"));
                self.audio.measure(cable);
            }
            Action::TestTone(on) => {
                self.test_tone = on;
                self.send(Command::TestTone(on));
            }
            kept @ (Action::RecordOffset(_) | Action::UseMeasured(..) | Action::FromDisk(_)) => {
                return Some(kept);
            }
        }
        None
    }

    fn audio_panel(&mut self, ui: &mut Ui, project: &Project) {
        ui.set_min_width(280.0);
        // Long lines (the dropout help, notices) wrap rather than widen it.
        ui.set_max_width(300.0);
        // With a long plugin list the panel can be taller than the room
        // under the bar in a small window: then it scrolls (ADR-0075).
        // The plugin list gives way first, down to two rows.
        let room = ui.ctx().content_rect().bottom() - ui.cursor().top() - theme::space::L;
        self.budget.list_room = room - self.budget.outside;
        // The shared menu owns scrolling; a second scroll area here would
        // reserve width twice and clip the budget labels at the right.
        let top = ui.cursor().top();
        self.budget.list_height = 0.0;
        self.audio_panel_rows(ui, project);
        self.budget.outside = ui.cursor().top() - top - self.budget.list_height;
    }

    fn audio_panel_rows(&mut self, ui: &mut Ui, project: &Project) {
        match &self.audio.status {
            Status::Running { device, .. } => {
                ui.label(RichText::new(device).strong());
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
                dismiss = crate::widgets::raised::button(ui, "Dismiss")
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
        if crate::widgets::raised::button(ui, "Audio settings…")
            .on_hover_text(
                "Choose the driver, device, sample rate, buffer and channels in Preferences",
            )
            .clicked()
        {
            self.open_preferences = Some(crate::preferences::Page::Audio);
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
        if self.budget.shown.is_some() {
            super::budget_view::show(ui, &mut self.budget);
            ui.separator();
        }
        if let Some(summary) = dropouts::summary(&self.dropouts, Instant::now()) {
            ui.label(RichText::new(format!("⚠ {summary}")).color(theme::p().warning));
            ui.add(egui::Label::new(RichText::new(DROPOUT_HELP).small()).wrap());
            ui.separator();
        }
        if let Some(summary) = late_summary(&self.late, Instant::now()) {
            ui.label(RichText::new(format!("⚠ {summary}")).color(theme::p().warning));
            ui.add(egui::Label::new(RichText::new(DISK_HELP).small()).wrap());
            ui.separator();
        }
        if let Some(meters) = self.audio.meters().cloned() {
            egui::Grid::new("audio meters")
                .num_columns(2)
                .show(ui, |ui| {
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
                    // Only while audio plays from disk (the owner's D A).
                    if let Some(disk) = self.disk {
                        let late = disk.underruns.saturating_sub(self.disk_base);
                        let tip = "The share of the last second spent reading files that play \
                                   from disk, and the reads that came too late since audio \
                                   started or the counts were reset";
                        ui.label("Disk").on_hover_text(tip);
                        let mut text = format!("{:.0}% · {late} late", disk.load * 100.0);
                        if disk.failed > 0 {
                            text.push_str(&format!(" · {} unreadable", disk.failed));
                        }
                        ui.label(text).on_hover_text(tip);
                        ui.end_row();
                    }
                });
            // CPU is a live reading; only the counts can be reset.
            let late = self
                .disk
                .map_or(0, |d| d.underruns.saturating_sub(self.disk_base));
            let counted = meters.overloads() + meters.dropouts() + late > 0;
            if ui
                .add_enabled(counted, |ui: &mut egui::Ui| {
                    crate::widgets::raised::button(ui, "Reset overload and dropout counts")
                })
                .on_disabled_hover_text("Nothing to reset: both counts are zero")
                .clicked()
            {
                meters.reset_counters();
                if let Some(disk) = self.disk {
                    self.disk_base = disk.underruns;
                }
            }
        }
        ui.separator();
        if ui
            .add(|ui: &mut egui::Ui| {
                crate::widgets::raised::button_inked(
                    ui,
                    "Panic: stop and silence everything",
                    Some(theme::p().record),
                )
            })
            .on_hover_text("Silence everything at once")
            .clicked()
        {
            self.panic();
            self.test_tone = false;
        }
    }

    /// The device's rows, one label and value each (the interface audit's
    /// T2): rate, buffer and latencies, then delay compensation (D1:
    /// always here, even at 0) and the path that sets it.
    fn latency(&self, ui: &mut Ui, project: &Project) {
        let rate = self.state().sample_rate;
        let found = self.audio.mixer.compensation();
        let frames = found.frames;
        egui::Grid::new("audio latency")
            .num_columns(2)
            .show(ui, |ui| {
                if let Some(info) = &self.audio.info {
                    ui.label("Sample rate");
                    ui.label(format!("{:.1} kHz", f64::from(info.sample_rate) / 1000.0));
                    ui.end_row();
                    ui.label("Buffer");
                    ui.label(format!("{} samples", info.buffer_frames));
                    ui.end_row();
                    ui.label("Output latency");
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
        let late = self
            .disk
            .map_or(0, |d| d.underruns.saturating_sub(self.disk_base));
        self.late.update(late, Instant::now());
        // What shows beyond the dot is `fit`'s to say (transport/fit.rs).
        let chip = self.health(ui);
        let (text, color, tip) = match &self.audio.status {
            Status::Starting => (
                "Starting audio…".to_owned(),
                theme::p().text_hint,
                String::new(),
            ),
            Status::Running { device, detail, .. } => (
                format!("{device} · {detail}"),
                theme::p().text_secondary,
                "Audio device, sample rate, buffer and round-trip latency".to_owned(),
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
        let hidden_dropouts = !self.fit.plan.dropout && self.warning(Instant::now()).is_some();
        let dot = match self.audio.status {
            // The dropout chip gave way: the dot says so.
            Status::Running { .. } if hidden_dropouts => theme::p().warning,
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
        let name = match &extra {
            Some(extra) => format!("Audio: {text} · {extra}"),
            None => format!("Audio: {text}"),
        };
        // The chip shows the device and its rate; the buffer and the round
        // trip are in the tooltip and the panel (the interface audit's TB1).
        let shown = match &self.audio.status {
            Status::Running { device, rate, .. } => format!("{device} · {rate}"),
            _ => text,
        };
        let small = egui::TextStyle::Small.resolve(ui.style());
        let gap = theme::space::XS;
        // The device text, shortened to its room (`fit`'s plan).
        let device = (self.fit.plan.device > gap).then(|| {
            let mut job = egui::text::LayoutJob::single_section(
                shown,
                egui::TextFormat::simple(small.clone(), color),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(self.fit.plan.device - gap);
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        });
        // The compensation readout, a step brighter than the device text.
        let tail = extra
            .as_ref()
            .filter(|_| self.fit.plan.compensation)
            .map(|extra| {
                let mut tail = egui::text::LayoutJob::default();
                let section = |colour| egui::TextFormat::simple(small.clone(), colour);
                tail.append(" · ", 0.0, section(theme::p().text_hint));
                tail.append(extra, 0.0, section(theme::p().text));
                ui.fonts_mut(|fonts| fonts.layout_job(tail))
            });
        // What the text takes beyond the dot is `fit`'s optional room.
        let texts = device.as_ref().map_or(0.0, |g| gap + g.size().x)
            + tail.as_ref().map_or(0.0, |g| g.size().x);
        // A raised chip holding the light and the text: the light always
        // fits and shows the state at a glance (the owner kept it, TB1).
        let pad = theme::space::S;
        let light = 2.0 * DOT_RADIUS;
        let size = egui::vec2(2.0 * pad + light + texts, theme::size::CONTROL);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        self.fit.spent(texts);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if ui.is_rect_visible(rect) {
            let open = widgets::Menu::is_open_for(ui.ctx(), response.id);
            let hover = crate::motion::hover(
                ui.ctx(),
                response.id.with("hover"),
                response.hovered() || response.has_focus() || open,
            );
            let rect = rect.round_to_pixels(ui.pixels_per_point());
            let face = widgets::raised::Face::new(theme::controls(false))
                .hover(hover)
                .pressed(response.is_pointer_button_down_on());
            let radius = theme::radius::for_height(rect.height());
            widgets::raised::paint(ui.painter(), rect, radius, &face);
            widgets::raised::focus_ring(ui.painter(), &response, rect);
            let painter = ui.painter();
            let centre = egui::pos2(rect.left() + pad + DOT_RADIUS, rect.center().y);
            painter.circle_filled(centre, DOT_RADIUS, dot);
            let mut x = rect.left() + pad + light;
            if device.is_some() {
                x += gap;
            }
            for galley in [device, tail].into_iter().flatten() {
                let at = egui::pos2(x, rect.center().y - galley.size().y / 2.0).round();
                x += galley.size().x;
                painter.galley(at, galley, color);
            }
        }
        if extra.is_some() && response.hovered() {
            self.info = Some(Info::new(compensation::about(project, &found, rate)).on(
                Held::NONE,
                Mouse::Click,
                "open the audio panel",
            ));
        }
        let mut response = response.on_hover_text(tip);
        if let Some(chip) = chip {
            response = response.union(chip);
        }
        widgets::Menu::below(&response).show(|ui| self.audio_panel(ui, project));
    }
}

/// The chip's words after late disk reads: "Disk late", or "Disk late ×3".
fn late_chip(episode: u64) -> String {
    if episode == 1 {
        "Disk late".into()
    } else {
        format!("Disk late ×{episode}")
    }
}

/// The panel's warning while late disk reads are recent: "2 late disk reads
/// just now · 5 since audio started".
fn late_summary(late: &dropouts::Dropouts, now: Instant) -> Option<String> {
    let episode = late.showing(now)?;
    let plural = if episode == 1 { "read" } else { "reads" };
    let since = if late.was_reset() {
        "since you reset"
    } else {
        "since audio started"
    };
    Some(format!(
        "{episode} late disk {plural} just now · {} {since}",
        late.total()
    ))
}
