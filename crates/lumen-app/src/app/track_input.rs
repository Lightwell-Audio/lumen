use lumen_project::model::{Id, Monitor, TrackKind};

use crate::App;
use crate::recording::audio_input::next_input;

impl App {
    pub(crate) fn selected_audio_tracks(&self) -> Vec<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && self.arrangement.selected.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }

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
