//! The transport bar (spec §5, owner request for vector icons): go to start,
//! stop, play/pause, record, loop and metronome; tempo; time signature; an
//! editable position; audio status. Drives `lumen-engine`.
//!
//! Shortcuts (Ableton-compatible): Space play/stop · Shift+Space pause or
//! continue · Home go to start. They are ignored while typing in a field.

use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, RichText, Ui};
use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{ClickSound, Command, MetronomeSettings, Region, Timeline, TransportSnapshot};
use lumen_project::model::{Project, TimeSignature};

use crate::audio::{Audio, Choice, Status};
use crate::icons::{self, Icon};
use crate::{position, theme};

const DENOMINATORS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

pub struct Transport {
    pub audio: Audio,
    /// The UI's copy of what the engine plays against.
    pub timeline: Timeline,
    /// A new timeline is waiting to be sent (the queue was full).
    timeline_pending: bool,
    pub loop_region: Region,
    /// Text while the position field is being edited.
    editing: Option<String>,
    error: Option<String>,
    /// Keep drawing every frame until then, so the result of a command shows
    /// at once rather than at the next idle repaint.
    settle_until: Option<Instant>,
    /// Bytes of clip audio in memory, shown in the audio panel.
    pub clip_memory: usize,
    pub metronome: MetronomeSettings,
    test_tone: bool,
    pub settings_window: crate::audio_settings::Window,
    /// CPU load shown: the peak over the last second, refreshed once a second.
    cpu_peak: f32,
    cpu_taken: Option<Instant>,
}

