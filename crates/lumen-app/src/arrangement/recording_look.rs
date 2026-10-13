//! What the arrangement shows of a recording under way (M5 slice 6d): the
//! clips being recorded are drawn in record red (the owner's 3-A), lent by
//! the app each frame.

use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id};

/// The tracks recording, and which of their clips the take is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RecordingLook {
    /// The armed MIDI tracks recording.
    pub tracks: Vec<Id>,
    /// Where the take begins.
    pub from: Ticks,
    /// Overdub: the clips under the take stay, and take the red outline.
    pub overdub: bool,
    /// Replace: the clip each track's take is (once it exists).
    pub take_clips: Vec<Id>,
}

impl RecordingLook {
    /// Whether `clip`, on `track`, is drawn as being recorded.
    pub(crate) fn is_recording(&self, track: Id, clip: &Clip) -> bool {
        if !self.tracks.contains(&track) {
            return false;
        }
        if self.overdub {
            clip.start + clip.length > self.from
        } else {
            self.take_clips.contains(&clip.id)
        }
    }
}
