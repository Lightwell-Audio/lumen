//! The one rule a crossfade keeps (M7b E2): it stands only while an audio
//! clip ends where the incoming clip starts. Moving, trimming, deleting or
//! pasting either clip breaks it, and the marker goes with it, so a clip that
//! was moved away is an ordinary clip again, fade-in handle and all. Run by
//! the history after every edit.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Project};

/// Clears the crossfade marker of every clip with no audio clip ending at
/// its start.
pub fn tidy(project: &mut Project) {
    for track in &mut project.tracks {
        if !track.clips.iter().any(|c| c.crossfade) {
            continue;
        }
        let mut ends: Vec<Ticks> = track
            .clips
            .iter()
            .filter(|c| matches!(c.content, ClipContent::Audio { .. }))
            .map(|c| c.start + c.length)
            .collect();
        ends.sort_unstable();
        let stale = |c: &Clip| c.crossfade && ends.binary_search(&c.start).is_err();
        if !track.clips.iter().any(|c| stale(c)) {
            continue;
        }
        for clip in &mut Arc::make_mut(track).clips {
            if stale(clip) {
                let clip = Arc::make_mut(clip);
                clip.crossfade = false;
                clip.fade_in.length = Ticks::ZERO;
            }
        }
    }
}
