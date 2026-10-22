//! Showing a clip's envelope (M7 slice A7, the owner's E1 A): the gain and pan
//! of an audio clip are drawn and edited on the clip itself, with the lane
//! look and gestures of automation (`arrangement/auto_view.rs`). This is the
//! command that shows or hides it for the selected clip.

use lumen_project::automation::Target;
use lumen_project::envelope::Kind;
use lumen_project::model::{ClipContent, Id};

use crate::App;

impl App {
    /// The track and the one audio clip selected, if exactly one clip is
    /// selected and it holds audio.
    pub(crate) fn selected_audio_clip(&self) -> Option<(Id, Id)> {
        let mut selected = self.arrangement.clips.iter();
        let (first, None) = (selected.next()?, selected.next()) else {
            return None;
        };
        self.document
            .project()
            .tracks
            .iter()
            .find_map(|track| {
                track
                    .clips
                    .iter()
                    .find(|c| c.id == *first)
                    .filter(|c| matches!(c.content, ClipContent::Audio { .. }))
                    .map(|c| (track.id, c.id))
            })
    }

    /// Whether the envelope of `clip` is the lane its track shows.
    pub(crate) fn envelope_shown(&self, track: Id, clip: Id) -> bool {
        self.arrangement
            .auto
            .shown
            .get(&track)
            .and_then(|t| t.clip())
            .is_some_and(|(shown, _)| shown == clip)
    }

    /// Shows the selected clip's gain envelope over it, or hides it when it
    /// shows. The kind last shown for the clip is kept.
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
