//! Consolidate (M7b slice E3), the pure parts. Selected audio clips on a track
//! are rendered into one new file and replaced by one clip that plays it. The
//! rendering is the app's (it runs on a worker); this module says which clips
//! go into which file, and swaps them for the clip that plays it.
//!
//! What the file holds is the clips as they play, clip by clip: gain,
//! polarity, reverse, fades, crossfades and envelopes. The span is from the
//! first clip's start to the last clip's end, gaps as silence. A track's
//! fader, pan, sends and devices are not in it. The original media stays in
//! the project, so the media pool still lists it.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Color, Id, Media, Project};

use super::overwrite;

/// One new file's worth of clips: the selected audio clips of one track.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub track: Id,
    /// The clips as they were when the part was planned, in time order. A
    /// part is applied only if the track still holds exactly these.
    pub clips: Vec<Arc<Clip>>,
    /// Every clip of the track that touches the span when it was planned,
    /// selected or not, in time order. The new clip cuts away whatever lies
    /// under it, so a part is applied only if the span still holds exactly
    /// these: a clip moved or drawn into it while the file rendered would
    /// otherwise be lost to a result that never heard it.
    pub span_clips: Vec<Arc<Clip>>,
    /// From the first clip's start to the last clip's end.
    pub start: Ticks,
    pub end: Ticks,
    /// The new clip's name.
    pub name: String,
    pub color: Option<Color>,
    /// Every clip is mono, so the file is too.
    pub mono: bool,
}

/// The parts for the selected clips: one per track that has an unlocked audio
/// clip selected. Other kinds of clip are left out, and so is a frozen track.
pub fn plan(project: &Project, ids: &[Id]) -> Vec<Part> {
    let mut parts = Vec::new();
    for track in project.tracks.iter().filter(|t| t.frozen.is_none()) {
        let clips: Vec<Arc<Clip>> = track
            .clips
            .iter()
            .filter(|c| ids.contains(&c.id) && !c.locked)
            .filter(|c| matches!(c.content, ClipContent::Audio { .. }))
            .cloned()
            .collect();
        let (Some(first), Some(end)) = (
            clips.first(),
            clips.iter().map(|c| c.start + c.length).max(),
        ) else {
            continue;
        };
        let mono = clips.iter().all(|c| {
            let ClipContent::Audio { media, .. } = &c.content else {
                return false;
            };
            project
                .media
                .iter()
                .find(|m| m.id == *media)
                .is_some_and(|m| m.channels == 1)
        });
        let start = first.start;
        parts.push(Part {
            track: track.id,
            span_clips: touching(track, start, end),
            start,
            end,
            name: named(&first.name),
            color: first.color,
            mono,
            clips,
        });
    }
    parts
}

/// The name of a clip made from one called `name`.
fn named(name: &str) -> String {
    const SUFFIX: &str = " (joined)";
    if name.ends_with(SUFFIX) {
        name.to_owned()
    } else {
        format!("{name}{SUFFIX}")
    }
}

/// A part and the file rendered for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Baked {
    pub part: Part,
    pub media: Media,
}

/// The clips of `track` that touch `start..end`, in the track's order.
fn touching(track: &crate::model::Track, start: Ticks, end: Ticks) -> Vec<Arc<Clip>> {
    track
        .clips
        .iter()
        .filter(|c| c.start < end && c.start + c.length > start)
        .cloned()
        .collect()
}

/// Whether the span of `part` still holds exactly the clips it was planned
/// from, and nothing else: the planned clips unchanged, and no other clip
/// moved, drawn or pasted into the span.
pub fn is_current(project: &Project, part: &Part) -> bool {
    let Some(track) = project.tracks.iter().find(|t| t.id == part.track) else {
        return false;
    };
    let now = touching(track, part.start, part.end);
    now.len() == part.span_clips.len()
        && now
            .iter()
            .zip(&part.span_clips)
            .all(|(c, planned)| Arc::ptr_eq(c, planned) || **c == **planned)
        && part
            .clips
            .iter()
            .all(|planned| now.iter().any(|c| c.id == planned.id))
}

