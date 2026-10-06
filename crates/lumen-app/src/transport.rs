//! The transport bar (spec §5, owner request for vector icons): go to start,
//! stop, play/pause, record, loop and metronome; tempo; time signature; an
//! editable position; audio status. Drives `lumen-engine`.
//!
//! Shortcuts (Ableton-compatible): Space play/stop · Shift+Space pause or
//! continue · Home go to start. They are ignored while typing in a field.

use eframe::egui::{self, Key, Modifiers, RichText, Ui};
use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{Command, Region, Timeline, TransportSnapshot};
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
        }
    }

    pub fn state(&self) -> TransportSnapshot {
        self.audio.engine.transport()
    }

    fn send(&mut self, command: Command) {
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

    pub fn pause_continue(&mut self) {
        let command = if self.state().playing {
            Command::Pause
        } else {
            Command::Play
        };
        self.send(command);
    }

    pub fn locate(&mut self, at: Ticks) {
        self.send(Command::Locate(at));
    }

    pub fn toggle_loop(&mut self) {
        self.loop_region.enabled = !self.loop_region.enabled;
        self.send(Command::SetLoop(self.loop_region));
    }

    /// Rebuilds the timeline from the project and sends it to the engine.
    fn timeline_changed(&mut self, project: &Project) {
        self.timeline = timeline_of(project);
        self.timeline_pending = true;
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

    pub fn ui(&mut self, ui: &mut Ui, project: &mut Project) {
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
            Some("Space"),
            false,
            running,
            no_audio,
        )
        .clicked()
        {
            self.send(Command::Stop);
        }
        let (icon, label) = if state.playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        if icons::button(
            ui,
            icon,
            label,
            Some("Space"),
            state.playing,
            running,
            no_audio,
        )
        .clicked()
        {
            self.pause_continue();
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
        icons::button(
            ui,
            Icon::Metronome,
            "Metronome",
            None,
            false,
            false,
            "The metronome arrives in the next slices of Milestone 2",
        );
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
        if tempo.changed()
            && let Some(first) = project.tempo.first_mut()
        {
            first.bpm = bpm;
            self.timeline_changed(project);
        }

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
            project.time_signature = TimeSignature {
                numerator: numerator as u8,
                denominator: denominator as u8,
            };
            self.timeline_changed(project);
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
        ui.label(RichText::new(time).monospace().color(theme::RULER_TEXT));
        if let Some(error) = &self.error {
            ui.label(RichText::new("⚠").color(theme::WARNING))
                .on_hover_text(error.as_str());
        }
    }

    fn status(&self, ui: &mut Ui) {
        let (text, color, tip) = match &self.audio.status {
            Status::Starting => (
                "Starting audio…".to_owned(),
                theme::HINT_TEXT,
                String::new(),
            ),
            Status::Running { device, detail } => (
                format!("{device} · {detail}"),
                theme::RULER_TEXT,
                "Audio device, sample rate, buffer and round-trip latency. Choose the device \
                 with --audio until the device selector arrives"
                    .to_owned(),
            ),
            Status::Lost(why) => (
                "Audio device lost: reconnecting…".to_owned(),
                theme::WARNING,
                why.clone(),
            ),
            Status::Off(why) => ("Audio off".to_owned(), theme::WARNING, why.clone()),
        };
        // A dot that always fits shows the state at a glance; the text
        // shortens in narrow windows, and the tooltip always has all of it.
        let dot = match self.audio.status {
            Status::Running { .. } => theme::ACCENT,
            Status::Starting => theme::HINT_TEXT,
            Status::Lost(_) | Status::Off(_) => theme::WARNING,
        };
        let tip = if tip.is_empty() {
            text.clone()
        } else {
            format!("{text}\n\n{tip}")
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, dot);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Audio: {text}"))
        });
        response.on_hover_text(tip.as_str());
        ui.add(egui::Label::new(RichText::new(text).small().color(color)).truncate())
            .on_hover_text(tip);
    }
}
