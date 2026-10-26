//! The transport bar's controls: buttons, tempo, time signature, Follow
//! and the position field.

use eframe::egui::{self, Key, RichText, Ui};
use eframe::emath::GuiRounding as _;

use crate::commands::{Command as Run, Keymap};
use crate::widgets::Tip;
use lumen_core::time::Ticks;
use lumen_engine::{ClickSound, Command, TransportSnapshot};

use crate::audio::Status;
use crate::icons::{self, Icon, Look};
use crate::record_settings::{CountIn, RecordSettings};
use crate::{motion, position, theme, widgets};

use super::record::{self, Phase};
use super::{Transport, fit};

/// Overdub's tooltip: what the switch decides about a take.
const OVERDUB_HINT: &str = "Replace: a take replaces what is under it. Overdub: a take adds to it.";

/// The keyboard readout's type.
fn keyboard_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::CAPTION)
}

/// The type of a readout's values: Martian Mono 13, so numbers never
/// jitter (the Transport and Field cards).
fn value_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::BODY)
}

/// The type of a readout's units (" BPM"), in `text_hint`.
fn unit_font() -> egui::FontId {
    egui::FontId::monospace(theme::size::MONO_SMALL)
}

/// The room beside a readout's values inside its well (`.lm-display`:
/// `space_m`; the compact bar tightens it to `space_xs`, so Record and
/// Overdub fit beside the position at 960).
fn well_pad(compact: bool) -> f32 {
    if compact {
        theme::space::XS
    } else {
        theme::space::M
    }
}

