//! The bar and what fits in it: the three wells, the dividers between them, and
//! the optional pieces in the order they give way.

use super::controls::solo_piece;
use super::*;

impl Transport {
    /// The widths of the optional pieces, for `fit` (see its order).
    fn wants(&self, ui: &Ui) -> fit::Wants {
        let keyboard = self.keyboard_readout.as_ref().map_or(0.0, |readout| {
            fit::text_width(ui, readout, keyboard_font())
                + 2.0 * theme::space::S
                + widgets::well::PIECE_GAP
        });
        let (cpu, dropout, compensation) = self.status_wants(ui);
        fit::Wants {
            dropout,
            compensation,
            tap: tap_width(ui) + widgets::well::PIECE_GAP,
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

        // Three groups (the interface audit's TB1): the transport in one
        // well, the modes in another, then tempo, meter and position in one
        // numeric well.
        widgets::well::row(ui, |ui| {
            if icons::button(
                ui,
                Icon::ToStart,
                "Go to start",
                Tip {
                    command: Some(Run::GoToStart),
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
                    command: Some(Run::PlayStop),
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
            // Play starts from the start marker, as Space does; while playing
            // the button stops in place. Shift+Space continues from the
            // playhead.
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
                    command: Some(Run::PlayStop),
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
        });
        fit::divider(ui);
        let metronome = widgets::well::row(ui, |ui| {
            self.overdub_button(ui, keymap, record);
            let looped = doc.project().transport;
            let keys = keymap.keys(Run::ToggleLoop);
            let loop_button = icons::button(
                ui,
                Icon::Loop,
                "Loop",
                Tip {
                    command: Some(Run::ToggleLoop),
                    keys: keys.as_ref(),
                    hint: Some("Right-click for punch in and out"),
                },
                looped.loop_enabled,
                true,
                "",
            );
            if loop_button.clicked() {
                doc.set_loop_enabled(!looped.loop_enabled);
            }
            widgets::Menu::context(&loop_button).show(|ui| punch_menu(ui, doc, keymap));
            let keys = keymap.keys(Run::ToggleMetronome);
            let metronome = icons::button(
                ui,
                Icon::Metronome,
                "Metronome",
                Tip {
                    command: Some(Run::ToggleMetronome),
                    keys: keys.as_ref(),
                    hint: Some("Right-click for volume, sound, count-in and output"),
                },
                self.metronome.enabled,
                true,
                "",
            );
            if metronome.clicked() {
                self.toggle_metronome();
            }
            self.follow_button(ui, keymap);
            self.keyboard_button(ui, keymap);
            solo_piece(ui, doc, keymap);
            metronome
        })
        .inner;
        widgets::Menu::context(&metronome)
            .keeping_open()
            .show(|ui| self.metronome_menu(ui, record));
        fit::divider(ui);
        self.numbers(ui, doc, keymap, state, plan);
        // Compact, the dot follows the position without a divider.
        if !plan.compact {
            fit::divider(ui);
        }
        self.status(ui, doc.project());
        self.fit.finish(ui);
    }
}