/// Replaces each part's clips with one clip that plays its file, and
/// registers the files as media. A clip of another track or one that covered
/// part of the span without being selected is cut away where the new clip
/// lies, as any clip put down on others does. Returns the new clips' ids, in
/// the order of `baked`; parts that are no longer current are skipped.
pub fn apply(project: &mut Project, baked: &[Baked], fresh: &mut impl FnMut() -> Id) -> Vec<Id> {
    let mut made = Vec::new();
    for Baked { part, media } in baked {
        if !is_current(project, part) {
            continue;
        }
        let id = fresh();
        let Some(track) = project.tracks.iter_mut().find(|t| t.id == part.track) else {
            continue;
        };
        let track = Arc::make_mut(track);
        track
            .clips
            .retain(|c| !part.clips.iter().any(|p| p.id == c.id));
        track.clips.push(Arc::new(Clip {
            id,
            name: part.name.clone(),
            start: part.start,
            length: (part.end - part.start).max(Ticks(1)),
            muted: false,
            color: part.color,
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
            content: ClipContent::Audio {
                media: media.id,
                offset: 0,
                gain_db: 0.0,
            },
        }));
        track.clips.sort_by_key(|c| c.start);
        if !project.media.iter().any(|m| m.id == media.id) {
            project.media.push(media.clone());
        }
        overwrite(project, &[id], fresh);
        made.push(id);
    }
    made
}

/// The unlocked MIDI clips among `ids`, not on a frozen track.
pub fn midi_clips(project: &Project, ids: &[Id]) -> Vec<Id> {
    project
        .tracks
        .iter()
        .filter(|t| t.frozen.is_none())
        .flat_map(|t| t.clips.iter())
        .filter(|c| ids.contains(&c.id) && !c.locked)
        .filter(|c| matches!(c.content, ClipContent::Midi { .. }))
        .map(|c| c.id)
        .collect()
}