fn timeline_of(project: &Project) -> Timeline {
    let tempo = TempoMap::new(project.tempo.clone())
        .unwrap_or_else(|_| TempoMap::constant(120.0).expect("120 BPM is valid"));
    let sig = project.time_signature;
    let meter = MeterMap::constant(Meter {
        numerator: sig.numerator.into(),
        denominator: sig.denominator.into(),
    })
    .unwrap_or_else(|_| MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is valid"));
    Timeline::new(tempo, meter)
}

impl Transport {
    pub fn new(project: &Project, choice: &Choice) -> Self {
        let timeline = timeline_of(project);
        // Until the ruler can set it (M3), the loop covers bars 1–4.
        let loop_region = Region::new(Ticks::ZERO, timeline.meter.bar_start(4), false);
        Self {
            audio: Audio::start(choice, timeline.clone()),
            timeline,
            timeline_pending: false,
            loop_region,
            editing: None,
            error: None,
            settle_until: None,
            metronome: MetronomeSettings::default(),
            test_tone: false,
            settings_window: Default::default(),
            cpu_peak: 0.0,
            cpu_taken: None,
            clip_memory: 0,
        }
    }

    pub fn state(&self) -> TransportSnapshot {
        self.audio.engine.transport()
    }

    /// Whether the UI should redraw every frame: while playing, and briefly
    /// after a command until the engine has applied it.
    pub fn animating(&self) -> bool {
        self.state().playing || self.settle_until.is_some_and(|t| Instant::now() < t)
    }

    fn settle(&mut self) {
        // The engine applies commands at its next block (a few ms); the
        // margin covers large buffers.
        self.settle_until = Some(Instant::now() + Duration::from_millis(300));
    }

    fn send(&mut self, command: Command) {
        crate::trace::log(|| format!("send {command:?}"));
        self.settle();
        // A full queue means the engine is not running; the UI shows why.
        let _ = self.audio.engine.send(command);
    }

    pub fn play_stop(&mut self) {
        let command = if self.state().playing {
            Command::Stop
        } else {
            Command::Play
        };
        self.send(command);
    }

    /// Applies saved metronome settings.
    pub fn restore_metronome(&mut self, settings: MetronomeSettings) {
        self.metronome = settings;
        self.send(Command::SetMetronome(settings));
    }

    /// Shift+Space: stop where playback is, or continue from the playhead.
    pub fn pause_continue(&mut self) {
        let command = if self.state().playing {
            Command::Pause
        } else {
            Command::Continue
        };
        self.send(command);
    }

    pub fn locate(&mut self, at: Ticks) {
        self.send(Command::Locate(at));
    }

    /// Sets the loop region (when a project is opened).
    pub fn set_loop(&mut self, region: Region) {
        self.loop_region = region;
        self.send(Command::SetLoop(region));
    }

    pub fn toggle_loop(&mut self) {
        self.loop_region.enabled = !self.loop_region.enabled;
        self.send(Command::SetLoop(self.loop_region));
    }

    /// Rebuilds the timeline from the project and sends it to the engine
    /// (after an edit, undo or redo that may have changed tempo or meter).
    pub fn timeline_changed(&mut self, project: &Project) {
        self.timeline = timeline_of(project);
        self.timeline_pending = true;
        self.settle();
    }

    /// Per-frame work: device events and pending timeline swaps. Returns
    /// true if something visible changed.
    pub fn update(&mut self) -> bool {
        if self.timeline_pending {
            self.timeline_pending = self
                .audio
                .engine
                .set_timeline(self.timeline.clone())
                .is_err();
        }
        self.audio.poll()
    }

    /// Global shortcuts. Call before drawing, so the keys are consumed
    /// before any widget sees them.
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (pause, play, home) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::SHIFT, Key::Space),
                i.consume_key(Modifiers::NONE, Key::Space),
                i.consume_key(Modifiers::NONE, Key::Home),
            )
        });
        if pause {
            self.pause_continue();
        } else if play {
            self.play_stop();
        }
        if home {
            self.locate(Ticks::ZERO);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, doc: &mut crate::document::Document) {
        let project = doc.project();
        let state = self.state();
        let running = matches!(self.audio.status, Status::Running { .. });
        let no_audio = "No audio device is running";

        if icons::button(
            ui,
            Icon::ToStart,
            "Go to start",
            Some("Home"),
            false,
            true,
            "",
        )
        .clicked()
        {
            self.locate(Ticks::ZERO);
        }
        if icons::button(
            ui,
            Icon::Stop,
            "Stop",
            Some("Space. Stop twice to return to the start"),
            false,
            running,
            no_audio,
        )
        .clicked()
        {
            self.send(Command::Stop);
        }
        // Play starts from the start marker, as Space does; while playing the
        // button stops in place. Shift+Space continues from the playhead.
        let (icon, label, keys) = if state.playing {
            (Icon::Pause, "Stop here", "Space")
        } else {
            (
                Icon::Play,
                "Play from the start marker",
                "Space; Shift+Space continues from the playhead",
            )
        };
        if icons::button(
            ui,
            icon,
            label,
            Some(keys),
            state.playing,
            running,
            no_audio,
        )
        .clicked()
        {
            let command = if state.playing {
                Command::Pause
            } else {
                Command::Play
            };
            self.send(command);
        }
        icons::button(
            ui,
            Icon::Record,
            "Record",
            Some("F9"),
            false,
            false,
            "Recording arrives in Milestone 9",
        );
        if icons::button(
            ui,
            Icon::Loop,
            "Loop",
            None,
            self.loop_region.enabled,
            true,
            "",
        )
        .clicked()
        {
            self.toggle_loop();
        }
        let metronome = icons::button(
            ui,
            Icon::Metronome,
            "Metronome",
            Some("right-click for volume and sound"),
            self.metronome.enabled,
            true,
            "",
        );
        if metronome.clicked() {
            self.metronome.enabled = !self.metronome.enabled;
            self.send(Command::SetMetronome(self.metronome));
        }
        metronome.context_menu(|ui| {
            let mut changed = false;
            ui.label("Metronome volume");
            let mut percent = (self.metronome.volume * 100.0).round();
            changed |= ui
                .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
                .changed();
            self.metronome.volume = percent / 100.0;
            ui.separator();
            ui.label("Sound");
            for sound in ClickSound::ALL {
                changed |= ui
                    .radio_value(&mut self.metronome.sound, sound, sound.name())
                    .changed();
            }
            if changed {
                self.send(Command::SetMetronome(self.metronome));
            }
        });
        ui.separator();

        // Tempo.
        let mut bpm = project.tempo.first().map_or(120.0, |p| p.bpm);
        let tempo = ui
            .add(
                egui::DragValue::new(&mut bpm)
                    .range(20.0..=999.0)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .suffix(" BPM"),
            )
            .on_hover_text("Tempo: drag, or click and type (20–999 BPM)");
        tempo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Tempo"));
        if tempo.changed() {
            // A drag is one undo step (merged until the pointer is released).
            doc.apply("Change tempo", Some("tempo"), |p| {
                if let Some(first) = p.tempo.first_mut() {
                    first.bpm = bpm;
                }
            });
        }
        let project = doc.project();

        // Time signature.
        let mut numerator = u32::from(project.time_signature.numerator);
        let mut denominator = u32::from(project.time_signature.denominator);
        let num = ui
            .add(
                egui::DragValue::new(&mut numerator)
                    .range(1..=99)
                    .speed(0.1),
            )
            .on_hover_text("Beats per bar");
        num.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Beats per bar")
        });
        ui.label("/");
        let den = egui::ComboBox::from_id_salt("denominator")
            .width(36.0)
            .selected_text(denominator.to_string())
            .show_ui(ui, |ui| {
                for d in DENOMINATORS {
                    ui.selectable_value(&mut denominator, d, d.to_string());
                }
            })
            .response
            .on_hover_text("Beat unit");
        den.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Beat unit")
        });
        if (numerator, denominator)
            != (
                project.time_signature.numerator.into(),
                project.time_signature.denominator.into(),
            )
        {
            doc.apply("Change time signature", Some("meter"), |p| {
                p.time_signature = TimeSignature {
                    numerator: numerator as u8,
                    denominator: denominator as u8,
                };
            });
        }
        ui.separator();

        self.position_field(ui, state);
        ui.separator();
        self.status(ui);
    }

    fn position_field(&mut self, ui: &mut Ui, state: TransportSnapshot) {
        let bbt = position::format_bbt(&self.timeline, state.ticks);
        let time = position::format_time(self.timeline.tempo.seconds_at(state.ticks));
        if let Some(text) = &mut self.editing {
            let edit = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(110.0)
                    .font(egui::TextStyle::Monospace),
            );
            edit.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Go to position")
            });
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.editing = None;
                self.error = None;
            } else if edit.lost_focus() {
                if enter {
                    match position::parse(text, &self.timeline) {
                        Ok(at) => {
                            self.locate(at);
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                self.editing = None;
            }
        } else {
            let field = ui
                .add(
                    egui::Button::new(RichText::new(format!("{bbt:>7}")).monospace().strong())
                        .frame(false),
                )
                .on_hover_text(
                    "Position (bars.beats.sixteenths). Click to type a position or a time like 1:05",
                );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Position {bbt}"))
            });
            if field.clicked() {
                self.editing = Some(bbt);
            }
        }
        ui.label(
            RichText::new(time)
                .monospace()
                .color(theme::p().text_secondary),
        );
        if let Some(error) = &self.error {
            ui.label(RichText::new("⚠").color(theme::p().warning))
                .on_hover_text(error.as_str());
        }
    }

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
            .on_hover_text("Turn audio processing on or off. Off releases the device")
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
            .on_hover_text(
                "Stops playback and silences the metronome, test tone and any sound at once",
            )
            .clicked()
        {
            self.send(Command::Panic);
            self.test_tone = false;
        }
    }

    fn status(&mut self, ui: &mut Ui) {
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
