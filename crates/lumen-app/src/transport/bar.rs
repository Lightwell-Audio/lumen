//! The transport bar's controls: buttons, tempo, time signature, Follow
//! and the position field.

use eframe::egui::{self, Key, RichText, Ui};
use eframe::emath::GuiRounding as _;

use crate::commands::{Command as Run, Keymap};
use crate::widgets::Tip;
use lumen_core::time::Ticks;
use lumen_engine::{ClickSound, Command, TransportSnapshot};

use crate::audio::Status;
use crate::icons::{self, Icon};
use crate::{position, theme, widgets};

use super::{Transport, fit};

/// The keyboard readout's type.
fn keyboard_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::CAPTION)
}

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
        widgets::Menu::context(&response).show(|ui| {
            widgets::menu_caption(ui, "Follow the playhead");
            let modes = [Follow::Page, Follow::Continuous, Follow::Off]
                .map(|mode| widgets::Choice::new(mode, mode.name()));
            if let Some(mode) = widgets::segmented(ui, ui.id().with("follow"), self.follow, &modes)
            {
                self.follow = mode;
                self.follow_paused = false;
                if mode != Follow::Off {
                    self.follow_last = mode;
                }
                ui.close();
            }
        });
    }

    /// The computer keyboard's switch, after Follow (M5 slice 2, the owner's
    /// K A): a keycap that lights while the mode is on, and beside it the
    /// octaves under A to K and the velocity, in a well. A click is for the
    /// app to take (`keyboard_clicked`).
    fn keyboard_button(&mut self, ui: &mut Ui, keymap: &Keymap) {
        let on = self.keyboard_readout.is_some();
        let keys = keymap.keys(Run::ComputerKeyboard);
        let response = icons::button(
            ui,
            Icon::Keycap,
            "Computer keyboard plays notes",
            Tip {
                keys: keys.as_ref(),
                hint: Some("A to K play notes, Z X octave, C V velocity, Esc leaves"),
            },
            on,
            true,
            "",
        );
        if response.clicked() {
            self.keyboard_clicked = true;
        }
        self.keyboard_readout_shown = false;
        if let (Some(readout), true) = (self.keyboard_readout.clone(), self.fit.plan.keyboard) {
            let before = ui.cursor().left();
            let c = theme::p();
            let galley = ui
                .painter()
                .layout_no_wrap(readout.clone(), keyboard_font(), c.text);
            let size = galley.size() + egui::vec2(2.0 * theme::space::S, 6.0);
            let (rect, well) = ui.allocate_exact_size(size, egui::Sense::hover());
            well.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("Computer keyboard: {readout}"),
                )
            });
            ui.painter().rect_filled(rect, theme::radius::SMALL, c.well);
            ui.painter()
                .galley(rect.center() - galley.size() / 2.0, galley, c.text);
            self.keyboard_readout_shown = true;
            self.fit.took(ui, before);
        }
    }

    /// The widths of the optional pieces, for `fit` (see its order).
    fn wants(&self, ui: &Ui) -> fit::Wants {
        let gap = fit::gap(ui);
        let keyboard = self.keyboard_readout.as_ref().map_or(0.0, |readout| {
            fit::text_width(ui, readout, keyboard_font()) + 2.0 * theme::space::S + gap
        });
        let (cpu, dropout, compensation) = self.status_wants(ui);
        fit::Wants {
            dropout,
            compensation,
            cpu,
            keyboard,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
        // What fits: the optional pieces give way in `fit`'s order, then
        // the bar tightens.
        let wants = self.wants(ui);
        let plan = self.fit.begin(ui, wants);
        if plan.compact {
            ui.spacing_mut().item_spacing.x = fit::COMPACT_GAP;
        }
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
        let play_button = icons::button(
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
        );
        if state.playing {
            icons::beam(ui, play_button.rect, icon);
        }
        if play_button.clicked() {
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
        widgets::Menu::context(&loop_button).show(|ui| punch_menu(ui, doc));
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
        self.keyboard_button(ui, keymap);
        solo_chip(ui, doc, keymap);
        widgets::Menu::context(&metronome)
            .keeping_open()
            .show(|ui| {
                let mut changed = false;
                ui.label("Metronome volume");
                // A value slider like every other level (it was egui's
                // own); double-click goes back to the default level.
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(160.0, theme::size::CONTROL_SMALL),
                    egui::Sense::hover(),
                );
                let default = lumen_engine::MetronomeSettings::default().volume;
                let volume = widgets::value_slider(
                    ui,
                    rect,
                    ui.id().with("metronome volume"),
                    self.metronome.volume,
                    widgets::SliderKind::Percent { default },
                    "Metronome volume",
                    theme::controls(false),
                );
                if volume != self.metronome.volume {
                    self.metronome.volume = volume;
                    widgets::note_change(ui);
                    changed = true;
                }
                ui.separator();
                widgets::menu_caption(ui, "Sound");
                let sounds = ClickSound::ALL.map(|s| widgets::Choice::new(s, s.name()));
                if let Some(sound) =
                    widgets::segmented(ui, ui.id().with("sound"), self.metronome.sound, &sounds)
                {
                    self.metronome.sound = sound;
                    changed = true;
                }
                // Its own output, never through the master (the owner's D6):
                // the main pair, or the cue pair for the headphones.
                ui.separator();
                widgets::menu_caption(ui, "Output");
                let cue = crate::settings::CUE_CHANNEL;
                let ready = self.audio.cue_ready();
                let outputs = [
                    widgets::Choice::new(0, "Main"),
                    widgets::Choice::new(cue, "Cue")
                        .enabled(ready)
                        .tip(if ready {
                            "The cue pair, for headphones"
                        } else {
                            "Choose a Cue output in Audio settings first"
                        }),
                ];
                if let Some(channel) =
                    widgets::segmented(ui, ui.id().with("output"), self.metronome.channel, &outputs)
                {
                    self.metronome.channel = channel;
                    changed = true;
                }
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
                    .suffix(if plan.compact { "" } else { " BPM" }),
            )
            .on_hover_text("Tempo, 20–999 BPM");
        tempo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Tempo"));
        if tempo.changed() {
            // A drag is one undo step (merged until the pointer is released).
            doc.set_tempo(bpm, Some("tempo"));
        }
        let project = doc.project();

        // Time signature: one field (the interface audit's S7).
        let current = (
            u32::from(project.time_signature.numerator),
            u32::from(project.time_signature.denominator),
        );
        if let Some((beats, unit)) = widgets::time_signature(
            ui,
            ui.id().with("time signature"),
            current,
            crate::document::MAX_BEATS_PER_BAR,
        ) {
            doc.set_time_signature(beats, unit, Some("meter"));
        }
        ui.separator();

        self.position_field(ui, state);
        // Compact, the dot follows the position without a divider.
        if !plan.compact {
            ui.separator();
        }
        self.status(ui, doc.project());
        self.fit.finish(ui);
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
    // Raised like every control; filled accent like the S buttons it clears.
    let lit = crate::motion::hover(ui.ctx(), response.id.with("lit"), soloed > 0);
    let face = crate::widgets::raised::face_of(ui, &response).lit(lit, crate::theme::Tone::Accent);
    let rect = rect
        .shrink2(egui::vec2(0.0, 2.0))
        .round_to_pixels(ui.pixels_per_point());
    let body = crate::widgets::raised::paint(ui.painter(), rect, theme::radius::SMALL, &face);
    let ink = if soloed > 0 {
        face.text()
    } else {
        palette.text_hint
    };
    crate::widgets::raised::centre_galley(ui.painter(), body, galley, ink);
    crate::widgets::raised::focus_ring(ui.painter(), &response, rect);
    if soloed > 0 && response.clicked() {
        doc.clear_solos();
    }
}
