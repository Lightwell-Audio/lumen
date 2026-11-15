use std::time::Instant;

use crate::App;
use crate::arrangement::CueControls;
use crate::routing::Cue;

impl App {
    pub(crate) fn cue(&self) -> Cue {
        let project = self.document.project();
        Cue {
            on: self.settings.mixing.cue_mode && self.transport.audio.cue_ready(),
            volume_db: self.settings.mixing.cue_volume_db,
            tracks: self
                .arrangement
                .cued
                .iter()
                .filter(|id| project.tracks.iter().any(|t| t.id == **id))
                .copied()
                .collect(),
            outputs: self.transport.audio.outputs(),
        }
    }

    pub(crate) fn lend_cue_controls(&mut self) {
        self.arrangement.cue = CueControls {
            mode: self.settings.mixing.cue_mode,
            ready: self.transport.audio.cue_ready(),
            volume_db: self.settings.mixing.cue_volume_db,
        };
    }

    pub(crate) fn take_cue_controls(&mut self) {
        let controls = self.arrangement.cue;
        let mixing = &mut self.settings.mixing;
        if (controls.mode, controls.volume_db) != (mixing.cue_mode, mixing.cue_volume_db) {
            mixing.cue_mode = controls.mode;
            mixing.cue_volume_db = controls.volume_db;
            self.settings_changed = Some(Instant::now());
        }
    }
}
