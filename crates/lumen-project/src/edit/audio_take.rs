use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Id, Media, Pass, Project, Track, TrackKind, WarpMode};

use super::{overwrite, takes, warp};

#[derive(Debug, Clone, PartialEq)]
pub struct Take {
    pub track: Id,
    pub media: Media,
    pub name: String,
    pub start: Ticks,
    pub length: Ticks,
    pub passes: Vec<i64>,
    pub active: usize,
    pub follow: Option<WarpMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Placed {
    pub clips: Vec<Id>,
    pub new_tracks: Vec<Id>,
}

pub fn apply(project: &mut Project, takes: &[Take], fresh: &mut impl FnMut() -> Id) -> Placed {
    let mut placed = Placed::default();
    for take in takes {
        let Some(at) = project
            .tracks
            .iter()
            .position(|t| t.id == take.track && t.kind == TrackKind::Audio)
        else {
            continue;
        };
        let id = fresh();
        let mut clip = Clip {
            id,
            name: take.name.clone(),
            start: take.start,
            length: take.length.max(Ticks(1)),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: ClipContent::Audio {
                media: take.media.id,
                offset: 0,
                gain_db: 0.0,
            },
        };
        if !take.passes.is_empty() {
            let passes = take
                .passes
                .iter()
                .map(|&offset| Pass::Audio {
                    offset,
                    audio_phase_seconds: 0.0,
                    warp: None,
                })
                .collect::<Vec<_>>();
            let active = take.active.min(passes.len() - 1);
            takes::with_passes(&mut clip, passes, active);
        }
        if !project.media.iter().any(|m| m.id == take.media.id) {
            project.media.push(take.media.clone());
        }
        if let Some(mode) = take.follow
            && let Ok(warped) = warp::as_recorded(project, &clip, mode)
        {
            clip = warped;
        }
        let clip = Arc::new(clip);
        let end = clip.start + clip.length;
        let blocked = project.tracks[at]
            .clips
            .iter()
            .any(|c| c.locked && c.start < end && c.start + c.length > clip.start);
        if blocked {
            let track = fresh();
            put_on_new_track(project, at, track, clip);
            placed.new_tracks.push(track);
        } else {
            let track = Arc::make_mut(&mut project.tracks[at]);
            track.clips.push(clip);
            track.clips.sort_by_key(|c| c.start);
            overwrite(project, &[id], fresh);
        }
        placed.clips.push(id);
    }
    placed
}

pub(super) fn put_on_new_track(project: &mut Project, at: usize, id: Id, clip: Arc<Clip>) {
    let source = Arc::clone(&project.tracks[at]);
    let index = crate::groups::span(&project.tracks, at).end;
    let mut track = Track::new(
        id,
        format!("{} take", source.name),
        TrackKind::Audio,
        source.color,
    );
    track.parent = source.parent;
    track.clips = vec![clip];
    project.tracks.insert(index, Arc::new(track));
    crate::groups::normalize(&mut project.tracks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{clips, ids, q, song};
    use lumen_core::warp::Marker;

    fn file(id: u64) -> Media {
        Media {
            id: Id(id),
            path: "Take.wav".into(),
            original_path: None,
            frames: 96_000,
            channels: 1,
            sample_rate: 48_000,
            size_bytes: 0,
        }
    }

    fn take(project: &Project, track: usize, start: i64, length: i64) -> Take {
        Take {
            track: project.tracks[track].id,
            media: file(777),
            name: "Vox take".into(),
            start: q(start),
            length: q(length),
            passes: Vec::new(),
            active: 0,
            follow: None,
        }
    }

    fn the_clip(p: &Project, track: usize) -> Arc<Clip> {
        Arc::clone(p.tracks[track].clips.last().expect("a clip"))
    }

    #[test]
    fn a_take_that_follows_the_tempo_is_made_at_the_songs_tempo() {
        let mut p = song();
        let mut t = take(&p, 2, 0, 4);
        t.follow = Some(WarpMode::FullMix);
        apply(&mut p, &[t], &mut ids());
        let clip = the_clip(&p, 2);
        let warp = clip.warp.as_ref().expect("warp on");
        assert_eq!((warp.mode, warp.original_bpm), (WarpMode::FullMix, 120.0));
        assert_eq!(warp.markers.len(), 1, "a steady tempo needs one marker");
        assert!(matches!(clip.content, ClipContent::Audio { offset: 0, .. }));
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn a_take_that_does_not_follow_is_unwarped() {
        let mut p = song();
        let t = take(&p, 2, 0, 4);
        apply(&mut p, &[t], &mut ids());
        assert!(the_clip(&p, 2).warp.is_none());
    }

    #[test]
    fn a_tempo_change_inside_a_take_gets_a_marker_that_keeps_it_as_recorded() {
        let mut p = song();
        p.tempo.push(lumen_core::time::TempoPoint::step(q(2), 60.0));
        p.tempo
            .push(lumen_core::time::TempoPoint::step(q(100), 90.0));
        let mut t = take(&p, 2, 0, 4);
        t.follow = Some(WarpMode::FullMix);
        apply(&mut p, &[t], &mut ids());
        let warp = the_clip(&p, 2).warp.clone().expect("warp on");
        assert_eq!(
            warp.markers,
            [
                Marker::new(0.0, q(0)),
                Marker::new(1.0, q(2)),
                Marker::new(3.0, q(4))
            ]
        );
        let at_end = lumen_core::warp::source_at(&warp.markers, warp.original_bpm, q(4));
        assert!((at_end - 3.0).abs() < 1e-9, "the end plays second {at_end}");
        let third = lumen_core::warp::source_at(&warp.markers, warp.original_bpm, q(3));
        assert!(
            (third - 2.0).abs() < 1e-9,
            "beat three plays second {third}"
        );
        assert_eq!(warp.original_bpm, 120.0);
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn a_tempo_ramp_inside_a_take_is_followed_closely_and_passes_get_the_same() {
        let mut p = song();
        p.tempo[0].ramp = true;
        p.tempo.push(lumen_core::time::TempoPoint::step(q(4), 60.0));
        let mut t = take(&p, 2, 0, 4);
        t.follow = Some(WarpMode::FullMix);
        t.passes = vec![0, 96_000];
        t.active = 1;
        apply(&mut p, &[t], &mut ids());
        let clip = the_clip(&p, 2);
        let warp = clip.warp.as_ref().expect("warp on");
        assert!(warp.markers.len() > 4, "a marker every few ticks");
        assert!(lumen_core::warp::is_sound(&warp.markers));
        let tempo = crate::edit::tempo_of(&p);
        for m in &warp.markers {
            let second = warp.markers[0].source + tempo.seconds_at(m.at);
            assert!((m.source - second).abs() < 1e-9, "{m:?} against {second}");
        }
        let other = clip
            .takes
            .as_ref()
            .and_then(|t| t.other(0))
            .expect("a pass");
        let Pass::Audio {
            warp: Some(other), ..
        } = other
        else {
            panic!("the pass follows too");
        };
        assert_eq!(other.markers.len(), warp.markers.len());
        assert_eq!(other.markers[0].source, 0.0);
        assert_eq!(warp.markers[0].source, 2.0);
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn a_short_take_inside_a_ramp_still_has_its_end_pinned() {
        let mut p = song();
        p.tempo[0].ramp = true;
        p.tempo
            .push(lumen_core::time::TempoPoint::step(q(1), 240.0));
        let mut t = take(&p, 2, 0, 1);
        t.length = lumen_core::time::Ticks(lumen_core::time::PPQ / 8);
        t.follow = Some(WarpMode::FullMix);
        apply(&mut p, &[t], &mut ids());
        let clip = the_clip(&p, 2);
        let warp = clip.warp.as_ref().expect("warp on");
        let tempo = crate::edit::tempo_of(&p);
        let end = lumen_core::time::Ticks(lumen_core::time::PPQ / 8);
        assert_eq!(warp.markers.last().map(|m| m.at), Some(end));
        let want = tempo.seconds_at(end);
        let got = lumen_core::warp::source_at(&warp.markers, warp.original_bpm, end);
        assert!((got - want).abs() < 1e-9, "the end plays {got}, not {want}");
        let mut p = song();
        p.tempo[0].ramp = true;
        p.tempo
            .push(lumen_core::time::TempoPoint::step(q(1), 240.0));
        let mut t = take(&p, 2, 8, 2);
        t.follow = Some(WarpMode::FullMix);
        apply(&mut p, &[t], &mut ids());
        let clip = the_clip(&p, 2);
        assert_eq!(clip.warp.as_ref().expect("warp").markers.len(), 1);
    }

    #[test]
    fn a_take_lands_on_its_track_and_cuts_what_it_covers() {
        let mut p = song();
        let t = take(&p, 0, 6, 2);
        let placed = apply(&mut p, &[t], &mut ids());
        assert_eq!(placed.clips.len(), 1);
        assert!(placed.new_tracks.is_empty());
        let spans: Vec<(i64, i64)> = clips(&p, 0).iter().map(|c| (c.1, c.2)).collect();
        assert_eq!(spans, [(4, 2), (6, 2), (8, 4)]);
        assert!(p.media.iter().any(|m| m.id == Id(777)), "its file is media");
        let clip = p.tracks[0]
            .clips
            .iter()
            .find(|c| c.id == placed.clips[0])
            .unwrap();
        assert_eq!(clip.name, "Vox take");
        assert!(matches!(
            clip.content,
            ClipContent::Audio {
                media: Id(777),
                offset: 0,
                ..
            }
        ));
    }

    #[test]
    fn a_loop_recording_plays_its_last_pass_and_keeps_the_others() {
        let mut p = song();
        let mut t = take(&p, 2, 0, 2);
        t.passes = vec![0, 96_000, 192_000];
        t.active = 2;
        let placed = apply(&mut p, &[t], &mut ids());
        let clip = p.tracks[2]
            .clips
            .iter()
            .find(|c| c.id == placed.clips[0])
            .unwrap();
        assert!(matches!(
            clip.content,
            ClipContent::Audio {
                offset: 192_000,
                ..
            }
        ));
        let takes = clip.takes.as_ref().unwrap();
        assert_eq!((takes.active, takes.count), (2, 3));
        assert_eq!(
            takes.other(0),
            Some(&Pass::Audio {
                offset: 0,
                audio_phase_seconds: 0.0,
                warp: None,
            })
        );
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn one_pass_is_a_plain_clip_that_plays_from_its_frame() {
        let mut p = song();
        let mut t = take(&p, 2, 0, 2);
        t.passes = vec![480];
        apply(&mut p, &[t], &mut ids());
        let clip = &p.tracks[2].clips[0];
        assert!(clip.takes.is_none());
        assert!(matches!(
            clip.content,
            ClipContent::Audio { offset: 480, .. }
        ));
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn a_take_on_an_empty_track_is_the_only_clip() {
        let mut p = song();
        let t = take(&p, 2, 0, 4);
        apply(&mut p, &[t], &mut ids());
        assert_eq!(
            clips(&p, 2).iter().map(|c| (c.1, c.2)).collect::<Vec<_>>(),
            [(0, 4)]
        );
    }

    #[test]
    fn two_takes_are_placed_together_and_a_missing_track_is_skipped() {
        let mut p = song();
        let mut gone = take(&p, 2, 0, 1);
        gone.track = Id(424_242);
        let takes = [take(&p, 1, 8, 2), gone, take(&p, 2, 0, 2)];
        let placed = apply(&mut p, &takes, &mut ids());
        assert_eq!(placed.clips.len(), 2);
        assert_eq!(p.media.iter().filter(|m| m.id == Id(777)).count(), 1);
    }

    #[test]
    fn a_locked_clip_is_never_cut_the_take_goes_on_a_new_track_after_it() {
        let mut p = song();
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = true;
        let before = p.tracks.len();
        let t = take(&p, 0, 6, 2);
        let placed = apply(&mut p, &[t], &mut ids());
        assert_eq!(p.tracks.len(), before + 1);
        assert_eq!(placed.new_tracks.len(), 1);
        assert_eq!(clips(&p, 0).len(), 1, "the locked clip is as it was");
        assert_eq!(
            p.tracks[1].id, placed.new_tracks[0],
            "right after the source"
        );
        assert_eq!(
            p.tracks[1].name,
            "Track 1 take".replace("Track 1", &p.tracks[0].name)
        );
        assert_eq!(p.tracks[1].clips[0].id, placed.clips[0]);
    }

    #[test]
    fn a_midi_track_takes_no_audio() {
        let mut p = song();
        Arc::make_mut(&mut p.tracks[2]).kind = TrackKind::Midi;
        let t = take(&p, 2, 0, 2);
        assert!(apply(&mut p, &[t], &mut ids()).clips.is_empty());
    }
}
