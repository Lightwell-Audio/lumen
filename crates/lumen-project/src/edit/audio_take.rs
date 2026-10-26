//! Recorded audio (M9 slice R2), the pure part: a finished take is a file, and
//! the file goes onto its track as a clip over the span it was played in.
//!
//! The M3 overlap rule applies, as for any clip put down on others: the take
//! wins and whatever it covers is cut away (one undo step restores it). A
//! locked clip is never cut, so a take that would land on one goes on a new
//! audio track after the source instead, and the answer says so.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Id, Media, Pass, Project, Track, TrackKind};

use super::{overwrite, takes};

/// A recorded file to put on a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Take {
    pub track: Id,
    pub media: Media,
    /// The clip's name.
    pub name: String,
    /// Where the file's first frame is played.
    pub start: Ticks,
    /// How long the file plays.
    pub length: Ticks,
    /// A loop recording's passes (M9 R3): the frame of the file at the clip's
    /// start for each pass, in order. The clip plays the last and keeps the
    /// others as takes. Fewer than two leave the clip as a single take.
    pub passes: Vec<i64>,
    /// The pass the clip plays, an index into `passes`.
    pub active: usize,
}

/// What [`apply`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Placed {
    /// The new clips, in the order of the takes that could be placed.
    pub clips: Vec<Id>,
    /// Tracks made for takes that a locked clip kept off their own track.
    pub new_tracks: Vec<Id>,
}

/// Puts each take on its track and registers its file as media. A take whose
/// track is gone, or is not an audio track, is left out.
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
            content: ClipContent::Audio {
                media: take.media.id,
                offset: 0,
                gain_db: 0.0,
            },
        };
        if take.passes.len() >= 2 {
            let passes = take
                .passes
                .iter()
                .map(|&offset| Pass::Audio {
                    offset,
                    audio_phase_seconds: 0.0,
                })
                .collect::<Vec<_>>();
            let active = take.active.min(passes.len() - 1);
            takes::with_passes(&mut clip, passes, active);
        }
        let clip = Arc::new(clip);
        if !project.media.iter().any(|m| m.id == take.media.id) {
            project.media.push(take.media.clone());
        }
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

/// A new audio track after the source (and its group's other tracks), in the
/// source's group, holding the clip.
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
        }
    }

    #[test]
    fn a_take_lands_on_its_track_and_cuts_what_it_covers() {
        let mut p = song();
        // Track 0 holds one clip from 4 to 12: a take over 6..8 splits it.
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
                audio_phase_seconds: 0.0
            })
        );
        assert!(crate::validate(&p).is_ok());
    }

    #[test]
    fn one_pass_is_the_single_take_of_before() {
        let mut p = song();
        let mut t = take(&p, 2, 0, 2);
        t.passes = vec![480];
        apply(&mut p, &[t], &mut ids());
        let clip = &p.tracks[2].clips[0];
        assert!(clip.takes.is_none());
        assert!(matches!(clip.content, ClipContent::Audio { offset: 0, .. }));
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
