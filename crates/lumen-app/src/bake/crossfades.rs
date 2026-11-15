use std::collections::HashMap;

use lumen_core::fade::Fade;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Id, Media, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Zone {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Crossing {
    pub left: Id,
    pub fade: Fade,
    pub zone: Zone,
    pub seamless: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Played {
    pub clip: Clip,
    pub blend: [bool; 2],
}

fn audio(clip: &Clip) -> Option<Id> {
    match clip.content {
        ClipContent::Audio { media, .. } => Some(media),
        ClipContent::Midi { .. } => None,
    }
}

fn before<'a>(clips: &'a [std::sync::Arc<Clip>], clip: &Clip) -> Option<&'a Clip> {
    clips
        .iter()
        .map(|c| &**c)
        .find(|c| c.id != clip.id && c.start + c.length == clip.start && c.length > Ticks::ZERO)
}

pub(crate) fn crossing(project: &Project, track: Id, clip: &Clip) -> Option<Crossing> {
    if !clip.crossfade && !project.auto_crossfade {
        return None;
    }
    audio(clip)?;
    let track = project.tracks.iter().find(|t| t.id == track)?;
    let left = before(&track.clips, clip)?;
    let tempo = TempoMap::new(project.tempo.clone()).ok()?;
    let sources = sources(project, left, clip);
    resolve(project.auto_crossfade, &sources, &tempo, left, clip)
}

fn sources<'a>(project: &'a Project, a: &Clip, b: &Clip) -> HashMap<Id, &'a Media> {
    project
        .media
        .iter()
        .filter(|m| Some(m.id) == audio(a) || Some(m.id) == audio(b))
        .map(|m| (m.id, m))
        .collect()
}

pub(crate) fn outgoing(project: &Project, track: Id, left: &Clip) -> Option<(Id, Crossing)> {
    let track = project.tracks.iter().find(|t| t.id == track)?;
    let end = left.start + left.length;
    let next = track
        .clips
        .iter()
        .find(|c| c.id != left.id && c.start == end && c.crossfade)?;
    let crossing = crossing(project, track.id, next)?;
    (crossing.left == left.id && crossing.zone == Zone::Before).then_some((next.id, crossing))
}

pub(crate) fn room(project: &Project, track: Id, clip: &Clip) -> Ticks {
    let Some(track) = project.tracks.iter().find(|t| t.id == track) else {
        return Ticks::ZERO;
    };
    let (Some(left), Ok(tempo)) = (
        before(&track.clips, clip),
        TempoMap::new(project.tempo.clone()),
    ) else {
        return Ticks::ZERO;
    };
    let Some(source) = audio(clip).and_then(|m| project.media.iter().find(|f| f.id == m)) else {
        return Ticks::ZERO;
    };
    left.length.min(hidden(&tempo, source, clip))
}

fn hidden(tempo: &TempoMap, source: &Media, clip: &Clip) -> Ticks {
    let ClipContent::Audio { offset, .. } = clip.content else {
        return Ticks::ZERO;
    };
    if let Some(warp) = &clip.warp {
        return Ticks::ZERO - warp.first_tick().min(Ticks::ZERO);
    }
    let origin = offset as f64 / f64::from(source.sample_rate.max(1)) + clip.audio_phase_seconds;
    clip.start - tempo.ticks_at_seconds((tempo.seconds_at(clip.start) - origin).max(0.0))
}

pub(crate) fn preview(project: &Project, track: Id, clip: &Clip) -> Clip {
    let mut shown = clip.clone();
    shown.crossfade = false;
    if clip.crossfade {
        shown.fade_in = Fade::default();
    }
    if let Some((_, crossing)) = outgoing(project, track, clip) {
        shown.crossfade = true;
        shown.fade_out = crossing.fade;
    }
    shown
}

pub(crate) fn incoming(project: &Project, left: Id) -> Option<Id> {
    let track = project
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == left))?;
    let from = track.clips.iter().find(|c| c.id == left)?;
    if from.locked || audio(from).is_none() {
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
                && c.loop_length.is_none()
                && audio(c).is_some()
        })
        .map(|c| c.id)
}

