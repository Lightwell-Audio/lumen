use lumen_project::automation::Target;
use lumen_project::envelope::Kind;
use lumen_project::model::{ClipContent, Id};

use crate::App;

impl App {
    pub(crate) fn selected_audio_clip(&self) -> Option<(Id, Id)> {
        let mut selected = self.arrangement.clips.iter();
        let (first, None) = (selected.next()?, selected.next()) else {
            return None;
        };
        self.document.project().tracks.iter().find_map(|track| {
            track
                .clips
                .iter()
                .find(|c| c.id == *first)
                .filter(|c| matches!(c.content, ClipContent::Audio { .. }))
                .map(|c| (track.id, c.id))
        })
    }

    pub(crate) fn envelope_shown(&self, track: Id, clip: Id) -> bool {
        self.arrangement
            .auto
            .shown
            .get(&track)
            .and_then(|t| t.clip())
            .is_some_and(|(shown, _)| shown == clip)
    }

    pub(crate) fn clip_envelope_toggle(&mut self) {
        let Some((track, clip)) = self.selected_audio_clip() else {
            return;
        };
        if self.envelope_shown(track, clip) {
            self.arrangement.auto.shown.remove(&track);
            self.arrangement.auto.picks = None;
            self.arrangement.auto.focus = false;
        } else {
            self.arrangement
                .auto
                .show(track, Target::envelope(clip, Kind::Gain));
        }
    }
}