/// Allocates a 30 pt readout well `width` wide (the design system's
/// `.lm-display`, ADR-0052) and paints it: sunk, with the lip and a
/// border that strengthens under the pointer. Returns its rect.
fn readout_well(ui: &mut Ui, id: egui::Id, width: f32) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, theme::size::CONTROL_LARGE),
        egui::Sense::hover(),
    );
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let over = ui.rect_contains_pointer(rect);
    let hover = crate::motion::hover(ui.ctx(), id.with("well hover"), over);
    widgets::raised::paint_field(ui.painter(), rect, theme::radius::LARGE, hover);
    rect
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
            let size = egui::vec2(
                galley.size().x + 2.0 * theme::space::S,
                theme::size::CONTROL_SMALL,
            );
            let (rect, well) = ui.allocate_exact_size(size, egui::Sense::hover());
            well.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("Computer keyboard: {readout}"),
                )
            });
            widgets::raised::paint_sunken(ui.painter(), rect, theme::radius::SMALL);
            ui.painter()
                .galley(rect.center() - galley.size() / 2.0, galley, c.text);
            self.keyboard_readout_shown = true;
            self.fit.took(ui, before);
        }
    }

    /// The Record button (M5 slice 6d, the owner's 1-A): the record glyph in
    /// red at rest; a hollow ring that pulses on each count-in beat; filled
    /// and lit with the Flare while recording. A click, or F9, asks the app
    /// for Record (which counts in, cancels a count-in, or ends a take).
    fn record_button(
        &mut self,
        ui: &mut Ui,
        keymap: &Keymap,
        state: TransportSnapshot,
        record: RecordSettings,
        beats: u32,
    ) {
        let keys = keymap.keys(Run::Record);
        let count_in = format!("Count-in: {}", record.count_in.name());
        let red = theme::p().record;
        let base = Look {
            enabled: true,
            tone: Some(theme::Tone::Record),
            ..Look::default()
        };
        let (icon, look) = match record::phase(&state) {
            Phase::Rest => (
                Icon::Record,
                Look {
                    ink: Some(red.gamma_multiply(record::REST_INK)),
                    ..base
                },
            ),
            Phase::CountingIn => (
                Icon::RecordRing,
                Look {
                    active: true,
                    ink: Some(red),
                    lit: Some(record::count_in_lit(
                        state.count_in_left,
                        state.count_in_total,
                        beats,
                        motion::reduced(),
                    )),
                    ..base
                },
            ),
            Phase::Recording => (
                Icon::Record,
                Look {
                    active: true,
                    ..base
                },
            ),
        };
        let response = icons::button_look(
            ui,
            icon,
            "Record",
            Tip {
                keys: keys.as_ref(),
                hint: Some(count_in.as_str()),
            },
            look,
        );
        if response.clicked() {
            self.requests.push(Run::Record);
        }
    }

    /// The Overdub switch (M5 slice 6d, the owner's 2-B): on, a take adds to
    /// the clip under it; off, Replace overwrites it. A click asks the app
    /// for Overdub.
    fn overdub_button(&mut self, ui: &mut Ui, keymap: &Keymap, record: RecordSettings) {
        let keys = keymap.keys(Run::Overdub);
        let overdub = icons::button(
            ui,
            Icon::Layers,
            "Overdub",
            Tip {
                keys: keys.as_ref(),
                hint: Some(OVERDUB_HINT),
            },
            record.overdub,
            true,
            "",
        );
        if overdub.clicked() {
            self.requests.push(Run::Overdub);
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

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        doc: &mut crate::document::Document,
        keymap: &Keymap,
        record: RecordSettings,
    ) {
        // What fits: the optional pieces give way in `fit`'s order, then
        // the bar tightens.
        let wants = self.wants(ui);
        let plan = self.fit.begin(ui, wants);
        if plan.compact {
            ui.spacing_mut().item_spacing.x = fit::COMPACT_GAP;
        }
        let state = self.state();
        // The count-in's beats, in the meter the song plays at (the engine
        // counts the same beats).
        let beats = record
            .count_in
            .beats(self.timeline.meter.meter_at(state.marker).numerator);
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
            icons::beam(ui, play_button.rect);
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
        self.record_button(ui, keymap, state, record, beats);
        self.overdub_button(ui, keymap, record);
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
                hint: Some("Right-click for volume, sound, count-in and output"),
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
                // The count-in and the record-only switch are the palette's
                // settings: the choices ask the app for their commands.
                ui.separator();
                widgets::menu_caption(ui, "Count-in");
                let counts = CountIn::ALL
                    .map(|c| widgets::Choice::new(c, c.name()).named(record::count_in_name(c)));
                if let Some(count_in) =
                    widgets::segmented(ui, ui.id().with("count in"), record.count_in, &counts)
                {
                    self.requests.push(record::count_in_command(count_in));
                }
                let mut only = record.only_while_recording;
                if widgets::switch(ui, &mut only, "Only while recording", true).clicked() {
                    self.requests.push(Run::RecordOnlyMetronome);
                }
                let mut keep = record.keep_played;
                if widgets::switch(ui, &mut keep, "Keep played notes", true)
                    .on_hover_text(
                        "Keeps the last ten minutes of notes you play while not recording, in memory only, for Capture MIDI",
                    )
                    .clicked()
                {
                    self.requests.push(Run::KeepPlayedNotes);
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
        fit::divider(ui);

        // Tempo, in a 30 pt well: the value in Martian Mono, its unit in
        // `text_hint` (the Transport card).
        let project = doc.project();
        // The tempo and the time signature in force at the playhead: with
        // changes along the song, the fields show and edit the segment the
        // playhead is in (M7 slice A6, the owner's choice).
        let mut bpm = lumen_project::edit::maps::tempo_in(&project.tempo, state.ticks);
        let pad = well_pad(plan.compact);
        let unit = (!plan.compact).then_some("BPM");
        let value_w = fit::text_width(ui, "999.00", value_font());
        let unit_w = unit.map_or(0.0, |unit| {
            theme::space::S + fit::text_width(ui, unit, unit_font())
        });
        let well_id = ui.id().with("tempo well");
        let well = readout_well(ui, well_id, 2.0 * pad + value_w + unit_w);
        // The drag value fills the value's part of the well, with no fill
        // or outline of its own: the well is its ground.
        let value_rect = egui::Rect::from_min_max(
            egui::pos2(well.left() + pad, well.top()),
            egui::pos2(well.left() + pad + value_w, well.bottom()),
        );
        let mut field = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(value_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        field.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        field.spacing_mut().button_padding.x = 0.0;
        field.set_clip_rect(value_rect.intersect(ui.clip_rect()));
        field.style_mut().visuals.text_edit_bg_color = Some(theme::p().well);
        for state in {
            let w = &mut field.style_mut().visuals.widgets;
            [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open]
        } {
            state.bg_fill = egui::Color32::TRANSPARENT;
            state.weak_bg_fill = egui::Color32::TRANSPARENT;
            state.bg_stroke = egui::Stroke::NONE;
            state.corner_radius = egui::CornerRadius::same(theme::radius::LARGE);
        }
        let tempo = field
            .add_sized(
                value_rect.size(),
                egui::DragValue::new(&mut bpm)
                    .range(20.0..=999.0)
                    // Typed values apply on Enter or leaving the field: live,
                    // "1" on the way to "125" played at 20 BPM and sent the
                    // playhead past the audio (owner, after slice 4a).
                    .update_while_editing(false)
                    .speed(0.1)
                    .fixed_decimals(2),
            )
            .on_hover_text("Tempo, 20–999 BPM");
        tempo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Tempo"));
        if let Some(unit) = unit {
            let galley =
                ui.painter()
                    .layout_no_wrap(unit.to_owned(), unit_font(), theme::p().text_hint);
            let at = egui::pos2(
                value_rect.right() + theme::space::S,
                well.center().y - galley.size().y / 2.0,
            );
            ui.painter()
                .galley(at.round(), galley, theme::p().text_hint);
        }
        if tempo.changed() {
            // A drag is one undo step (merged until the pointer is released).
            doc.set_tempo_at(state.ticks, bpm, Some("tempo"));
        }

        // Time signature: one field (the interface audit's S7).
        let here = self.timeline.meter.meter_at(state.ticks);
        let current = (here.numerator, here.denominator);
        if let Some((beats, unit)) = widgets::time_signature(
            ui,
            ui.id().with("time signature"),
            current,
            crate::document::MAX_BEATS_PER_BAR,
        ) {
            doc.set_time_signature_at(state.ticks, beats, unit, Some("meter"));
        }
        fit::divider(ui);

        self.position_field(ui, state, plan.compact);
        // Compact, the dot follows the position without a divider.
        if !plan.compact {
            fit::divider(ui);
        }
        self.status(ui, doc.project());
        self.fit.finish(ui);
    }

    /// The position, in a 30 pt well: bars and beats, then the time, both
    /// Martian Mono. Clicking the bars types a place to go to. While counting
    /// in, it counts up through the bars before 1 in the secondary colour
    /// (M5 slice 6d).
    fn position_field(&mut self, ui: &mut Ui, state: TransportSnapshot, compact: bool) {
        let counting = state.count_in_total > 0;
        let shown = if counting {
            record::count_in_ticks(
                &self.timeline,
                state.marker,
                state.count_in_left,
                state.sample_rate,
            )
        } else {
            state.ticks
        };
        let bbt = if counting {
            record::count_in_text(&self.timeline, shown)
        } else {
            position::format_bbt(&self.timeline, state.ticks)
        };
        let time = record::time_text(
            self.time_format,
            self.timeline.tempo.seconds_at(shown),
            state.sample_rate,
        );
        if let Some(text) = &mut self.editing {
            let edit = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(110.0)
                    .min_size(egui::vec2(0.0, theme::size::CONTROL_LARGE))
                    .vertical_align(egui::Align::Center)
                    .font(value_font()),
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
            let c = theme::p();
            let pad = well_pad(compact);
            let bars_colour = if counting { c.text_secondary } else { c.text };
            let bars = ui
                .painter()
                .layout_no_wrap(format!("{bbt:>7}"), value_font(), bars_colour);
            let clock = ui
                .painter()
                .layout_no_wrap(time.clone(), value_font(), c.text_secondary);
            let (bars_w, clock_w) = (bars.size().x, clock.size().x);
            let well_id = ui.id().with("position well");
            let gap = if compact {
                theme::space::XS
            } else {
                theme::space::S
            };
            let well = readout_well(ui, well_id, 2.0 * pad + bars_w + gap + clock_w);
            let bars_rect = egui::Rect::from_min_size(
                egui::pos2(well.left() + pad, well.top()),
                egui::vec2(bars_w, well.height()),
            );
            let clock_rect = egui::Rect::from_min_size(
                egui::pos2(bars_rect.right() + gap, well.top()),
                egui::vec2(clock_w, well.height()),
            );
            let field = ui
                .interact(bars_rect, well_id.with("bars"), egui::Sense::click())
                .on_hover_text("Position: click to type a bar or a time");
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Position {bbt}"))
            });
            let time_node = ui.interact(clock_rect, well_id.with("time"), egui::Sense::hover());
            time_node.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, time.as_str())
            });
            let centre = |rect: egui::Rect, galley: &std::sync::Arc<egui::Galley>| {
                (rect.center() - galley.size() / 2.0).round()
            };
            let (at_bars, at_clock) = (centre(bars_rect, &bars), centre(clock_rect, &clock));
            ui.painter().galley(at_bars, bars, bars_colour);
            ui.painter().galley(at_clock, clock, c.text_secondary);
            if field.has_focus() {
                widgets::raised::paint_focus(ui.painter(), well, theme::radius::LARGE);
            }
            if field.clicked() {
                self.editing = Some(bbt);
            }
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new("⚠").color(theme::p().warning))
                .on_hover_text(error.as_str());
        }
    }
}