pub(crate) fn played(project: &Project) -> HashMap<Id, Played> {
    let mut out: HashMap<Id, Played> = HashMap::new();
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return out;
    };
    let sources = project.media.iter().map(|m| (m.id, m)).collect();
    for track in &project.tracks {
        for clip in &track.clips {
            if !clip.crossfade && !project.auto_crossfade {
                continue;
            }
            let Some(left) = before(&track.clips, clip) else {
                continue;
            };
            let Some(crossing) = resolve(project.auto_crossfade, &sources, &tempo, left, clip)
            else {
                continue;
            };
            if crossing.seamless {
                continue;
            }
            let d = crossing.fade.length;
            let fade = Fade {
                length: d,
                curve: crossing.fade.curve,
                ..Fade::default()
            };
            let outgoing = out.entry(left.id).or_insert_with(|| Played {
                clip: left.clone(),
                blend: [false; 2],
            });
            outgoing.clip.fade_out = fade;
            outgoing.blend[1] = true;
            if crossing.zone == Zone::After {
                outgoing.clip.length = outgoing.clip.length + d;
            }
            let incoming = out.entry(clip.id).or_insert_with(|| Played {
                clip: (**clip).clone(),
                blend: [false; 2],
            });
            incoming.clip.fade_in = fade;
            incoming.blend[0] = true;
            if crossing.zone == Zone::Before {
                let c = &mut incoming.clip;
                let begins = tempo.seconds_at(c.start);
                c.start = c.start - d;
                c.length = c.length + d;
                if let Some(warp) = c.warp.as_mut() {
                    warp.shift(d);
                    lumen_project::edit::warp::anchor(project, c);
                } else {
                    c.audio_phase_seconds -= begins - tempo.seconds_at(c.start);
                }
                c.envelopes.shift(d);
            }
        }
    }
    out
}

