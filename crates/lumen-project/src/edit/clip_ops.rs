//! Clip commands of M7b slice E1: join, crop to a range and unloop. Pure
//! functions over the model; the document wraps each in one undo step.
//!
//! **Join** is seamless or it does not happen to audio. Two clips join when
//! cutting the joined clip back at the seam gives exactly the two clips
//! again (the proof is the check, not a list of rules), so joining what a
//! split made returns the original. MIDI clips also merge when they do not
//! meet: their shown notes, loops baked, go into one clip.

use std::sync::Arc;

use lumen_core::fade::Fade;
use lumen_core::time::{TempoMap, Ticks};

use crate::model::{Clip, ClipContent, ClipEvent, Id, Note, Project};

use super::{
    FOREVER_AFTER, FOREVER_BEFORE, clear, content_length, in_window, repeats, slice, tempo_of,
    to_arrangement,
};

/// The most clips an unloop makes from one clip.
pub const MOST_REPEATS: i64 = 1_024;

/// Source seconds two phases may differ by and still be one position: far
/// below a sample at any rate.
const PHASE_SLACK: f64 = 1e-7;

/// What a join did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Joined {
    /// The clips that now stand where the selection was: the joined clips
    /// and the selected ones that stayed as they were.
    pub clips: Vec<Id>,
    /// How many clips were taken into another.
    pub merged: usize,
    /// Whether audio clips were left apart because they do not continue one
    /// another in their source (that is Join into new file's work).
    pub apart: bool,
}

/// Joins the selected unlocked clips of each track that follow one another
/// (see the module): the first of a run keeps its id and the rest go.
pub fn join(project: &mut Project, ids: &[Id]) -> Joined {
    let tempo = tempo_of(project);
    let snapshot = project.clone();
    let mut out = Joined::default();
    for track in &mut project.tracks {
        let picked: Vec<&Arc<Clip>> = track
            .clips
            .iter()
            .filter(|c| ids.contains(&c.id) && !c.locked)
            .collect();
        if picked.len() < 2 {
            out.clips.extend(picked.iter().map(|c| c.id));
            continue;
        }
        let mut runs: Vec<Clip> = Vec::new();
        let mut merged = 0;
        for next in picked {
            let Some(current) = runs.last_mut() else {
                runs.push((**next).clone());
                continue;
            };
            if let Some(joined) = seamless(&snapshot, &tempo, current, next) {
                *current = joined;
                merged += 1;
            } else if let Some(joined) = merge_midi(current, next) {
                *current = joined;
                merged += 1;
            } else {
                out.apart |= matches!(current.content, ClipContent::Audio { .. })
                    && matches!(next.content, ClipContent::Audio { .. });
                runs.push((**next).clone());
            }
        }
        if merged == 0 {
            out.clips.extend(runs.iter().map(|c| c.id));
            continue;
        }
        out.merged += merged;
        out.clips.extend(runs.iter().map(|c| c.id));
        let t = Arc::make_mut(track);
        t.clips.retain(|c| !ids.contains(&c.id) || c.locked);
        t.clips.extend(runs.into_iter().map(Arc::new));
        t.clips.sort_by_key(|c| c.start);
    }
    out
}

/// `left` and `next` as one clip, if cutting that clip at their seam gives
/// them back exactly.
fn seamless(project: &Project, tempo: &TempoMap, left: &Clip, next: &Clip) -> Option<Clip> {
    if left.start + left.length != next.start || left.muted != next.muted {
        return None;
    }
    let mut joined = left.clone();
    joined.length = left.length + next.length;
    joined.fade_in = restored(left.fade_in, joined.length);
    joined.fade_out = restored(next.fade_out, joined.length - joined.fade_in.length);
    cuts_back(project, tempo, &joined, left, next).then_some(joined)
}

/// Whether cutting `joined` at the seam of `left` and `next` gives both.
fn cuts_back(project: &Project, tempo: &TempoMap, joined: &Clip, left: &Clip, next: &Clip) -> bool {
    let (a, b) = (left.start, next.start);
    let end = next.start + next.length;
    let same = |cut: Option<Clip>, want: &Clip| {
        cut.is_some_and(|mut cut| {
            let phase = (cut.audio_phase_seconds - want.audio_phase_seconds).abs();
            cut.id = want.id;
            cut.name = want.name.clone();
            cut.color = want.color;
            cut.audio_phase_seconds = want.audio_phase_seconds;
            phase <= PHASE_SLACK && cut == *want
        })
    };
    same(slice(project, tempo, joined, a, b), left)
        && same(slice(project, tempo, joined, b, end), next)
}