/// Consolidates the selected unlocked MIDI clips: each repeat of a looping
/// clip becomes a real clip, then the clips of each track merge into one
/// holding every note they show (a gap between them is just empty time).
/// No file is made. Audio clips in `ids` are not touched. Returns the ids
/// now standing for the MIDI selection.
pub fn midi(project: &mut Project, ids: &[Id], fresh: &mut impl FnMut() -> Id) -> Vec<Id> {
    let picked = midi_clips(project, ids);
    if picked.is_empty() {
        return picked;
    }
    let (now, _) = super::clip_ops::unloop(project, &picked, fresh);
    super::clip_ops::join(project, &now).clips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{clips, ids, q, song};
    use crate::model::Note;

    fn media(id: u64) -> Media {
        Media {
            id: Id(id),
            path: "baked.wav".into(),
            original_path: None,
            frames: 480_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 0,
        }
    }

    /// Track 0 with clips at 0..2, 3..5 and 8..10 (quarters), all unlocked.
    fn three_clips() -> Project {
        let mut project = song();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let template = track.clips[0].clone();
        track.clips = [(1, 0, 2), (2, 3, 2), (3, 8, 2)]
            .into_iter()
            .map(|(id, start, length)| {
                let mut clip = (*template).clone();
                clip.id = Id(id);
                clip.start = q(start);
                clip.length = q(length);
                Arc::new(clip)
            })
            .collect();
        project
    }

    #[test]
    fn a_frozen_track_is_left_out_of_the_plan() {
        let mut project = three_clips();
        let media = project.media[0].id;
        Arc::make_mut(&mut project.tracks[0]).frozen = Some(crate::model::Frozen {
            id: Id(900),
            media,
            start: q(0),
            length: q(10),
            mono: false,
        });
        let parts = plan(&project, &[Id(1), Id(2), Id(3)]);
        assert!(parts.iter().all(|p| p.track != project.tracks[0].id));
    }

    #[test]
    fn a_part_spans_the_first_start_to_the_last_end_and_leaves_other_tracks_and_locked_clips() {
        let mut project = three_clips();
        Arc::make_mut(&mut project.tracks[0]).clips[2] = {
            let mut locked = (*project.tracks[0].clips[2]).clone();
            locked.locked = true;
            Arc::new(locked)
        };
        let parts = plan(&project, &[Id(1), Id(2), Id(3), Id(20)]);
        assert_eq!(
            parts.len(),
            2,
            "tracks 0 and 1 each have something selected"
        );
        assert_eq!(parts[0].track, project.tracks[0].id);
        assert_eq!((parts[0].start, parts[0].end), (q(0), q(5)));
        assert_eq!(parts[0].clips.len(), 2, "the locked clip stays out");
        assert_eq!(parts[0].name, "c10 (joined)");
        assert_eq!(parts[1].track, project.tracks[1].id);
        assert!(plan(&project, &[Id(999)]).is_empty());
    }

    #[test]
    fn the_part_is_mono_only_when_every_media_file_is() {
        let mut project = three_clips();
        assert!(!plan(&project, &[Id(1)])[0].mono);
        project.media[0].channels = 1;
        assert!(plan(&project, &[Id(1), Id(2)])[0].mono);
    }

    #[test]
    fn a_name_is_not_suffixed_twice() {
        assert_eq!(named("Kick"), "Kick (joined)");
        assert_eq!(named("Kick (joined)"), "Kick (joined)");
    }

    #[test]
    fn applying_replaces_the_clips_with_one_that_plays_the_new_file() {
        let mut project = three_clips();
        let part = plan(&project, &[Id(1), Id(2)]).remove(0);
        let made = apply(
            &mut project,
            &[Baked {
                part,
                media: media(777),
            }],
            &mut ids(),
        );
        assert_eq!(made.len(), 1);
        // The unselected clip stays; the two selected ones became one.
        assert_eq!(
            clips(&project, 0),
            vec![(made[0].0, 0, 5, 0), (3, 8, 2, 1_000)]
        );
        let clip = &project.tracks[0].clips[0];
        assert!(matches!(
            clip.content,
            ClipContent::Audio {
                media: Id(777),
                offset: 0,
                ..
            }
        ));
        assert!(project.media.iter().any(|m| m.id == Id(777)));
        assert!(
            project.media.iter().any(|m| m.id == Id(900)),
            "the original stays"
        );
    }

    #[test]
    fn an_unselected_clip_inside_the_span_is_cut_away_by_the_new_clip() {
        let mut project = three_clips();
        let part = plan(&project, &[Id(1), Id(3)]).remove(0);
        assert_eq!((part.start, part.end), (q(0), q(10)));
        apply(
            &mut project,
            &[Baked {
                part,
                media: media(777),
            }],
            &mut ids(),
        );
        // The clip between them was covered, so only the new clip is left.
        assert_eq!(project.tracks[0].clips.len(), 1);
        assert_eq!(project.tracks[0].clips[0].length, q(10));
    }

    #[test]
    fn a_part_is_skipped_when_its_clips_changed_or_went() {
        let mut project = three_clips();
        let part = plan(&project, &[Id(1), Id(2)]).remove(0);
        let baked = [Baked {
            part: part.clone(),
            media: media(777),
        }];
        // A clip moved.
        let mut moved = project.clone();
        let track = Arc::make_mut(&mut moved.tracks[0]);
        let mut clip = (*track.clips[1]).clone();
        clip.start = q(4);
        track.clips[1] = Arc::new(clip);
        assert!(!is_current(&moved, &part));
        assert!(apply(&mut moved, &baked, &mut ids()).is_empty());
        assert!(!moved.media.iter().any(|m| m.id == Id(777)));
        // A clip deleted.
        Arc::make_mut(&mut project.tracks[0]).clips.remove(0);
        assert!(apply(&mut project, &baked, &mut ids()).is_empty());
    }

    #[test]
    fn a_clip_put_into_the_span_while_it_rendered_makes_the_part_stale() {
        let project = three_clips();
        // Clips 1 and 3 are selected; the gap holds clip 2, which is planned.
        let part = plan(&project, &[Id(1), Id(3)]).remove(0);
        assert_eq!(part.span_clips.len(), 3);
        let baked = [Baked {
            part: part.clone(),
            media: media(777),
        }];
        // An unselected clip drawn into the span: the old result would cut it.
        let mut drawn = project.clone();
        let track = Arc::make_mut(&mut drawn.tracks[0]);
        let mut clip = (*track.clips[0]).clone();
        clip.id = Id(50);
        clip.start = q(6);
        clip.length = q(1);
        track.clips.insert(2, Arc::new(clip));
        assert!(!is_current(&drawn, &part));
        assert!(apply(&mut drawn, &baked, &mut ids()).is_empty());
        assert_eq!(drawn.tracks[0].clips.len(), 4, "the new clip is kept");
        // The unselected clip in the span that was planned going away is stale too.
        let mut gone = project.clone();
        Arc::make_mut(&mut gone.tracks[0]).clips.remove(1);
        assert!(!is_current(&gone, &part));
        // An edit outside the span does not matter.
        let mut outside = project.clone();
        let track = Arc::make_mut(&mut outside.tracks[0]);
        let mut clip = (*track.clips[0]).clone();
        clip.id = Id(51);
        clip.start = q(20);
        track.clips.push(Arc::new(clip));
        assert!(is_current(&outside, &part));
        assert!(is_current(&project, &part));
    }

    /// Track 0's clip made a MIDI clip of `notes` (starting at 4 quarters, 8 long).
    fn midi_song(notes: Vec<Note>) -> Project {
        let mut project = song();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let mut clip = (*track.clips[0]).clone();
        clip.content = ClipContent::Midi {
            notes,
            events: Vec::new(),
        };
        track.clips[0] = Arc::new(clip);
        project
    }

    fn shown(project: &Project, track: usize) -> Vec<Vec<(i64, i64, u8)>> {
        project.tracks[track]
            .clips
            .iter()
            .map(|c| {
                let ClipContent::Midi { notes, .. } = &c.content else {
                    return Vec::new();
                };
                notes
                    .iter()
                    .map(|n| (n.start().0 / q(1).0, n.length().0 / q(1).0, n.pitch()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_looping_midi_clip_becomes_one_clip_with_the_repeats_baked() {
        let mut project = midi_song(vec![Note::new(q(0), q(2), 60, 90)]);
        let track = Arc::make_mut(&mut project.tracks[0]);
        let mut clip = (*track.clips[0]).clone();
        clip.loop_length = Some(q(3));
        clip.length = q(7);
        track.clips[0] = Arc::new(clip);
        let made = midi(&mut project, &[Id(10)], &mut ids());
        assert_eq!(made, [Id(10)], "the clip keeps its id");
        assert_eq!(project.tracks[0].clips.len(), 1);
        let clip = &project.tracks[0].clips[0];
        assert_eq!((clip.start, clip.length), (q(4), q(7)));
        assert_eq!(clip.loop_length, None);
        assert_eq!(shown(&project, 0), [[(0, 2, 60), (3, 2, 60), (6, 1, 60)]]);
    }

    #[test]
    fn midi_clips_with_a_gap_become_one_clip_and_audio_is_left_alone() {
        let mut project = midi_song(vec![Note::new(q(1), q(1), 60, 90)]);
        let track = Arc::make_mut(&mut project.tracks[0]);
        let mut second = (*track.clips[0]).clone();
        second.id = Id(11);
        second.start = q(14);
        second.length = q(4);
        second.content = ClipContent::Midi {
            notes: vec![Note::new(q(2), q(1), 72, 100)],
            events: Vec::new(),
        };
        track.clips.push(Arc::new(second));
        let audio = project.tracks[1].clone();
        let made = midi(&mut project, &[Id(10), Id(11), Id(20)], &mut ids());
        assert_eq!(made, [Id(10)]);
        assert_eq!(project.tracks[0].clips.len(), 1);
        let clip = &project.tracks[0].clips[0];
        assert_eq!((clip.start, clip.length), (q(4), q(14)));
        assert_eq!(shown(&project, 0), [[(1, 1, 60), (12, 1, 72)]]);
        assert_eq!(project.tracks[1], audio, "the audio clip is not touched");
    }

    #[test]
    fn a_single_plain_or_locked_midi_clip_is_left_as_it_was() {
        let mut project = midi_song(vec![Note::new(q(1), q(1), 60, 90)]);
        let before = project.clone();
        midi(&mut project, &[Id(10)], &mut ids());
        assert_eq!(project, before);
        let track = Arc::make_mut(&mut project.tracks[0]);
        let mut clip = (*track.clips[0]).clone();
        clip.locked = true;
        clip.loop_length = Some(q(3));
        track.clips[0] = Arc::new(clip);
        let before = project.clone();
        assert!(midi(&mut project, &[Id(10)], &mut ids()).is_empty());
        assert_eq!(project, before);
    }
}