fn resolve(
    auto: bool,
    sources: &HashMap<Id, &Media>,
    tempo: &TempoMap,
    left: &Clip,
    clip: &Clip,
) -> Option<Crossing> {
    let explicit = clip.crossfade;
    if explicit && clip.fade_in.length <= Ticks::ZERO {
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
    let same_flow = match (&left.warp, &clip.warp) {
        (None, None) => (left_origin + duration - right_origin).abs() < 1e-7,
        (Some(a), Some(b)) => a.continues(b, clip.start - left.start),
        _ => false,
    };
    let contiguous = left.loop_length.is_none()
        && clip.loop_length.is_none()
        && left_media == media
        && left.reversed == clip.reversed
        && same_flow;
    if explicit {
        if clip.loop_length.is_some() {
            return None;
        }
        let length = clip
            .fade_in
            .length
            .min(left.length)
            .min(hidden(tempo, incoming, clip));
        return (length > Ticks::ZERO).then_some(Crossing {
            left: left.id,
            fade: Fade {
                length,
                curve: clip.fade_in.curve,
                ..Fade::default()
            },
            zone: Zone::Before,
            seamless: contiguous,
        });
    }
    if !auto
        || contiguous
        || clip.fade_in.length > Ticks::ZERO
        || left.fade_out.length > Ticks::ZERO
        || left.loop_length.is_some()
        || clip.loop_length.is_some()
    {
        return None;
    }
    let file_seconds = source.frames as f64 / f64::from(source.sample_rate.max(1));
    let begins = tempo.seconds_at(clip.start);
    let wanted = tempo.ticks_at_seconds(begins + 0.005) - clip.start;
    let tail = match &left.warp {
        Some(warp) => {
            let tail = warp.last_tick(file_seconds) - left.length;
            if tail <= Ticks::ZERO {
                return None;
            }
            tail
        }
        None => {
            let available = file_seconds - (left_origin + duration);
            if available <= 0.0 {
                return None;
            }
            tempo.ticks_at_seconds(begins + available) - clip.start
        }
    };
    let length = wanted.min(tail).min(clip.length);
    (length > Ticks::ZERO).then_some(Crossing {
        left: left.id,
        fade: Fade {
            length,
            ..Fade::default()
        },
        zone: Zone::After,
        seamless: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn pair() -> Project {
        let mut project = lumen_project::testing::small_project(1);
        project.auto_crossfade = false;
        project.tempo = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }];
        let mut track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| audio(c).is_some()))
            .unwrap()
            .as_ref()
            .clone();
        let mut left = track
            .clips
            .iter()
            .find(|c| audio(c).is_some())
            .unwrap()
            .as_ref()
            .clone();
        let media = audio(&left).unwrap();
        let source = project.media.iter_mut().find(|m| m.id == media).unwrap();
        source.sample_rate = 48_000;
        source.frames = 48_000 * 8;
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
        right.content = ClipContent::Audio {
            media,
            offset: 12_000,
            gain_db: 0.0,
        };
        track.clips = vec![Arc::new(left), Arc::new(right)];
        project.tracks = vec![Arc::new(track)];
        project
    }

    fn right_mut(p: &mut Project) -> &mut Clip {
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1])
    }

    fn by_hand(p: &mut Project, length: Ticks) {
        let right = right_mut(p);
        right.crossfade = true;
        right.fade_in.length = length;
    }

    #[test]
    fn a_crossfade_by_hand_runs_over_the_end_of_the_left_clip() {
        let mut p = pair();
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let id = p.tracks[0].id;
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.zone, Zone::Before);
        assert_eq!(found.fade.length, Ticks(Ticks::quarters(1).0 / 4));
        assert!(!found.seamless);

        let played = played(&p);
        let (out, inc) = (&played[&p.tracks[0].clips[0].id], &played[&right.id]);
        assert_eq!(out.clip.start + out.clip.length, right.start);
        assert_eq!(out.clip.fade_out.length, found.fade.length);
        assert_eq!(inc.clip.start, right.start - found.fade.length);
        assert_eq!(inc.clip.length, right.length + found.fade.length);
        assert_eq!(inc.clip.fade_in.length, found.fade.length);
        assert_eq!((out.blend, inc.blend), ([false, true], [true, false]));
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        let shift = tempo.seconds_at(right.start) - tempo.seconds_at(inc.clip.start);
        assert!((inc.clip.audio_phase_seconds - (right.audio_phase_seconds - shift)).abs() < 1e-9);
    }

    #[test]
    fn it_is_limited_by_the_hidden_start_and_the_left_clip() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks::quarters(4));
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.fade.length, Ticks(Ticks::quarters(1).0 / 2));
        let right = right_mut(&mut p);
        right.content = ClipContent::Audio {
            media: audio(right).unwrap(),
            offset: 0,
            gain_db: 0.0,
        };
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        assert!(played(&p).is_empty());
        assert_eq!(preview(&p, id, &right).fade_in.length, Ticks::ZERO);
        let mut p = pair();
        by_hand(&mut p, Ticks::ZERO);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
    }

    #[test]
    fn a_gap_or_a_loop_makes_no_crossfade() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        right_mut(&mut p).start = Ticks::quarters(3);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        right_mut(&mut p).start = Ticks::quarters(2);
        right_mut(&mut p).loop_length = Some(Ticks::quarters(1));
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
    }

    #[test]
    fn a_crossfade_across_a_split_is_shown_but_changes_nothing_heard() {
        let mut p = pair();
        let id = p.tracks[0].id;
        let right = right_mut(&mut p);
        right.content = ClipContent::Audio {
            media: audio(right).unwrap(),
            offset: 48_000,
            gain_db: 0.0,
        };
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).expect("shown");
        assert!(found.seamless);
        assert!(played(&p).is_empty(), "no blend is played");
        let left = p.tracks[0].clips[0].clone();
        assert!(preview(&p, id, &left).crossfade, "but the handle is there");
    }

    #[test]
    fn the_handles_show_the_crossfade_on_the_left_clip() {
        let mut p = pair();
        let id = p.tracks[0].id;
        by_hand(&mut p, Ticks(Ticks::quarters(1).0 / 4));
        let right = (*p.tracks[0].clips[1]).clone();
        let mut left = (*p.tracks[0].clips[0]).clone();
        left.fade_out.length = Ticks::quarters(1);
        let shown = preview(&p, id, &left);
        assert!(shown.crossfade);
        assert_eq!(
            shown.fade_out.length,
            Ticks(Ticks::quarters(1).0 / 4),
            "its own fade is replaced"
        );
        let shown = preview(&p, id, &right);
        assert!(!shown.crossfade);
        assert_eq!(
            shown.fade_in.length,
            Ticks::ZERO,
            "the incoming clip has no fade-in handle"
        );
        let (next, found) = outgoing(&p, id, &left).unwrap();
        assert_eq!((next, found.left), (right.id, left.id));
    }

    #[test]
    fn a_warped_clips_tail_is_what_its_map_leaves_in_the_file_not_the_tempo() {
        use lumen_project::model::Warp;
        let mut p = pair();
        p.auto_crossfade = true;
        let id = p.tracks[0].id;
        let two = Ticks::quarters(2);
        let mut warp = Warp::new(120.0, 6.996);
        warp.markers = vec![
            lumen_core::warp::Marker::new(6.996, Ticks::ZERO),
            lumen_core::warp::Marker::new(7.996, two),
            lumen_core::warp::Marker::new(8.0, two + Ticks(2_822)),
        ];
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).warp = Some(warp);
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.zone, Zone::After);
        assert!(
            found.fade.length > Ticks::ZERO && found.fade.length <= Ticks(2_822),
            "{:?}",
            found.fade.length
        );
        let mut spent = Warp::new(120.0, 7.0);
        spent.markers = vec![lumen_core::warp::Marker::new(7.0, Ticks::ZERO)];
        spent
            .markers
            .push(lumen_core::warp::Marker::new(8.0, Ticks::quarters(1)));
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).warp = Some(spent);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
    }

    #[test]
    fn an_automatic_crossfade_blends_five_ms_over_the_left_clips_tail() {
        let mut p = pair();
        p.auto_crossfade = true;
        let id = p.tracks[0].id;
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.zone, Zone::After);
        let tempo = TempoMap::new(p.tempo.clone()).unwrap();
        assert_eq!(
            tempo.sample_at(right.start + found.fade.length, 48_000)
                - tempo.sample_at(right.start, 48_000),
            240
        );
        let played = played(&p);
        let out = &played[&p.tracks[0].clips[0].id].clip;
        assert_eq!(out.length, p.tracks[0].clips[0].length + found.fade.length);
        assert_eq!(
            played[&right.id].clip.start, right.start,
            "the right clip stays"
        );
        assert_eq!(played[&right.id].clip.fade_in.length, found.fade.length);
        right_mut(&mut p).fade_in.length = Ticks(10);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        right_mut(&mut p).fade_in.length = Ticks::ZERO;
        p.auto_crossfade = false;
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none());
        p.auto_crossfade = true;
        let ClipContent::Audio { media, .. } = p.tracks[0].clips[0].content else {
            unreachable!()
        };
        p.media.iter_mut().find(|m| m.id == media).unwrap().frames = 48_000;
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).is_none(), "no audio past the end");
    }

    fn warped_pair(p: &mut Project) {
        for index in [0, 1] {
            let clip = (*p.tracks[0].clips[index]).clone();
            let warped = lumen_project::edit::warp::enable(p, &clip, 120.0).unwrap();
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[index]).warp = warped.warp;
        }
    }

    #[test]
    fn warped_clips_cross_over_the_audio_their_map_keeps_before_the_start() {
        let mut p = pair();
        warped_pair(&mut p);
        by_hand(&mut p, Ticks::quarters(4));
        let id = p.tracks[0].id;
        let right = p.tracks[0].clips[1].clone();
        let found = crossing(&p, id, &right).unwrap();
        assert_eq!(found.fade.length, Ticks(Ticks::quarters(1).0 / 2));
        assert!(!found.seamless, "unrelated sound");
        let heard = &played(&p)[&right.id].clip;
        let (was, now) = (right.warp.as_ref().unwrap(), heard.warp.as_ref().unwrap());
        assert!((now.source_at(Ticks::ZERO) - (was.source_at(Ticks::ZERO) - 0.25)).abs() < 1e-9);
        assert!((now.source_at(found.fade.length) - was.source_at(Ticks::ZERO)).abs() < 1e-9);
    }

    #[test]
    fn a_split_of_a_warped_clip_is_seamless_and_a_warped_next_to_a_plain_one_is_not() {
        let mut p = pair();
        let id = p.tracks[0].id;
        let left = (*p.tracks[0].clips[0]).clone();
        let mut whole = left.clone();
        whole.length = Ticks::quarters(4);
        let whole = lumen_project::edit::warp::enable(&p, &whole, 120.0).unwrap();
        Arc::make_mut(&mut p.tracks[0]).clips = vec![Arc::new(whole.clone())];
        let mut next = 1_000;
        let mut fresh = || {
            next += 1;
            Id(next)
        };
        let parts = lumen_project::edit::split::clips(
            &mut p,
            &[whole.id],
            &[Ticks::quarters(2)],
            &mut fresh,
        );
        assert_eq!(parts.len(), 2);
        let right = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]);
        right.crossfade = true;
        right.fade_in.length = Ticks(Ticks::quarters(1).0 / 4);
        let right = p.tracks[0].clips[1].clone();
        assert!(crossing(&p, id, &right).unwrap().seamless);
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[1]).warp = None;
        let right = p.tracks[0].clips[1].clone();
        assert!(!crossing(&p, id, &right).is_some_and(|c| c.seamless));
    }
}
