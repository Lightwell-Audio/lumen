//! Takes (M9 slice R3, format v31): playing another pass of a loop recording,
//! taking one out to its own track, and dropping the rest.
//!
//! A clip's takes are described in `model::takes`. The clip's own content is
//! the active pass; the others are held beside it, so a swap exchanges the
//! two and nothing is copied twice. Every function here changes only clips
//! that are not locked, except [`extract`], which leaves its source alone.

use std::sync::Arc;

use crate::model::{Clip, Id, OtherTake, Pass, Project, Takes, TrackKind};

use super::audio_take;

/// Gives `clip` the passes `passes`, playing number `active`. Fewer than two
/// passes leave the clip without takes. A pass of the wrong kind for the clip
/// returns false and changes nothing.
pub fn with_passes(clip: &mut Clip, mut passes: Vec<Pass>, active: usize) -> bool {
    if passes.len() < 2 || active >= passes.len() {
        if passes.len() != 1 || !passes[0].fits(clip) {
            return false;
        }
        clip.takes = None;
        return passes.pop().is_some_and(|p| p.put_on(clip));
    }
    if !passes.iter().all(|p| p.fits(clip)) {
        return false;
    }
    let mut played = clip.clone();
    if !passes[active].clone().put_on(&mut played) {
        return false;
    }
    let others = passes
        .into_iter()
        .enumerate()
        .filter(|(n, _)| *n != active)
        .map(|(n, pass)| OtherTake {
            take: n as u32,
            pass,
        })
        .collect::<Vec<_>>();
    let count = others.len() as u32 + 1;
    played.takes = Some(Takes::new(active as u32, count, others));
    *clip = played;
    true
}

/// Where the clip `id` is, as track and clip indexes.
fn find(project: &Project, id: Id) -> Option<(usize, usize)> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(t, track)| track.clips.iter().position(|c| c.id == id).map(|c| (t, c)))
}

/// Makes `clip` play pass `take`, the one it plays going into that pass's
/// place. Returns false when there is no such pass or the clip has none.
pub fn swap(clip: &mut Clip, take: u32) -> bool {
    let Some(takes) = &clip.takes else {
        return false;
    };
    if take == takes.active {
        return false;
    }
    let Some(slot) = takes.others.iter().position(|o| o.take == take) else {
        return false;
    };
    let Some(current) = Pass::of(clip) else {
        return false;
    };
    let mut takes = takes.clone();
    let chosen = std::mem::replace(&mut takes.others[slot].pass, current);
    if !chosen.put_on(clip) {
        return false;
    }
    // The pass now held under `slot` is the one that was active; renumber.
    takes.others[slot].take = takes.active;
    takes.active = take;
    takes.others.sort_by_key(|o| o.take);
    clip.takes = Some(takes);
    true
}

/// Plays pass `take` on the unlocked clip `id`.
pub fn play(project: &mut Project, id: Id, take: u32) -> bool {
    let Some((t, c)) = find(project, id) else {
        return false;
    };
    let mut clip = (*project.tracks[t].clips[c]).clone();
    if clip.locked || !swap(&mut clip, take) {
        return false;
    }
    Arc::make_mut(&mut project.tracks[t]).clips[c] = Arc::new(clip);
    true
}

/// Plays the pass `by` places after the one playing, round the set: 1 is the
/// next take, -1 the one before. Returns the number now playing.
pub fn step(project: &mut Project, id: Id, by: i32) -> Option<u32> {
    let (t, c) = find(project, id)?;
    let takes = project.tracks[t].clips[c].takes.as_ref()?;
    let count = i64::from(takes.count);
    let to = (i64::from(takes.active) + i64::from(by)).rem_euclid(count) as u32;
    play(project, id, to).then_some(to)
}

/// Drops the passes a clip is not playing. Returns how many were dropped.
pub fn settle(clip: &mut Clip) -> u32 {
    clip.takes.take().map_or(0, |takes| takes.count - 1)
}

