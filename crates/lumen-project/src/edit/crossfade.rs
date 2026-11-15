use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Project};

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
