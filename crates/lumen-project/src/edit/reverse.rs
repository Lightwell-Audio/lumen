use std::sync::Arc;

use lumen_core::time::{TempoMap, Ticks};

use crate::model::{Clip, ClipContent, Id, Media, Project};

use super::normalize::{self, Window};
use super::tempo_of;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flipped {
    pub changed: usize,
    pub refused: usize,
}

const REACH: f64 = 1.5;

fn audio_clips(project: &Project, ids: &[Id]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for (t, track) in project.tracks.iter().enumerate() {
        for (c, clip) in track.clips.iter().enumerate() {
            if ids.contains(&clip.id)
                && !clip.locked
                && matches!(clip.content, ClipContent::Audio { .. })
            {
                found.push((t, c));
            }
        }
    }
    found
}

pub fn reverse(project: &mut Project, ids: &[Id]) -> Flipped {
    let tempo = tempo_of(project);
    let mut out = Flipped::default();
    for (t, c) in audio_clips(project, ids) {
        let clip = &project.tracks[t].clips[c];
        let ClipContent::Audio { media, .. } = &clip.content else {
            continue;
        };
        let Some(file) = project.media.iter().find(|m| m.id == *media) else {
            out.refused += 1;
            continue;
        };
        let Some(mirrored) = mirrored(clip, file, &tempo) else {
            out.refused += 1;
            continue;
        };
        let track = Arc::make_mut(&mut project.tracks[t]);
        track.clips[c] = Arc::new(mirrored);
        out.changed += 1;
    }
    out
}

pub fn invert(project: &mut Project, ids: &[Id]) -> usize {
    let found = audio_clips(project, ids);
    for &(t, c) in &found {
        let track = Arc::make_mut(&mut project.tracks[t]);
        let clip = Arc::make_mut(&mut track.clips[c]);
        clip.inverted = !clip.inverted;
    }
    found.len()
}

fn mirrored(clip: &Clip, file: &Media, tempo: &TempoMap) -> Option<Clip> {
    let rate = f64::from(file.sample_rate.max(1));
    let total = file.frames as f64 / rate;
    let Window { from, span } = normalize::window(clip, rate, tempo)?;
    let to = total - from - span;
    if to * rate < -REACH {
        return None;
    }
    let to = to.max(0.0);
    let (new_offset, phase) = super::warp::split_seconds(to, rate);
    let mut out = clip.clone();
    out.takes = None;
    out.reversed = !clip.reversed;
    if let Some(warp) = &mut out.warp {
        let sum = clip.loop_length.unwrap_or(clip.length);
        lumen_core::warp::mirror(&mut warp.markers, sum, total);
    }
    out.audio_phase_seconds = phase;
    if let ClipContent::Audio { offset, .. } = &mut out.content {
        *offset = new_offset;
    }
    if let Some(period) = clip.loop_length {
        out.loop_phase = Ticks((-(clip.loop_phase.0 + clip.length.0)).rem_euclid(period.0));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::split;
    use crate::edit::tests::{ids, q, song};

    fn only(project: &Project) -> Clip {
        (*project.tracks[0].clips[0]).clone()
    }

    fn media_frames(project: &Project) -> u64 {
        project.media[0].frames
    }

    #[test]
    fn reversing_twice_returns_the_clip_exactly() {
        let mut project = song();
        let original = only(&project);
        let done = reverse(&mut project, &[original.id]);
        assert_eq!((done.changed, done.refused), (1, 0));
        assert!(only(&project).reversed);
        reverse(&mut project, &[original.id]);
        assert_eq!(only(&project), original);
    }

    #[test]
    fn reversing_mirrors_the_window_about_the_end_of_the_file() {
        let mut project = song();
        let rate = f64::from(project.media[0].sample_rate);
        let frames = media_frames(&project) as f64;
        let id = only(&project).id;
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.length = q(4);
        if let ClipContent::Audio { offset, .. } = &mut c.content {
            *offset = rate as i64;
        }
        reverse(&mut project, &[id]);
        let ClipContent::Audio { offset, .. } = only(&project).content else {
            panic!("audio stays audio")
        };
        let want = frames - rate - 2.0 * rate;
        assert!(
            (offset as f64 + only(&project).audio_phase_seconds * rate - want).abs() < 1e-3,
            "the window [1 s, 3 s) is [{want}, ...) in the reversed file"
        );
    }

    #[test]
    fn a_clip_cut_in_two_then_reversed_plays_the_halves_in_the_other_order() {
        let mut project = song();
        let first = only(&project);
        let parts = split::clips(&mut project, &[first.id], &[q(8)], &mut ids());
        reverse(&mut project, &parts);
        let (a, b) = (
            (*project.tracks[0].clips[0]).clone(),
            (*project.tracks[0].clips[1]).clone(),
        );
        let (ClipContent::Audio { offset: oa, .. }, ClipContent::Audio { offset: ob, .. }) =
            (&a.content, &b.content)
        else {
            panic!("audio stays audio")
        };
        let rate = f64::from(project.media[0].sample_rate);
        let at = |c: &Clip, o: i64| o as f64 / rate + c.audio_phase_seconds;
        assert!(
            at(&a, *oa) > at(&b, *ob),
            "the earlier part now plays the later stretch of the file"
        );
    }

    #[test]
    fn a_looped_clip_mirrors_its_phase_and_comes_back() {
        let mut project = song();
        let t = Arc::make_mut(&mut project.tracks[0]);
        let c = Arc::make_mut(&mut t.clips[0]);
        c.loop_length = Some(q(3));
        c.loop_phase = q(2);
        c.length = q(7);
        let original = only(&project);
        reverse(&mut project, &[original.id]);
        assert_eq!(only(&project).loop_phase, Ticks::ZERO);
        reverse(&mut project, &[original.id]);
        assert_eq!(only(&project), original);
    }

    #[test]
    fn locked_midi_and_unknown_clips_are_left_alone() {
        let mut project = song();
        let id = only(&project).id;
        let t = Arc::make_mut(&mut project.tracks[0]);
        Arc::make_mut(&mut t.clips[0]).locked = true;
        let before = project.clone();
        assert_eq!(reverse(&mut project, &[id]), Flipped::default());
        assert_eq!(invert(&mut project, &[id]), 0);
        assert_eq!(project, before);

        let mut project = song();
        let id = only(&project).id;
        project.media.clear();
        let done = reverse(&mut project, &[id]);
        assert_eq!((done.changed, done.refused), (0, 1), "no media to mirror");
    }

    #[test]
    fn a_clip_longer_than_the_file_cannot_be_mirrored() {
        let mut project = song();
        let id = only(&project).id;
        let t = Arc::make_mut(&mut project.tracks[0]);
        Arc::make_mut(&mut t.clips[0]).length = q(10_000);
        let before = project.clone();
        let done = reverse(&mut project, &[id]);
        assert_eq!((done.changed, done.refused), (0, 1));
        assert_eq!(project, before);
    }

    #[test]
    fn polarity_toggles_and_toggles_back() {
        let mut project = song();
        let original = only(&project);
        assert_eq!(invert(&mut project, &[original.id]), 1);
        assert!(only(&project).inverted);
        invert(&mut project, &[original.id]);
        assert_eq!(only(&project), original);
    }
}
