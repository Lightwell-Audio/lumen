//! The metronome's menu, and the loop and punch switches.

use super::*;

impl Transport {
    /// The metronome button's menu: volume, sound, count-in, the
    /// record-only and keep-played switches, and the output.
    pub(super) fn metronome_menu(&mut self, ui: &mut Ui, record: RecordSettings) {
        let mut changed = false;
        ui.label("Metronome volume");
        // A value slider like every other level (it was egui's own);
        // double-click goes back to the default level.
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
        let counts =
            CountIn::ALL.map(|c| widgets::Choice::new(c, c.name()).named(record::count_in_name(c)));
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
        // Its own output, never through the master (the owner's D6): the
        // main pair, or the cue pair for the headphones.
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
    }
}

/// Loop and punch switches: the loop button's and the loop brace's
/// right-click menus. Punch in and out act at the loop's edges.
pub fn punch_menu(ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
    let t = doc.project().transport;
    let keys = keymap.keys(Run::ToggleLoop);
    if widgets::menu_toggle(ui, "Loop", keys.as_ref(), t.loop_enabled).clicked() {
        doc.set_loop_enabled(!t.loop_enabled);
    }
    let keys = keymap.keys(Run::PunchIn);
    if widgets::menu_toggle(ui, Run::PunchIn.name(), keys.as_ref(), t.punch_in).clicked() {
        doc.set_punch(!t.punch_in, t.punch_out);
    }
    let keys = keymap.keys(Run::PunchOut);
    if widgets::menu_toggle(ui, Run::PunchOut.name(), keys.as_ref(), t.punch_out).clicked() {
        doc.set_punch(t.punch_in, !t.punch_out);
    }
    ui.label(
        RichText::new("A take keeps only what is inside the punch range.")
            .weak()
            .small(),
    );
}
