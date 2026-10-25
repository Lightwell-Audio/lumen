//! The keyboard side of an audio track's input (M9 slice R1): the commands
//! that step the selected audio tracks through the inputs and through the
//! monitor modes, for those who never open the chip's menu.

use lumen_project::model::{Id, Monitor, TrackKind};

use crate::App;
use crate::audio_input::next_input;

impl App {
    /// The selected audio tracks.
    pub(crate) fn selected_audio_tracks(&self) -> Vec<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && self.arrangement.selected.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }

    /// Steps the selected audio tracks to the next input (the first track's
    /// decides which; the others follow it).
    pub(crate) fn next_input_for_selected(&mut self) {
        let ids = self.selected_audio_tracks();
        let Some(first) = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| ids.contains(&t.id))
        else {
            return;
        };
        let next = next_input(first.input, &self.transport.audio.open_inputs());
        self.document.set_audio_input(&ids, next);
    }

    /// Steps the selected audio tracks' monitor mode: Auto, In, Off.
    pub(crate) fn next_monitor_for_selected(&mut self) {
        let ids = self.selected_audio_tracks();
        let Some(first) = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| ids.contains(&t.id))
        else {
            return;
        };
        let next = match first.monitor {
            Monitor::Auto => Monitor::In,
            Monitor::In => Monitor::Off,
            Monitor::Off => Monitor::Auto,
        };
        self.document.set_monitor(&ids, next);
    }
}