/// Loop and punch switches: the loop button's and the loop brace's
/// right-click menus. Punch in and out act at the loop's edges.
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
        RichText::new("A take keeps only what is inside the punch range.")
            .weak()
            .small(),
    );
}

/// The solo chip (the owner's D3, M4 slice 4): an "S" that lights while
/// anything is soloed, with the count in its hint; a click clears every
/// solo. Quiet and inactive while nothing is.
fn solo_chip(ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
    let palette = *theme::p();
    let soloed = doc.project().tracks.iter().filter(|t| t.solo).count();
    let text = "S";
    let font = crate::fonts::medium(theme::size::CAPTION);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, palette.text);
    let height = theme::size::CONTROL_SMALL;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(height, height), egui::Sense::click());
    let keys = keymap.keys(Run::ClearSolos);
    let response = if soloed > 0 {
        crate::widgets::tip(
            response,
            &format!("Clear all solos ({soloed})"),
            keys.as_ref(),
            None,
        )
    } else {
        response.on_hover_text("Nothing is soloed")
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, soloed > 0, "Clear all solos")
    });
    // Raised like every control; filled accent like the S buttons it clears
    // (the Transport card, ADR-0052).
    let lit = crate::motion::hover(ui.ctx(), response.id.with("lit"), soloed > 0);
    let face = crate::widgets::raised::face_of(ui, &response).lit(lit, crate::theme::Tone::Accent);
    // A 20 pt circle; the count remains available in the tooltip.
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let body = crate::widgets::raised::paint(ui.painter(), rect, theme::radius::FULL, &face);
    let ink = if soloed > 0 {
        face.text()
    } else {
        palette.text_hint
    };
    crate::widgets::raised::centre_galley(ui.painter(), body, galley, ink);
    if response.has_focus() {
        crate::widgets::raised::paint_focus(ui.painter(), rect, theme::radius::FULL);
    }
    if soloed > 0 && response.clicked() {
        doc.clear_solos();
    }
}
