//! Join eligibility shared by the arrangement preview and engine translation.
//! This runs off the audio thread; source tails are bounded again by the
//! engine against the loaded audio, which may have been resampled.

use lumen_core::fade::Fade;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Id, Media, Project};
use std::collections::HashMap;

/// A temporary clip for handles and hit testing, including suppression of
/// ordinary edge fades replaced by an active crossing. Never persisted.
pub(crate) fn preview(project: &Project, track: Id, clip: &Clip) -> Clip {
    let mut shown = clip.clone();
    shown.crossfade = false;
    if let Some(fade) = fade(project, track, clip) {
        shown.crossfade = true;
        shown.fade_in = fade;
    } else if clip.crossfade {
        shown.fade_in = Fade::default();
    }
    if let Some(track) = project.tracks.iter().find(|t| t.id == track)
        && track.clips.iter().any(|next| {
            next.start == clip.start + clip.length && fade(project, track.id, next).is_some()
        })
    {
        shown.fade_out = Fade::default();
    }
    shown
}

/// The audio clip that starts where `left` ends on the same track: the one a
/// crossfade dragged from `left`'s fade-out handle belongs to. Both clips
/// must be unlocked audio.
pub(crate) fn incoming(project: &Project, left: Id) -> Option<Id> {
    let track = project
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == left))?;
    let from = track.clips.iter().find(|c| c.id == left)?;
    if from.locked || !matches!(from.content, ClipContent::Audio { .. }) {
        return None;
    }
    let end = from.start + from.length;
    track
        .clips
        .iter()
        .find(|c| {
            c.id != left
                && c.start == end
                && !c.locked
                && c.length > Ticks::ZERO
                && matches!(c.content, ClipContent::Audio { .. })
        })
        .map(|c| c.id)
}

/// Effective incoming fade at a touching audio join. A missing tail, a gap,
/// or contiguous source material leaves the ordinary fade behavior intact.
pub(crate) fn fade(project: &Project, track: Id, clip: &Clip) -> Option<Fade> {
    if !clip.crossfade && !project.auto_crossfade {
        return None;
    }
    let ClipContent::Audio { .. } = clip.content else {
        return None;
    };
    let track = project.tracks.iter().find(|t| t.id == track)?;
    let left = track
        .clips
        .iter()
        .find(|c| c.id != clip.id && c.start + c.length == clip.start && c.length > Ticks::ZERO)?;
    let tempo = TempoMap::new(project.tempo.clone()).ok()?;
    let ClipContent::Audio { media: a, .. } = left.content else {
        return None;
    };
    let ClipContent::Audio { media: b, .. } = clip.content else {
        return None;
    };
    let sources = project
        .media
        .iter()
        .filter(|m| m.id == a || m.id == b)
        .map(|m| (m.id, m))
        .collect();
    resolve(&sources, &tempo, left, clip).map(|join| join.fade)
}

/// The crossfades that change what is heard, by incoming clip. A crossfade
/// asked for across a seamless join (a split whose halves still continue each
/// other) is left out: it is shown, but the audio plays on unchanged.
pub(crate) fn resolved(project: &Project) -> HashMap<Id, Fade> {
    let mut out = HashMap::new();
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return out;
    };
    let sources = project.media.iter().map(|m| (m.id, m)).collect();
    for track in &project.tracks {
        let ends: HashMap<_, _> = track
            .clips
            .iter()
            .map(|c| (c.start + c.length, c.as_ref()))
            .collect();
        for clip in &track.clips {
            if (clip.crossfade || project.auto_crossfade)
                && let Some(left) = ends.get(&clip.start)
                && let Some(join) = resolve(&sources, &tempo, left, clip)
                && !join.seamless
            {
                out.insert(clip.id, join.fade);
            }
        }
    }
    out
}

/// A join that crossfades. `seamless`: the two clips continue the same audio,
/// so blending them changes nothing heard and none is played.
struct Join {
    fade: Fade,
    seamless: bool,
}