/// A fade cropped by a cut, as it was before it: whole again when the clip
/// has room for the original.
fn restored(fade: Fade, room: Ticks) -> Fade {
    if fade.span == Ticks::ZERO {
        return Fade {
            length: fade.length.min(room),
            ..fade
        };
    }
    let whole = fade.span - fade.offset;
    let length = whole.min(room);
    Fade {
        length,
        span: if fade.offset == Ticks::ZERO && length == fade.span {
            Ticks::ZERO
        } else {
            fade.span
        },
        ..fade
    }
}

/// Two MIDI clips as one new clip from the start of the first to the end of
/// the second: the notes and events each shows, loops baked.
fn merge_midi(first: &Clip, second: &Clip) -> Option<Clip> {
    if first.muted != second.muted || second.start < first.start + first.length {
        return None;
    }
    if !matches!(first.content, ClipContent::Midi { .. })
        || !matches!(second.content, ClipContent::Midi { .. })
    {
        return None;
    }
    let (mut notes, mut events) = baked(first);
    let (more_notes, more_events) = baked(second);
    let by = second.start - first.start;
    notes.extend(
        more_notes
            .into_iter()
            .map(|n| Note(n.0 + by, n.1, n.2, n.3, n.4, n.5)),
    );
    events.extend(
        more_events
            .into_iter()
            .map(|e| ClipEvent { at: e.at + by, ..e }),
    );
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    events.sort_by_key(|e| e.at);
    Some(Clip {
        length: second.start + second.length - first.start,
        fade_in: first.fade_in,
        crossfade: first.crossfade,
        fade_out: second.fade_out,
        loop_length: None,
        loop_phase: Ticks::ZERO,
        envelopes: Default::default(),
        content: ClipContent::Midi { notes, events },
        ..first.clone()
    })
}

/// The notes and events a MIDI clip plays, in ticks from its start: its
/// window, every repeat of a loop, none longer than its content or the clip.
fn baked(clip: &Clip) -> (Vec<Note>, Vec<ClipEvent>) {
    let ClipContent::Midi { notes, events } = &clip.content else {
        return (Vec::new(), Vec::new());
    };
    let content = content_length(clip);
    let mut out_notes = Vec::new();
    let mut out_events = Vec::new();
    for repeat in 0..repeats(clip) {
        for n in notes.iter().filter(|n| in_window(clip, n.start())) {
            let at = to_arrangement(clip, n.start(), repeat) - clip.start;
            if at < Ticks::ZERO || at >= clip.length {
                continue;
            }
            let length = n.length().min(content - n.start()).min(clip.length - at);
            out_notes.push(Note(at, length, n.2, n.3, n.4, n.5));
        }
        for e in events.iter().filter(|e| in_window(clip, e.at)) {
            let at = to_arrangement(clip, e.at, repeat) - clip.start;
            if at >= Ticks::ZERO && at < clip.length {
                out_events.push(ClipEvent { at, ..*e });
            }
        }
    }
    (out_notes, out_events)
}

/// Keeps `[a, b)` on `tracks` and removes everything else there (Crop to
/// selection). Locked clips stay whole.
pub fn crop(
    project: &mut Project,
    tracks: &[Id],
    a: Ticks,
    b: Ticks,
    fresh: &mut impl FnMut() -> Id,
) {
    if a >= b {
        return;
    }
    clear(project, tracks, FOREVER_BEFORE, a, fresh);
    clear(project, tracks, b, FOREVER_AFTER, fresh);
}