/// Drops the other passes of each unlocked clip among `ids`. Returns how many
/// passes were dropped in all.
pub fn keep_only(project: &mut Project, ids: &[Id]) -> u32 {
    let mut dropped = 0;
    for track in &mut project.tracks {
        let hit = track
            .clips
            .iter()
            .any(|c| ids.contains(&c.id) && !c.locked && c.takes.is_some());
        if !hit {
            continue;
        }
        let track = Arc::make_mut(track);
        for clip in &mut track.clips {
            if ids.contains(&clip.id) && !clip.locked && clip.takes.is_some() {
                dropped += settle(Arc::make_mut(clip));
            }
        }
    }
    dropped
}

/// What [`extract`] made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extracted {
    pub track: Id,
    pub clip: Id,
}

/// Puts pass `take` of the audio clip `id` on a new track after the clip's
/// own, as a clip over the same span. The source keeps playing what it did.
/// MIDI passes are not extracted: a new MIDI track would have no instrument.
pub fn extract(
    project: &mut Project,
    id: Id,
    take: u32,
    fresh: &mut impl FnMut() -> Id,
) -> Option<Extracted> {
    let (t, c) = find(project, id)?;
    if project.tracks[t].kind != TrackKind::Audio {
        return None;
    }
    let source = &project.tracks[t].clips[c];
    source.takes.as_ref()?;
    let mut copy = (**source).clone();
    // The pass the clip plays is a take too, and needs no swap.
    if copy.takes.as_ref()?.active != take && !swap(&mut copy, take) {
        return None;
    }
    copy.takes = None;
    copy.id = fresh();
    copy.locked = false;
    copy.name = format!("{} (take {})", source.name, take + 1);
    let clip = copy.id;
    let track = fresh();
    audio_take::put_on_new_track(project, t, track, Arc::new(copy));
    Some(Extracted { track, clip })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{ids, song};
    use crate::model::{ClipContent, Note};
    use lumen_core::time::Ticks;

    fn audio(offset: i64) -> Pass {
        Pass::Audio {
            offset,
            audio_phase_seconds: 0.0,
        }
    }

    fn offset(clip: &Clip) -> i64 {
        match clip.content {
            ClipContent::Audio { offset, .. } => offset,
            ClipContent::Midi { .. } => panic!("an audio clip"),
        }
    }

    /// The clip with three audio passes at 100, 200 and 300; it plays the
    /// last.
    fn three() -> (Project, Id) {
        let mut p = song();
        let id = p.tracks[0].clips[0].id;
        let track = Arc::make_mut(&mut p.tracks[0]);
        let clip = Arc::make_mut(&mut track.clips[0]);
        assert!(with_passes(
            clip,
            vec![audio(100), audio(200), audio(300)],
            2
        ));
        (p, id)
    }

    fn clip(p: &Project, id: Id) -> &Clip {
        p.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == id)
            .unwrap()
    }

    #[test]
    fn a_clip_plays_the_pass_it_was_given_and_keeps_the_rest() {
        let (p, id) = three();
        let c = clip(&p, id);
        assert_eq!(offset(c), 300);
        let takes = c.takes.as_ref().unwrap();
        assert_eq!((takes.active, takes.count), (2, 3));
        assert_eq!(takes.other(0), Some(&audio(100)));
        assert_eq!(takes.other(1), Some(&audio(200)));
        assert!(takes.is_sound());
    }

    #[test]
    fn one_pass_is_no_set() {
        let mut p = song();
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        assert!(with_passes(clip, vec![audio(50)], 0));
        assert!(clip.takes.is_none());
        assert_eq!(offset(clip), 50);
    }

    #[test]
    fn playing_a_pass_swaps_and_playing_back_gives_the_clip_again() {
        let (mut p, id) = three();
        let before = p.clone();
        assert!(play(&mut p, id, 0));
        let c = clip(&p, id);
        assert_eq!(offset(c), 100);
        let takes = c.takes.as_ref().unwrap();
        assert_eq!(takes.active, 0);
        assert_eq!(takes.other(2), Some(&audio(300)));
        assert!(takes.is_sound());
        assert!(play(&mut p, id, 2));
        assert_eq!(p, before);
    }

    #[test]
    fn the_pass_playing_and_a_pass_that_is_not_there_change_nothing() {
        let (mut p, id) = three();
        let before = p.clone();
        assert!(!play(&mut p, id, 2));
        assert!(!play(&mut p, id, 9));
        assert!(!play(&mut p, Id(424_242), 0));
        assert_eq!(p, before);
    }

    #[test]
    fn a_locked_clip_plays_what_it_played() {
        let (mut p, id) = three();
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = true;
        assert!(!play(&mut p, id, 0));
        assert_eq!(keep_only(&mut p, &[id]), 0);
        assert!(clip(&p, id).takes.is_some());
    }

    #[test]
    fn stepping_goes_round_the_set() {
        let (mut p, id) = three();
        assert_eq!(step(&mut p, id, 1), Some(0));
        assert_eq!(step(&mut p, id, -1), Some(2));
        assert_eq!(step(&mut p, id, -1), Some(1));
        assert_eq!(offset(clip(&p, id)), 200);
        assert_eq!(step(&mut p, Id(424_242), 1), None);
    }

    #[test]
    fn edits_to_the_playing_pass_survive_a_swap() {
        let mut p = song();
        let id = p.tracks[0].clips[0].id;
        let clip_mut = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        clip_mut.content = ClipContent::Midi {
            notes: vec![],
            events: vec![],
        };
        clip_mut.audio_phase_seconds = 0.0;
        let note = |at: i64, pitch: u8| Note::new(Ticks(at), Ticks(100), pitch, 90);
        let passes = vec![
            Pass::Midi {
                notes: vec![note(0, 60)],
                events: vec![],
            },
            Pass::Midi {
                notes: vec![note(0, 62)],
                events: vec![],
            },
        ];
        assert!(with_passes(clip_mut, passes, 1));
        // Edit the playing pass, as the piano roll would.
        if let ClipContent::Midi { notes, .. } = &mut clip_mut.content {
            notes.push(note(200, 64));
        }
        assert!(swap(clip_mut, 0));
        assert!(swap(clip_mut, 1));
        let ClipContent::Midi { notes, .. } = &clip_mut.content else {
            panic!("a MIDI clip");
        };
        assert_eq!(notes.len(), 2, "the added note is still there");
        let _ = id;
    }

    #[test]
    fn a_pass_of_the_wrong_kind_is_refused() {
        let mut p = song();
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]);
        let before = clip.clone();
        let midi = Pass::Midi {
            notes: vec![],
            events: vec![],
        };
        assert!(!with_passes(clip, vec![audio(1), midi], 0));
        assert_eq!(*clip, before);
    }

    #[test]
    fn keeping_one_drops_the_rest_and_says_how_many() {
        let (mut p, id) = three();
        assert_eq!(keep_only(&mut p, &[id]), 2);
        assert!(clip(&p, id).takes.is_none());
        assert_eq!(offset(clip(&p, id)), 300, "it keeps the pass it plays");
        assert_eq!(keep_only(&mut p, &[id]), 0);
    }

    #[test]
    fn extracting_a_pass_puts_it_on_a_new_track_and_leaves_the_clip() {
        let (mut p, id) = three();
        let before = clip(&p, id).clone();
        let tracks = p.tracks.len();
        let made = extract(&mut p, id, 0, &mut ids()).unwrap();
        assert_eq!(p.tracks.len(), tracks + 1);
        assert_eq!(p.tracks[1].id, made.track, "right after the source");
        let copy = clip(&p, made.clip);
        assert_eq!(offset(copy), 100);
        assert!(copy.takes.is_none());
        assert_eq!((copy.start, copy.length), (before.start, before.length));
        assert_eq!(*clip(&p, id), before, "the source is untouched");
    }

    #[test]
    fn the_pass_already_playing_can_be_extracted_too() {
        let (mut p, id) = three();
        let made = extract(&mut p, id, 2, &mut ids()).unwrap();
        assert_eq!(offset(clip(&p, made.clip)), 300);
        assert!(extract(&mut p, id, 7, &mut ids()).is_none());
    }

    #[test]
    fn a_clip_without_takes_is_not_extracted() {
        let mut p = song();
        let id = p.tracks[0].clips[0].id;
        assert!(extract(&mut p, id, 0, &mut ids()).is_none());
        assert_eq!(p.tracks.len(), song().tracks.len());
    }
}
