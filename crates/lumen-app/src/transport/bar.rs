//! The transport bar's controls: buttons, tempo, time signature, Follow
//! and the position field.

use eframe::egui::{self, Key, RichText, Ui};

use crate::commands::{Command as Run, Keymap};
use crate::widgets::Tip;
use lumen_core::time::Ticks;
use lumen_engine::{ClickSound, Command, TransportSnapshot};

use crate::audio::Status;
use crate::icons::{self, Icon};
use crate::{position, theme};

use super::{DENOMINATORS, Transport};

impl Transport {
    fn follow_button(&mut self, ui: &mut egui::Ui, keymap: &Keymap) {
        use crate::settings::Follow;
        let on = self.follow != Follow::Off;
        let keys = keymap.keys(Run::ToggleFollow);
        let response = icons::button(
            ui,
            Icon::Follow,
            "Follow playhead",
            Tip {
                keys: keys.as_ref(),
                hint: Some("Right-click for page or continuous"),
            },
            on && !self.follow_paused,
            true,
            "",
        );
        let response = if on && self.follow_paused {
            response.on_hover_text("Paused while you look around; Play or a click resumes it")
        } else {
            response
        };
        if response.clicked() {
            self.toggle_follow();
        }
        response.context_menu(|ui| {
            ui.label("Follow the playhead");
            for mode in [Follow::Page, Follow::Continuous, Follow::Off] {
                if ui
                    .radio_value(&mut self.follow, mode, mode.name())
                    .clicked()
                {
                    self.follow_paused = false;
                    if mode != Follow::Off {
                        self.follow_last = mode;
                    }
                    ui.close();
                }
            }
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
        let state = self.state();
        let running = matches!(self.audio.status, Status::Running { .. });
        let no_audio = "No audio device is running";
        let (start, play) = (keymap.keys(Run::GoToStart), keymap.keys(Run::PlayStop));

        if icons::button(
            ui,
            Icon::ToStart,
            "Go to start",
            Tip {
                keys: start.as_ref(),
                hint: None,
            },
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
            Tip {
                keys: play.as_ref(),
                hint: Some("Stop twice to return to the start"),
            },
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
        let continues = keymap
            .text(Run::PauseContinue)
            .map(|k| format!("{k} continues from the playhead"));
        let (icon, label, hint) = if state.playing {
            (Icon::Pause, "Stop here", None)
        } else {
            (
                Icon::Play,
                "Play from the start marker",
                continues.as_deref(),
            )
        };
        if icons::button(
            ui,
            icon,
            label,
            Tip {
                keys: play.as_ref(),
                hint,
            },
            state.playing,
            running,
            no_audio,
        )
        .clicked()
        {
            let command = if state.playing {
                Command::Pause
            } else {
                self.played_from_stop = true;
                Command::Play
            };
            self.send(command);
        }
        icons::button(
            ui,
            Icon::Record,
            "Record",
            Tip::default(),
            false,
            false,
            "Recording arrives in Milestone 9",
        );
        let looped = doc.project().transport;
        let loop_button = icons::button(
            ui,
            Icon::Loop,
            "Loop",
            Tip {
                keys: None,
                hint: Some("Right-click for punch in and out"),
            },
            looped.loop_enabled,
            true,
            "",
        );
        if loop_button.clicked() {
            doc.set_loop_enabled(!looped.loop_enabled);
        }
        loop_button.context_menu(|ui| punch_menu(ui, doc));
        let metronome = icons::button(
            ui,
            Icon::Metronome,
            "Metronome",
            Tip {
                keys: None,
                hint: Some("Right-click for volume, sound and output"),
            },
            self.metronome.enabled,
            true,
            "",
        );
        if metronome.clicked() {
            self.metronome.enabled = !self.metronome.enabled;
            self.send(Command::SetMetronome(self.metronome));
        }
        self.follow_button(ui, keymap);
        solo_chip(ui, doc, keymap);
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
            // Its own output, never through the master (the owner's D6):
            // the main pair, or the cue pair for the headphones.
            ui.separator();
            ui.label("Output");
            let cue = crate::settings::CUE_CHANNEL;
            changed |= ui
                .radio_value(&mut self.metronome.channel, 0, "Main")
                .changed();
            let ready = self.audio.cue_ready();
            changed |= ui
                .add_enabled(
                    ready,
                    egui::RadioButton::new(self.metronome.channel == cue, "Cue"),
                )
                .on_disabled_hover_text("Choose a Cue output in Audio settings first")
                .clicked()
                && {
                    self.metronome.channel = cue;
                    true
                };
            if changed {
                self.send(Command::SetMetronome(self.metronome));
            }
        });
        ui.separator();

        // Tempo.
        let project = doc.project();
        let mut bpm = project.tempo.first().map_or(120.0, |p| p.bpm);
        let tempo = ui
            .add(
                egui::DragValue::new(&mut bpm)
                    .range(20.0..=999.0)
                    // Typed values apply on Enter or leaving the field: live,
                    // "1" on the way to "125" played at 20 BPM and sent the
                    // playhead past the audio (owner, after slice 4a).
                    .update_while_editing(false)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .suffix(" BPM"),
            )
            .on_hover_text("Tempo, 20–999 BPM");
        tempo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Tempo"));
        if tempo.changed() {
            // A drag is one undo step (merged until the pointer is released).
            doc.set_tempo(bpm, Some("tempo"));
        }
        let project = doc.project();

        // Time signature.
        let mut numerator = u32::from(project.time_signature.numerator);
        let mut denominator = u32::from(project.time_signature.denominator);
        let num = ui
            .add(
                egui::DragValue::new(&mut numerator)
                    .update_while_editing(false)
                    .range(1..=crate::document::MAX_BEATS_PER_BAR)
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
            doc.set_time_signature(numerator, denominator, Some("meter"));
        }
        ui.separator();

        self.position_field(ui, state);
        ui.separator();
        self.status(ui, doc.project());
    }

    fn position_field(&mut self, ui: &mut Ui, state: TransportSnapshot) {
        let bbt = position::format_bbt(&self.timeline, state.ticks);
        let time = crate::timefmt::format(
            self.time_format,
            self.timeline.tempo.seconds_at(state.ticks),
            state.sample_rate,
        );
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
                .on_hover_text("Position: click to type a bar or a time");
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
}

/// Loop and punch switches: the loop button's and the loop brace's
/// right-click menus. Punch in and out act at the loop's edges, as in
/// Ableton (recording arrives in Milestone 9).
pub fn punch_menu(ui: &mut Ui, doc: &mut crate::document::Document) {
    let t = doc.project().transport;
    if ui
        .add(egui::Button::selectable(t.loop_enabled, "Loop"))
        .clicked()
    {
        doc.set_loop_enabled(!t.loop_enabled);
        ui.close();
    }
    if ui
        .add(egui::Button::selectable(
            t.punch_in,
            "Punch in at the loop start",
        ))
        .clicked()
    {
        doc.set_punch(!t.punch_in, t.punch_out);
        ui.close();
    }
    if ui
        .add(egui::Button::selectable(
            t.punch_out,
            "Punch out at the loop end",
        ))
        .clicked()
    {
        doc.set_punch(t.punch_in, !t.punch_out);
        ui.close();
    }
    ui.label(
        RichText::new("Recording, which punch controls, arrives in Milestone 9")
            .weak()
            .small(),
    );
}

/// The solo chip (the owner's D3, M4 slice 4): an "S" that lights while
/// anything is soloed, with the count when more than one track is; a click
/// clears every solo. Quiet and inactive while nothing is.
fn solo_chip(ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
    let palette = *theme::p();
    let soloed = doc.project().tracks.iter().filter(|t| t.solo).count();
    let text = match soloed {
        0 | 1 => "S".to_owned(),
        n => format!("S {n}"),
    };
    let font = crate::fonts::medium(theme::size::CAPTION);
    let galley = ui.painter().layout_no_wrap(text, font, palette.text);
    let height = ui.spacing().interact_size.y;
    let width = (galley.size().x + 2.0 * theme::space::S).max(height);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let keys = keymap.keys(Run::ClearSolos);
    let response = if soloed > 0 {
        crate::widgets::tip(response, "Clear all solos", keys.as_ref(), None)
    } else {
        response.on_hover_text("Nothing is soloed")
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, soloed > 0, "Clear all solos")
    });
    let hover = crate::motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let (fill, ink) = if soloed > 0 {
        let fill = palette.accent;
        (fill, theme::text_on(fill))
    } else {
        (palette.control.gamma_multiply(0.6), palette.text_hint)
    };
    let fill = if soloed > 0 {
        fill.lerp_to_gamma(palette.text, 0.12 * hover)
    } else {
        fill
    };
    let rect = rect.shrink2(egui::vec2(0.0, 2.0));
    ui.painter().rect_filled(rect, theme::radius::SMALL, fill);
    ui.painter()
        .galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, ink);
    if soloed > 0 && response.clicked() {
        doc.clear_solos();
    }
}