/// Replaces each selected unlocked looping clip with real clips, one for
/// each repeat it shows. The first keeps the clip's id. Returns the ids now
/// standing for the selection, and whether any clip was too long to unloop
/// (more than [`MOST_REPEATS`] repeats), which is left as it was.
pub fn unloop(
    project: &mut Project,
    ids: &[Id],
    fresh: &mut impl FnMut() -> Id,
) -> (Vec<Id>, bool) {
    let tempo = tempo_of(project);
    let snapshot = project.clone();
    let mut result = Vec::new();
    let mut too_long = false;
    for track in &mut project.tracks {
        let loops = |c: &Clip| {
            ids.contains(&c.id) && !c.locked && c.loop_length.is_some_and(|period| period.0 > 0)
        };
        if !track.clips.iter().any(|c| loops(c)) {
            result.extend(
                track
                    .clips
                    .iter()
                    .filter(|c| ids.contains(&c.id))
                    .map(|c| c.id),
            );
            continue;
        }
        let mut replaced = Vec::with_capacity(track.clips.len());
        for clip in &track.clips {
            if !ids.contains(&clip.id) {
                replaced.push(Arc::clone(clip));
                continue;
            }
            if !loops(clip) {
                result.push(clip.id);
                replaced.push(Arc::clone(clip));
                continue;
            }
            let Some(pieces) = unlooped(&snapshot, &tempo, clip, fresh) else {
                too_long = true;
                result.push(clip.id);
                replaced.push(Arc::clone(clip));
                continue;
            };
            for piece in pieces {
                result.push(piece.id);
                replaced.push(Arc::new(piece));
            }
        }
        Arc::make_mut(track).clips = replaced;
    }
    (result, too_long)
}