fn resolve(
    sources: &HashMap<Id, &Media>,
    tempo: &TempoMap,
    left: &Clip,
    clip: &Clip,
) -> Option<Join> {
    // An explicit handle dragged to zero overrides the automatic default.
    if clip.crossfade && clip.fade_in.length <= Ticks::ZERO {
        return None;
    }
    let ClipContent::Audio { media, offset, .. } = clip.content else {
        return None;
    };
    let ClipContent::Audio {
        media: left_media,
        offset: left_offset,
        ..
    } = left.content
    else {
        return None;
    };
    let source = sources.get(&left_media)?;
    let incoming = sources.get(&media)?;
    let duration = tempo.seconds_at(clip.start) - tempo.seconds_at(left.start);
    let left_origin =
        left_offset as f64 / f64::from(source.sample_rate.max(1)) + left.audio_phase_seconds;
    let right_origin =
        offset as f64 / f64::from(incoming.sample_rate.max(1)) + clip.audio_phase_seconds;
    let contiguous = if let (Some(a), Some(b)) = (left.loop_length, clip.loop_length) {
        a == b
            && a > Ticks::ZERO
            && left_offset == offset
            && (left.loop_phase + left.length).0.rem_euclid(a.0)
                == clip.loop_phase.0.rem_euclid(a.0)
    } else {
        left.loop_length.is_none()
            && clip.loop_length.is_none()
            && (left_origin + duration - right_origin).abs() < 1e-7
    };
    if left_media == media && left.reversed == clip.reversed && contiguous {
        // Only a crossfade asked for by hand is shown across a seamless join,
        // so every split does not show one.
        return (clip.crossfade && clip.fade_in.length > Ticks::ZERO).then(|| Join {
            fade: Fade {
                length: clip.fade_in.length.min(clip.length),
                curve: clip.fade_in.curve,
                ..Fade::default()
            },
            seamless: true,
        });
    }
    let tail_start = if let Some(period) = left.loop_length.filter(|p| *p > Ticks::ZERO) {
        let phase = Ticks((left.loop_phase + left.length).0.rem_euclid(period.0));
        let origin = left.start - left.loop_phase;
        left_origin + tempo.seconds_at(origin + phase) - tempo.seconds_at(origin)
    } else {
        left_origin + duration
    };
    let available = source.frames as f64 / f64::from(source.sample_rate.max(1)) - tail_start;
    if available <= 0.0 {
        return None;
    }
    let requested = if clip.fade_in.length > Ticks::ZERO {
        clip.fade_in.length
    } else {
        tempo.ticks_at_seconds(tempo.seconds_at(clip.start) + 0.005) - clip.start
    };
    let tail = tempo.ticks_at_seconds(tempo.seconds_at(clip.start) + available) - clip.start;
    let length = requested.min(tail).min(clip.length);
    (length > Ticks::ZERO).then_some(Join {
        fade: Fade {
            length,
            curve: clip.fade_in.curve,
            ..Fade::default()
        },
        seamless: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn join() -> Project {
        let mut project = lumen_project::testing::small_project(1);
        project.auto_crossfade = true;
        project.tempo = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }];
        let mut track = project
            .tracks
            .iter()
            .find(|t| {
                t.clips
                    .iter()
                    .any(|c| matches!(c.content, ClipContent::Audio { .. }))
            })
            .unwrap()
            .as_ref()
            .clone();
        let mut left = track
            .clips
            .iter()
            .find(|c| matches!(c.content, ClipContent::Audio { .. }))
            .unwrap()
            .as_ref()
            .clone();
        let ClipContent::Audio { media, .. } = left.content else {
            unreachable!()
        };
        let source = project.media.iter_mut().find(|m| m.id == media).unwrap();
        source.sample_rate = 48_000;
        source.frames = 96_000;
        left.start = Ticks::ZERO;
        left.length = Ticks::quarters(2);
        left.content = ClipContent::Audio {
            media,
            offset: 0,
            gain_db: 0.0,
        };
        left.audio_phase_seconds = 0.0;
        left.loop_length = None;
        left.loop_phase = Ticks::ZERO;
        left.reversed = false;
        left.crossfade = false;
        left.fade_in = Fade::default();
        left.fade_out = Fade::default();
        let mut right = left.clone();
        right.id = Id(100_000);
        right.start = Ticks::quarters(2);
        // Restart the source at the join: deliberately not a split seam.
        track.clips = vec![Arc::new(left), Arc::new(right)];
        project.tracks = vec![Arc::new(track)];
        project
    }

    #[test]
    fn automatic_join_and_translation_share_the_five_ms_window() {
        let p = join();
        let right = &p.tracks[0].clips[1];
        let f = fade(&p, p.tracks[0].id, right).unwrap();
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        assert_eq!(
            tempo.sample_at(right.start + f.length, 48_000) - tempo.sample_at(right.start, 48_000),
            240
        );
        assert_eq!(resolved(&p).get(&right.id), Some(&f));
        let audio = Arc::new(lumen_engine::ClipAudio {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000]],
        });
        let clips = crate::lanes::engine_clips(&p, 48_000, |_| Some((audio.clone(), 48_000)));
        assert!(clips[1].shape.crossfade);
        assert_eq!(clips[1].shape.fade_in.frames, 240);
    }

    #[test]
    fn gaps_contiguous_splits_and_missing_tails_do_not_crossfade() {
        let mut p = join();
        let track_id = p.tracks[0].id;
        let check = |p: &Project| assert!(fade(p, track_id, &p.tracks[0].clips[1]).is_none());
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]).start = Ticks::quarters(3);
        check(&p);
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.start = Ticks::quarters(2);
        if let ClipContent::Audio { offset, .. } = &mut right.content {
            *offset = 48_000;
        }
        check(&p);
        if let ClipContent::Audio { offset, .. } =
            &mut Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]).content
        {
            *offset = 0;
        }
        let ClipContent::Audio { media, .. } = p.tracks[0].clips[0].content else {
            unreachable!()
        };
        p.media.iter_mut().find(|m| m.id == media).unwrap().frames = 48_000;
        check(&p);
    }

    #[test]
    fn explicit_length_is_limited_by_available_tail_and_setting_is_respected() {
        let mut p = join();
        p.auto_crossfade = false;
        let id = p.tracks[0].id;
        assert!(fade(&p, id, &p.tracks[0].clips[1]).is_none());
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.crossfade = true;
        right.fade_in.length = Ticks::quarters(4);
        assert_eq!(
            fade(&p, id, &p.tracks[0].clips[1]).unwrap().length,
            Ticks::quarters(2)
        );
        let ClipContent::Audio { media, .. } = p.tracks[0].clips[0].content else {
            unreachable!()
        };
        p.media.iter_mut().find(|m| m.id == media).unwrap().frames = 48_048;
        let f = fade(&p, id, &p.tracks[0].clips[1]).unwrap();
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        assert_eq!(
            tempo.sample_at(Ticks::quarters(2) + f.length, 48_000) - 48_000,
            48
        );
    }

    #[test]
    fn zero_and_broken_custom_fades_do_not_become_ordinary_fade_ins() {
        let mut p = join();
        let id = p.tracks[0].id;
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.crossfade = true;
        right.fade_in.length = Ticks::ZERO;
        assert!(fade(&p, id, &p.tracks[0].clips[1]).is_none());
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.fade_in.length = Ticks::quarters(1);
        right.start = Ticks::quarters(3);
        let audio = Arc::new(lumen_engine::ClipAudio {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000]],
        });
        let clips = crate::lanes::engine_clips(&p, 48_000, |_| Some((audio.clone(), 48_000)));
        assert!(!clips[1].shape.crossfade);
        assert_eq!(clips[1].shape.fade_in.frames, 0);
    }

    #[test]
    fn a_crossfade_asked_for_across_a_split_is_shown_but_changes_nothing_heard() {
        let mut p = join();
        let id = p.tracks[0].id;
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        // The right half continues the left's audio: a plain split.
        if let ClipContent::Audio { offset, .. } = &mut right.content {
            *offset = 48_000;
        }
        assert!(
            fade(&p, id, &p.tracks[0].clips[1]).is_none(),
            "not asked for"
        );
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.crossfade = true;
        right.fade_in.length = Ticks::quarters(1);
        let shown = fade(&p, id, &p.tracks[0].clips[1]).expect("shown");
        assert_eq!(shown.length, Ticks::quarters(1));
        assert!(resolved(&p).is_empty(), "no blend is played");
        let audio = Arc::new(lumen_engine::ClipAudio {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 96_000]],
        });
        let clips = crate::lanes::engine_clips(&p, 48_000, |_| Some((audio.clone(), 48_000)));
        assert!(!clips[1].shape.crossfade);
        assert_eq!(clips[1].shape.fade_in.frames, 0, "plays on seamlessly");
    }

    #[test]
    fn preview_hides_the_outgoing_fade_only_while_the_crossing_is_active() {
        let mut p = join();
        let left = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        left.fade_out.length = Ticks::quarters(1);
        let track = &p.tracks[0];
        assert_eq!(
            preview(&p, track.id, &track.clips[0]).fade_out.length,
            Ticks::ZERO
        );
        p.auto_crossfade = false;
        let track = &p.tracks[0];
        assert_eq!(
            preview(&p, track.id, &track.clips[0]).fade_out.length,
            Ticks::quarters(1)
        );
    }
}