/// The clips a looping `clip` is, repeat by repeat; `None` for too many.
fn unlooped(
    project: &Project,
    tempo: &TempoMap,
    clip: &Clip,
    fresh: &mut impl FnMut() -> Id,
) -> Option<Vec<Clip>> {
    let period = clip.loop_length.filter(|p| p.0 > 0)?;
    let end = clip.start + clip.length;
    let origin = clip.start - clip.loop_phase;
    if i64::from(repeats(clip)) > MOST_REPEATS {
        return None;
    }
    let mut cuts = vec![clip.start];
    let mut at = origin + period;
    while at < end {
        if at > clip.start {
            cuts.push(at);
        }
        at = at + period;
    }
    cuts.push(end);
    let (baked_notes, baked_events) = baked(clip);
    let mut pieces = Vec::with_capacity(cuts.len() - 1);
    for (index, pair) in cuts.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let mut piece = slice(project, tempo, clip, from, to)?;
        if index > 0 {
            piece.id = fresh();
        }
        piece.loop_length = None;
        let in_loop = if index == 0 {
            clip.loop_phase
        } else {
            Ticks::ZERO
        };
        match &mut piece.content {
            ClipContent::Audio { .. } => {
                // The source time at the piece's start: the clip's own at its
                // loop origin, plus the way into the loop the piece begins.
                piece.audio_phase_seconds = clip.audio_phase_seconds + tempo.seconds_at(from)
                    - tempo.seconds_at(from - in_loop);
                piece.envelopes = clip.envelopes.clone();
                piece.envelopes.shift(Ticks::ZERO - in_loop);
            }
            ClipContent::Midi { .. } => {
                let (a, b) = (from - clip.start, to - clip.start);
                let notes = baked_notes
                    .iter()
                    .filter(|n| n.start() >= a && n.start() < b)
                    .map(|n| Note(n.0 - a, n.1.min(b - n.0), n.2, n.3, n.4, n.5))
                    .collect();
                let events = baked_events
                    .iter()
                    .filter(|e| e.at >= a && e.at < b)
                    .map(|e| ClipEvent { at: e.at - a, ..*e })
                    .collect();
                piece.content = ClipContent::Midi { notes, events };
            }
        }
        piece.loop_phase = Ticks::ZERO;
        pieces.push(piece);
    }
    Some(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::split;
    use crate::edit::tests::{clips as clip_summary, ids, q, song};
    use crate::model::{Color, EventKind, Id};

    fn clip_at(project: &Project, track: usize, index: usize) -> Clip {
        (*project.tracks[track].clips[index]).clone()
    }

    fn set(project: &mut Project, track: usize, index: usize, edit: impl FnOnce(&mut Clip)) {
        let t = Arc::make_mut(&mut project.tracks[track]);
        edit(Arc::make_mut(&mut t.clips[index]));
    }

    fn midi(project: &mut Project, notes: Vec<Note>, events: Vec<ClipEvent>) {
        set(project, 0, 0, |c| {
            c.content = ClipContent::Midi { notes, events }
        });
    }

    #[test]
    fn clips_of_different_colours_still_join_and_take_the_first_ones() {
        let mut project = song();
        let original = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[original.id], &[q(6)], &mut ids());
        set(&mut project, 0, 0, |c| c.color = Some(Color(0xAA3300)));
        set(&mut project, 0, 1, |c| c.color = Some(Color(0x0033AA)));
        let done = join(&mut project, &parts);
        assert_eq!((done.merged, done.apart), (1, false));
        assert_eq!(project.tracks[0].clips.len(), 1);
        assert_eq!(clip_at(&project, 0, 0).color, Some(Color(0xAA3300)));
    }

    #[test]
    fn splitting_then_joining_returns_the_original_audio_clip() {
        let mut project = song();
        let original = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[original.id], &[q(6), q(9)], &mut ids());
        assert_eq!(parts.len(), 3);
        let done = join(&mut project, &parts);
        assert_eq!(done.merged, 2);
        assert!(!done.apart);
        assert_eq!(done.clips, [original.id]);
        assert_eq!(clip_at(&project, 0, 0), original);
        assert_eq!(project.tracks[0].clips.len(), 1);
    }

    #[test]
    fn a_fade_longer_than_a_part_is_whole_again_after_the_join() {
        let mut project = song();
        set(&mut project, 0, 0, |c| {
            c.fade_in.length = q(5);
            c.fade_out.length = q(3);
        });
        let original = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[original.id], &[q(6)], &mut ids());
        assert_eq!(join(&mut project, &parts).merged, 1);
        assert_eq!(clip_at(&project, 0, 0), original);
    }

    #[test]
    fn a_looping_clip_cut_in_two_joins_back_too() {
        let mut project = song();
        set(&mut project, 0, 0, |c| {
            c.loop_length = Some(q(3));
            c.loop_phase = q(1);
        });
        let original = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[original.id], &[q(5), q(10)], &mut ids());
        assert_eq!(join(&mut project, &parts).merged, 2);
        assert_eq!(clip_at(&project, 0, 0), original);
    }

    #[test]
    fn audio_that_does_not_continue_in_its_source_stays_apart() {
        let mut project = song();
        let first = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[first.id], &[q(8)], &mut ids());
        // The second part plays other material.
        set(&mut project, 0, 1, |c| {
            if let ClipContent::Audio { offset, .. } = &mut c.content {
                *offset += 960;
            }
        });
        let before = project.clone();
        let done = join(&mut project, &parts);
        assert_eq!((done.merged, done.apart), (0, true));
        assert_eq!(project, before, "nothing changed");
        assert_eq!(done.clips, parts);
    }

    #[test]
    fn audio_with_a_gap_between_stays_apart_and_a_locked_clip_is_not_taken() {
        let mut project = song();
        let first = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[first.id], &[q(8)], &mut ids());
        set(&mut project, 0, 1, |c| c.start = c.start + q(1));
        let before = project.clone();
        assert_eq!(join(&mut project, &parts).merged, 0);
        assert_eq!(project, before);

        let mut project = song();
        let parts = split::clips(&mut project, &[first.id], &[q(8)], &mut ids());
        set(&mut project, 0, 1, |c| c.locked = true);
        let before = project.clone();
        assert_eq!(join(&mut project, &parts).merged, 0);
        assert_eq!(project, before);
    }

    #[test]
    fn midi_cut_in_two_joins_back_with_its_hidden_notes() {
        let mut project = song();
        midi(
            &mut project,
            vec![
                Note::new(q(0), q(3), 60, 90),
                Note::new(q(3), q(3), 62, 80),
                Note::new(q(7), q(1), 64, 70),
            ],
            vec![ClipEvent {
                at: q(3),
                kind: EventKind::Controller {
                    number: 1,
                    value: 64,
                },
            }],
        );
        let original = clip_at(&project, 0, 0);
        let parts = split::clips(&mut project, &[original.id], &[q(6)], &mut ids());
        assert_eq!(join(&mut project, &parts).merged, 1);
        assert_eq!(clip_at(&project, 0, 0), original);
    }

    #[test]
    fn midi_clips_with_a_gap_merge_their_shown_notes() {
        let mut project = song();
        midi(
            &mut project,
            vec![
                Note::new(q(1), q(1), 60, 90),
                Note::new(q(20), q(1), 61, 90),
            ],
            vec![],
        );
        // A second clip after a gap, at 14 quarters.
        let mut second = clip_at(&project, 0, 0);
        second.id = Id(77);
        second.start = q(14);
        second.length = q(4);
        second.content = ClipContent::Midi {
            notes: vec![Note::new(q(2), q(1), 72, 100)],
            events: vec![ClipEvent {
                at: q(1),
                kind: EventKind::PitchBend { value: 100 },
            }],
        };
        Arc::make_mut(&mut project.tracks[0])
            .clips
            .push(Arc::new(second));
        let done = join(&mut project, &[Id(10), Id(77)]);
        assert_eq!((done.merged, done.clips.as_slice()), (1, &[Id(10)][..]));
        let clip = clip_at(&project, 0, 0);
        assert_eq!((clip.start, clip.length), (q(4), q(14)));
        let ClipContent::Midi { notes, events } = &clip.content else {
            panic!("MIDI stays MIDI");
        };
        // The note hidden past the first clip's end is not brought in.
        assert_eq!(
            notes
                .iter()
                .map(|n| (n.start(), n.pitch()))
                .collect::<Vec<_>>(),
            [(q(1), 60), (q(12), 72)]
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].at, q(11));
        assert_eq!(project.tracks[0].clips.len(), 1);
    }

    #[test]
    fn crop_keeps_only_the_range_on_its_tracks() {
        let mut project = song();
        crop(&mut project, &[Id(1)], q(6), q(10), &mut ids());
        assert_eq!(clip_summary(&project, 0), [(10, 6, 4, 1_000)]);
        assert_eq!(
            clip_summary(&project, 1),
            [(20, 0, 8, 1_000)],
            "other tracks keep theirs"
        );
    }

    #[test]
    fn unlooping_audio_makes_a_clip_for_each_repeat_with_the_source_in_place() {
        let mut project = song();
        set(&mut project, 0, 0, |c| {
            c.loop_length = Some(q(3));
            c.loop_phase = q(1);
            c.audio_phase_seconds = 0.5;
        });
        let original = clip_at(&project, 0, 0);
        let (made, too_long) = unloop(&mut project, &[original.id], &mut ids());
        assert!(!too_long);
        let t = &project.tracks[0];
        assert_eq!(made.len(), 3, "[4,6) then [6,9) and [9,12)");
        let starts: Vec<_> = t.clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, [q(4), q(6), q(9)]);
        assert!(
            t.clips
                .iter()
                .all(|c| c.loop_length.is_none() && c.loop_phase == Ticks::ZERO)
        );
        assert_eq!(t.clips[0].id, original.id);
        // 120 BPM: the first part begins a quarter (0.5 s) into the loop.
        assert!((t.clips[0].audio_phase_seconds - 1.0).abs() < 1e-9);
        assert!((t.clips[1].audio_phase_seconds - 0.5).abs() < 1e-9);
        assert_eq!(t.clips[0].fade_in, original.fade_in);
        assert!(t.clips[1].fade_in.is_none());
        assert_eq!(t.clips.last().unwrap().fade_out, original.fade_out);
    }

    #[test]
    fn unlooping_midi_bakes_the_repeats_cut_at_the_loop_and_the_clip() {
        let mut project = song();
        midi(
            &mut project,
            vec![Note::new(q(0), q(2), 60, 90), Note::new(q(1), q(5), 62, 80)],
            vec![],
        );
        set(&mut project, 0, 0, |c| {
            c.loop_length = Some(q(3));
            c.length = q(7);
        });
        let (made, _) = unloop(&mut project, &[Id(10)], &mut ids());
        assert_eq!(made.len(), 3);
        let shown: Vec<Vec<(i64, i64, u8)>> = project.tracks[0]
            .clips
            .iter()
            .map(|c| {
                let ClipContent::Midi { notes, .. } = &c.content else {
                    panic!("MIDI stays MIDI");
                };
                notes
                    .iter()
                    .map(|n| (n.start().0 / q(1).0, n.length().0 / q(1).0, n.pitch()))
                    .collect()
            })
            .collect();
        // The long note stops at the loop end (two quarters left) each repeat.
        assert_eq!(shown[0], [(0, 2, 60), (1, 2, 62)]);
        assert_eq!(shown[1], [(0, 2, 60), (1, 2, 62)]);
        assert_eq!(shown[2], [(0, 1, 60)]);
        assert_eq!(project.tracks[0].clips[2].length, q(1));
    }

    #[test]
    fn a_clip_that_does_not_loop_or_is_locked_is_left_alone_by_unloop() {
        let mut project = song();
        set(&mut project, 0, 0, |c| {
            c.locked = true;
            c.loop_length = Some(q(2));
        });
        let before = project.clone();
        let (made, _) = unloop(&mut project, &[Id(10), Id(20)], &mut ids());
        assert_eq!(project, before);
        assert_eq!(made, [Id(10), Id(20)]);
    }
}
