//! Editing time on the arrangement (M3 slice 6b): taking the part of a
//! clip between two times, clearing a span, deleting or inserting time
//! across the whole song; the clipboard is `clipboard.rs`. Pure functions
//! over the
//! model, so every edit is tested here and the app wraps each in one undo
//! step.
//!
//! A clip cut at a time keeps its audio in place: an unlooped clip retains its
//! integer media offset and advances `audio_phase_seconds`; looped clips
//! retain their source origin and advance `loop_phase`. MIDI notes are cut
//! at the edges, or stay loop-local when the clip repeats.

use std::sync::Arc;

use lumen_core::time::{TempoMap, Ticks};

#[cfg(test)]
use crate::model::EventKind;
use crate::model::{Clip, ClipContent, ClipEvent, Id, Note, Project};

pub mod automation;
pub mod carry;
mod clipboard;
pub mod events;
pub mod maps;
mod note_selection;
mod note_time;
pub mod note_tools;
pub mod notes;
pub mod quantize;
pub mod record;
pub mod seams;
pub mod split;
pub mod velocity;
mod windows;

pub use clipboard::{
    Clipboard, LanePart, copy_clips, copy_range, landing_tracks, paste, paste_lanes,
};
pub use note_selection::NoteSelection;
pub use note_time::{
    content_length, in_window, repeats, shown, shown_notes, to_arrangement, to_clip_time,
};
pub use windows::slice;
use windows::{crop_fade_end, crop_fade_start};

/// Times before and after any clip, for "everything before" and "everything
/// after" (far from overflow).
const FOREVER_BEFORE: Ticks = Ticks(i64::MIN / 4);
const FOREVER_AFTER: Ticks = Ticks(i64::MAX / 4);

/// The project's tempo, or 120 BPM if it is invalid (validation keeps it
/// valid; this only avoids a panic).
pub fn tempo_of(project: &Project) -> TempoMap {
    TempoMap::new(project.tempo.clone())
        .unwrap_or_else(|_| TempoMap::constant(120.0).expect("120 BPM is a valid tempo"))
}

/// Empties `[a, b)` on `tracks`: clips inside go, clips crossing an edge
/// are cut back to it, and a clip spanning the whole span is split in two
/// (the right part gets an id from `fresh`). Locked clips stay whole.
pub fn clear(
    project: &mut Project,
    tracks: &[Id],
    a: Ticks,
    b: Ticks,
    fresh: &mut impl FnMut() -> Id,
) {
    clear_except(project, tracks, a, b, &[], fresh);
}

/// [`clear`], leaving the clips `keep` alone.
pub fn clear_except(
    project: &mut Project,
    tracks: &[Id],
    a: Ticks,
    b: Ticks,
    keep: &[Id],
    fresh: &mut impl FnMut() -> Id,
) {
    if a >= b {
        return;
    }
    let tempo = tempo_of(project);
    let snapshot = project.clone();
    for track in &mut project.tracks {
        if !tracks.contains(&track.id) {
            continue;
        }
        let touched = |c: &Arc<Clip>| {
            !c.locked && !keep.contains(&c.id) && c.start < b && c.start + c.length > a
        };
        if !track.clips.iter().any(touched) {
            continue;
        }
        let mut kept = Vec::with_capacity(track.clips.len() + 1);
        for clip in &track.clips {
            if !touched(clip) {
                kept.push(Arc::clone(clip));
                continue;
            }
            if let Some(left) = slice(&snapshot, &tempo, clip, FOREVER_BEFORE, a) {
                kept.push(Arc::new(left));
            }
            if let Some(mut right) = slice(&snapshot, &tempo, clip, b, FOREVER_AFTER) {
                if clip.start < a {
                    right.id = fresh();
                }
                kept.push(Arc::new(right));
            }
        }
        kept.sort_by_key(|c| c.start);
        Arc::make_mut(track).clips = kept;
    }
}

/// The rule for clips put down on others (slice 6c, owner): the
/// clips `placed` win, and whatever they cover on their own track is cut
/// away, as [`clear`] does.
pub fn overwrite(project: &mut Project, placed: &[Id], fresh: &mut impl FnMut() -> Id) {
    let spans: Vec<(Id, Ticks, Ticks)> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(move |c| (t.id, &**c)))
        .filter(|(_, c)| placed.contains(&c.id))
        .map(|(track, c)| (track, c.start, c.start + c.length))
        .collect();
    for (track, a, b) in spans {
        clear_except(project, &[track], a, b, placed, fresh);
    }
}

/// Which edge of a clip a trim moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// The shortest a trim leaves a clip: a 1/64 note.
pub const MIN_LENGTH: Ticks = Ticks(lumen_core::time::PPQ / 16);

/// An audio clip's file: its rate, its frames and the clip's offset into it.
fn audio_of(project: &Project, clip: &Clip) -> Option<(u32, i64, i64)> {
    let ClipContent::Audio { media, offset, .. } = &clip.content else {
        return None;
    };
    let file = project.media.iter().find(|m| m.id == *media)?;
    Some((file.sample_rate.max(1), file.frames as i64, *offset))
}

/// `clip` with `edge` moved to `to` (snapped by the caller). The audio
/// stays in place, so a trim can reveal no more than the file holds: the
/// start stops where the file starts, and the end where it ends, unless
/// the clip loops. With `looping` (Shift on the end edge), a clip that does
/// not loop yet starts repeating its current length (loop-extend).
pub fn trim(
    project: &Project,
    tempo: &TempoMap,
    clip: &Clip,
    edge: Edge,
    to: Ticks,
    looping: bool,
) -> Clip {
    let end = clip.start + clip.length;
    let mut c = clip.clone();
    let audio = audio_of(project, clip);
    match edge {
        Edge::Start => {
            let mut start = to.min(end - MIN_LENGTH).max(Ticks::ZERO);
            if clip.loop_length.is_some_and(|period| period.0 > 0) {
                let origin = (clip.start - clip.loop_phase).max(Ticks::ZERO);
                start = start.max(origin);
            }
            if let Some((rate, _, offset)) = audio
                && clip.loop_length.is_none()
            {
                // Not before the file's first frame. `audio_phase_seconds`
                // tracks a fractional source position from the integer file
                // offset, so it participates in the available source time.
                let earliest = tempo.ticks_at_seconds(
                    tempo.seconds_at(clip.start)
                        - (offset as f64 / f64::from(rate) + clip.audio_phase_seconds),
                );
                start = start.max(earliest);
            }
            let moved = start - clip.start;
            c.start = start;
            c.length = end - start;
            if clip.loop_length.is_some_and(|period| period.0 > 0) {
                c.loop_phase = Ticks(clip.loop_phase.0.saturating_add(moved.0));
            } else {
                c.envelopes.shift(Ticks::ZERO - moved);
                match &mut c.content {
                    ClipContent::Audio { .. } => {
                        let (source_rate, _, offset) =
                            audio.unwrap_or((project.sample_rate.max(1), 0, 0));
                        c.audio_phase_seconds = (c.audio_phase_seconds + tempo.seconds_at(start)
                            - tempo.seconds_at(clip.start))
                        .max(-(offset as f64) / f64::from(source_rate));
                    }
                    ClipContent::Midi { notes, events } => {
                        shift_notes(notes, moved);
                        shift_events(events, moved);
                    }
                }
            }
            if moved > Ticks::ZERO {
                c.fade_in = crop_fade_start(c.fade_in, moved);
            }
            let out_length = c.fade_out.length.min(c.length - c.fade_in.length);
            if out_length < c.fade_out.length {
                c.fade_out.span = c.fade_out.span.max(c.fade_out.length);
                c.fade_out.length = out_length;
            }
        }
        Edge::End => {
            let mut new_end = to.max(clip.start + MIN_LENGTH);
            if looping && c.loop_length.is_none() {
                c.loop_length = Some(clip.length);
            }
            if c.loop_length.is_none()
                && let Some((rate, frames, offset)) = audio
            {
                // Not past the file's last frame.
                let source_at_start = offset as f64 / f64::from(rate) + clip.audio_phase_seconds;
                let latest = tempo.ticks_at_seconds(
                    tempo.seconds_at(clip.start)
                        + (frames as f64 / f64::from(rate) - source_at_start),
                );
                new_end = new_end.min(latest.max(clip.start + MIN_LENGTH));
            }
            // A MIDI clip's notes past its new end are kept, hidden.
            c.length = new_end - clip.start;
            if new_end < end {
                c.fade_out = crop_fade_end(c.fade_out, end - new_end);
            }
            let in_length = c.fade_in.length.min(c.length);
            if in_length < c.fade_in.length {
                c.fade_in.span = c.fade_in.span.max(c.fade_in.length);
                c.fade_in.length = in_length;
            }
        }
    }
    let visible_length = content_length(&c).min(c.length);
    if let ClipContent::Midi { notes, .. } = &mut c.content {
        for note in notes {
            if note.start() >= Ticks::ZERO && note.start() < visible_length {
                *note = note.with_carries_on(true);
            }
        }
    }
    c.fade_in.length = c.fade_in.length.min(c.length);
    c.fade_out.length = c.fade_out.length.min(c.length - c.fade_in.length);
    c
}

/// Notes `by` earlier in their clip (its start moved later by `by`). None is
/// dropped or shortened: those now outside the clip's window are hidden
/// (`note_time::in_window`), and come back when an edge is dragged back.
fn shift_notes(notes: &mut [Note], by: Ticks) {
    for n in notes {
        n.0 = n.0 - by;
    }
}

/// Events `by` earlier in their clip, kept as notes are.
fn shift_events(events: &mut [ClipEvent], by: Ticks) {
    for e in events {
        e.at = e.at - by;
    }
}

/// `clip` with its content slid `by` (later when positive) inside it,
/// the clip itself staying put (Ctrl+Alt+drag). Audio slides no further
/// than the file allows.
pub fn slip(project: &Project, tempo: &TempoMap, clip: &Clip, by: Ticks) -> Clip {
    let mut c = clip.clone();
    match &mut c.content {
        ClipContent::Audio { offset, .. } => {
            let Some((rate, frames, _)) = audio_of(project, clip) else {
                return c;
            };
            let shown =
                tempo.sample_at(clip.start + clip.length, rate) - tempo.sample_at(clip.start, rate);
            let moved = tempo.sample_at(clip.start + by, rate) - tempo.sample_at(clip.start, rate);
            let phase_frames = clip.audio_phase_seconds * f64::from(rate);
            let first = ((-phase_frames).ceil() as i64).max(0);
            let last = if clip.loop_length.is_some() {
                (frames as f64 - 1.0 - phase_frames).floor() as i64
            } else {
                (frames as f64 - shown as f64 - phase_frames).floor() as i64
            };
            let target = *offset - moved;
            if target < 0 && phase_frames > 0.0 {
                // A trim leaves its advance in the phase, not the offset
                // (which cannot go negative): sliding back takes it from
                // there, and stops where the file starts.
                *offset = 0;
                c.audio_phase_seconds = (phase_frames + target as f64).max(0.0) / f64::from(rate);
            } else {
                *offset = target.clamp(first, last.max(first));
            }
        }
        ClipContent::Midi { notes, events } => {
            shift_notes(notes, Ticks::ZERO - by);
            shift_events(events, Ticks::ZERO - by);
        }
    }
    c
}

/// Where a time lands when `[a, b)` is deleted: before it stays, inside it
/// goes to `a`, after it moves back by its length.
fn closed(t: Ticks, a: Ticks, b: Ticks) -> Ticks {
    if t <= a {
        t
    } else if t < b {
        a
    } else {
        t - (b - a)
    }
}

/// Deletes `[a, b)` from the whole song (Delete Time): every
/// track is cleared there and everything after moves back; locators inside
/// go, those after move; the loop closes up (unless that would empty it).
pub fn delete_time(project: &mut Project, a: Ticks, b: Ticks, fresh: &mut impl FnMut() -> Id) {
    if a >= b {
        return;
    }
    let all: Vec<Id> = project.tracks.iter().map(|t| t.id).collect();
    clear(project, &all, a, b, fresh);
    for track in &mut project.tracks {
        if track.clips.iter().any(|c| c.start >= b && !c.locked) {
            for clip in &mut Arc::make_mut(track).clips {
                if clip.start >= b && !clip.locked {
                    Arc::make_mut(clip).start = clip.start - (b - a);
                }
            }
        }
    }
    project.markers.retain(|m| m.at <= a || m.at >= b);
    for marker in &mut project.markers {
        marker.at = closed(marker.at, a, b);
    }
    let t = &mut project.transport;
    let (start, end) = (closed(t.loop_start, a, b), closed(t.loop_end, a, b));
    if start < end {
        (t.loop_start, t.loop_end) = (start, end);
    }
    sort_clips(project);
}

/// Inserts `length` of silence at `at` across the whole song (Insert
/// Time): clips crossing it split, and everything after moves on,
/// locators and the loop included.
pub fn insert_time(
    project: &mut Project,
    at: Ticks,
    length: Ticks,
    fresh: &mut impl FnMut() -> Id,
) {
    if length <= Ticks::ZERO {
        return;
    }
    let tempo = tempo_of(project);
    let snapshot = project.clone();
    for track in &mut project.tracks {
        let crossing = |c: &Arc<Clip>| !c.locked && c.start < at && c.start + c.length > at;
        let after = |c: &Arc<Clip>| !c.locked && c.start >= at;
        if !track.clips.iter().any(|c| crossing(c) || after(c)) {
            continue;
        }
        let mut kept = Vec::with_capacity(track.clips.len() + 1);
        for clip in &track.clips {
            if crossing(clip) {
                if let Some(left) = slice(&snapshot, &tempo, clip, FOREVER_BEFORE, at) {
                    kept.push(Arc::new(left));
                }
                if let Some(mut right) = slice(&snapshot, &tempo, clip, at, FOREVER_AFTER) {
                    right.id = fresh();
                    right.start = right.start + length;
                    kept.push(Arc::new(right));
                }
            } else if after(clip) {
                let mut moved = (**clip).clone();
                moved.start = moved.start + length;
                kept.push(Arc::new(moved));
            } else {
                kept.push(Arc::clone(clip));
            }
        }
        Arc::make_mut(track).clips = kept;
    }
    let later = |t: Ticks| if t >= at { t + length } else { t };
    for marker in &mut project.markers {
        marker.at = later(marker.at);
    }
    let t = &mut project.transport;
    t.loop_start = later(t.loop_start);
    t.loop_end = later(t.loop_end);
    sort_clips(project);
}

fn sort_clips(project: &mut Project) {
    for track in &mut project.tracks {
        if !track.clips.is_sorted_by_key(|c| c.start) {
            Arc::make_mut(track).clips.sort_by_key(|c| c.start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Media, Track, TrackKind};
    use lumen_core::fade::{Fade, FadeShape};

    const Q: i64 = lumen_core::time::PPQ;

    pub(super) fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    /// Three audio tracks; track 0 has a clip from 4 to 12, track 1 one
    /// from 0 to 8, track 2 is empty. 120 BPM, files at 48 kHz.
    pub(super) fn song() -> Project {
        let mut project = crate::testing::small_project(1);
        project.tempo = vec![lumen_core::time::TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
            ramp: false,
        }];
        project.media = vec![Media {
            id: Id(900),
            path: "a.wav".into(),
            original_path: None,
            frames: 480_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 0,
        }];
        let clip = |id: u64, start: i64, length: i64| {
            Arc::new(Clip {
                id: Id(id),
                name: format!("c{id}"),
                start: q(start),
                length: q(length),
                muted: false,
                color: None,
                locked: false,
                fade_in: Fade {
                    length: q(1),
                    curve: FadeShape::Slow.curve(),
                    ..Fade::default()
                },
                fade_out: Fade {
                    length: q(1),
                    curve: FadeShape::Fast.curve(),
                    ..Fade::default()
                },
                loop_length: None,
                loop_phase: Ticks::ZERO,
                audio_phase_seconds: 0.0,
                envelopes: Default::default(),
                content: ClipContent::Audio {
                    media: Id(900),
                    offset: 1_000,
                    gain_db: 0.0,
                },
            })
        };
        let track = |id: u64, clips: Vec<Arc<Clip>>| {
            Arc::new(Track {
                clips,
                ..Track::new(Id(id), format!("t{id}"), TrackKind::Audio, Color(0))
            })
        };
        project.tracks = vec![
            track(1, vec![clip(10, 4, 8)]),
            track(2, vec![clip(20, 0, 8)]),
            track(3, vec![]),
        ];
        project.markers.clear();
        project
    }

    pub(super) fn ids() -> impl FnMut() -> Id {
        let mut next = 1_000;
        move || {
            next += 1;
            Id(next)
        }
    }

    /// `(id, start, length, offset)` per clip on track `i`, in quarters.
    pub(super) fn clips(p: &Project, i: usize) -> Vec<(u64, i64, i64, i64)> {
        p.tracks[i]
            .clips
            .iter()
            .map(|c| {
                let offset = match c.content {
                    ClipContent::Audio { offset, .. } => offset,
                    ClipContent::Midi { .. } => 0,
                };
                (c.id.0, c.start.0 / Q, c.length.0 / Q, offset)
            })
            .collect()
    }

    #[test]
    fn a_cut_or_a_start_trim_keeps_the_envelope_where_it_was_on_the_timeline() {
        use crate::envelope::Kind;
        use lumen_core::automation::{Lane, Point};
        let p = song();
        let tempo = tempo_of(&p);
        // The clip runs from quarter 4; its envelope has points at 1 and 5
        // quarters in, so at timeline quarters 5 and 9.
        let mut clip = (*p.tracks[0].clips[0]).clone();
        clip.envelopes.set(
            Kind::Gain,
            Lane::new(vec![Point::new(q(1), -6.0), Point::new(q(5), 0.0)]).unwrap(),
        );
        let places = |c: &Clip| -> Vec<Ticks> {
            c.envelopes
                .gain
                .as_ref()
                .map_or_else(Vec::new, |l| l.points().iter().map(|x| x.at).collect())
        };
        // A part cut at quarter 6 starts there: the first point is now one
        // quarter before its start, hidden, and the second is 3 quarters in.
        let right = slice(&p, &tempo, &clip, q(6), q(100)).unwrap();
        assert_eq!(places(&right), vec![q(-1), q(3)]);
        // The part before the cut starts where the clip did.
        let left = slice(&p, &tempo, &clip, q(0), q(6)).unwrap();
        assert_eq!(places(&left), vec![q(1), q(5)]);
        // The two points have the values they had.
        let values: Vec<f32> = right
            .envelopes
            .gain
            .as_ref()
            .unwrap()
            .points()
            .iter()
            .map(|x| x.value)
            .collect();
        assert_eq!(values, vec![-6.0, 0.0]);
        // Trimming the start to quarter 7 moves it three quarters.
        let trimmed = trim(&p, &tempo, &clip, Edge::Start, q(7), false);
        assert_eq!(places(&trimmed), vec![q(-2), q(2)]);
        // And dragging it back out brings the points back.
        let again = trim(&p, &tempo, &trimmed, Edge::Start, q(4), false);
        assert_eq!(places(&again), vec![q(1), q(5)]);
        // The end, a slip and a move leave the points alone.
        assert_eq!(
            places(&trim(&p, &tempo, &clip, Edge::End, q(8), false)),
            vec![q(1), q(5)]
        );
        assert_eq!(places(&slip(&p, &tempo, &clip, q(1))), vec![q(1), q(5)]);
    }

    #[test]
    fn a_slice_keeps_its_audio_in_place_and_only_its_own_edges_fades() {
        let p = song();
        let tempo = tempo_of(&p);
        let clip = &p.tracks[0].clips[0];
        // From quarter 6: two quarters in, a second at 120 BPM.
        let part = slice(&p, &tempo, clip, q(6), q(100)).unwrap();
        assert_eq!((part.start, part.length), (q(6), q(6)));
        assert!(matches!(
            part.content,
            ClipContent::Audio { offset: 1_000, .. }
        ));
        assert_eq!(part.audio_phase_seconds, 1.0);
        assert!(part.fade_in.is_none(), "cut there");
        assert_eq!(part.fade_out.length, q(1), "kept its own end");
        assert!(slice(&p, &tempo, clip, q(12), q(20)).is_none());
    }

    #[test]
    fn a_midi_slice_is_a_window_that_keeps_every_note() {
        // The clip window (format v15): the slice from 4 to 6 shows the note
        // at 5 and keeps the others, hidden, so nothing is lost.
        let p = song();
        let mut clip = (*p.tracks[1].clips[0]).clone();
        clip.content = ClipContent::Midi {
            notes: vec![
                Note::new(q(1), q(1), 60, 100),
                Note::new(q(5), q(2), 62, 90),
            ],
            events: vec![
                ClipEvent {
                    at: q(1),
                    kind: EventKind::Pressure { value: 10 },
                },
                ClipEvent {
                    at: q(5),
                    kind: EventKind::Pressure { value: 20 },
                },
            ],
        };
        let part = slice(&p, &tempo_of(&p), &clip, q(4), q(6)).unwrap();
        assert_eq!(
            part.content,
            ClipContent::Midi {
                notes: vec![
                    Note::new(q(-3), q(1), 60, 100),
                    Note::new(q(1), q(2), 62, 90),
                ],
                events: vec![
                    ClipEvent {
                        at: q(-3),
                        kind: EventKind::Pressure { value: 10 },
                    },
                    ClipEvent {
                        at: q(1),
                        kind: EventKind::Pressure { value: 20 },
                    },
                ],
            }
        );
        assert_eq!(shown_notes(&part), vec![1], "only the note at 5 shows");
        // Dragging its start back to the original's shows the first again,
        // where it was.
        let back = trim(&p, &tempo_of(&p), &part, Edge::Start, clip.start, false);
        let ClipContent::Midi { notes, .. } = &back.content else {
            unreachable!()
        };
        assert_eq!(notes[0], Note::new(q(1), q(1), 60, 100));
        assert_eq!(shown_notes(&back), vec![0, 1]);
    }

    #[test]
    fn slicing_a_revealed_hidden_capture_makes_a_genuine_seam_copy() {
        let p = song();
        let mut clip = (*p.tracks[1].clips[0]).clone();
        clip.start = q(2);
        clip.length = q(2);
        clip.content = ClipContent::Midi {
            notes: vec![Note::new(q(-1), q(4), 60, 100).with_carries_on(false)],
            events: Vec::new(),
        };
        assert!(shown_notes(&clip).is_empty());
        let recovered = trim(&p, &tempo_of(&p), &clip, Edge::Start, q(1), false);
        assert_eq!(shown_notes(&recovered), vec![0]);
        // Slicing the recovered note creates a legitimate hidden copy on the
        // right, which must carry through the new seam.
        let left = slice(&p, &tempo_of(&p), &recovered, q(1), q(2)).unwrap();
        let right = slice(&p, &tempo_of(&p), &recovered, q(2), q(5)).unwrap();
        let ClipContent::Midi {
            notes: left_notes, ..
        } = &left.content
        else {
            unreachable!()
        };
        let ClipContent::Midi {
            notes: right_notes, ..
        } = &right.content
        else {
            unreachable!()
        };
        assert_eq!(left_notes[0].start(), Ticks::ZERO);
        assert_eq!(right_notes[0].start(), q(-1));
        assert!(right_notes[0].carries_on());
        let left = Arc::new(left);
        let right = Arc::new(right);
        let t = Track {
            clips: vec![left, right],
            ..Track::new(Id(90), "Recovered", TrackKind::Midi, Color(0))
        };
        assert_eq!(crate::edit::seams::carried(&t), vec![Vec::new(), vec![0]]);
    }

    #[test]
    fn trimming_a_midi_clip_hides_notes_and_dragging_back_shows_them() {
        let p = song();
        let mut clip = (*p.tracks[1].clips[0]).clone();
        let notes = vec![
            Note::new(q(1), q(1), 60, 100),
            Note::new(q(6), q(1), 62, 90),
        ];
        clip.content = ClipContent::Midi {
            notes: notes.clone(),
            events: Vec::new(),
        };
        let short = trim(
            &p,
            &tempo_of(&p),
            &clip,
            Edge::End,
            clip.start + q(4),
            false,
        );
        assert_eq!(shown_notes(&short), vec![0], "the note at 6 is hidden");
        let long = trim(
            &p,
            &tempo_of(&p),
            &short,
            Edge::End,
            clip.start + clip.length,
            false,
        );
        assert_eq!(long.content, clip.content, "and comes back intact");
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn a_dropped_clip_cuts_away_what_it_covers() {
        let mut p = song();
        // Track 1's clip (0 to 8) dropped over track 0's (4 to 12) at 6.
        let mut moved = (*p.tracks[1].clips[0]).clone();
        moved.start = q(6);
        moved.length = q(2);
        Arc::make_mut(&mut p.tracks[0]).clips.push(Arc::new(moved));
        overwrite(&mut p, &[Id(20)], &mut ids());
        let left: Vec<_> = clips(&p, 0).iter().map(|c| (c.0, c.1, c.2)).collect();
        assert_eq!(left, [(10, 4, 2), (20, 6, 2), (1001, 8, 4)]);
    }

    #[test]
    fn trims_stop_at_the_file_and_shift_the_audio() {
        let p = song();
        let tempo = tempo_of(&p);
        let clip = &p.tracks[0].clips[0]; // 4 to 12, offset 1,000 of 480,000
        // The start back by 2 quarters (1 s): the offset is only 1,000
        // frames, so it stops 1,000 frames before quarter 4.
        let early = trim(&p, &tempo, clip, Edge::Start, q(2), false);
        assert!(matches!(
            early.content,
            ClipContent::Audio { offset: 1_000, .. }
        ));
        assert!(
            early.start > q(3) && early.start < q(4),
            "{:?}",
            early.start
        );
        // The start forward by 2 quarters: the audio stays in place.
        let late = trim(&p, &tempo, clip, Edge::Start, q(6), false);
        assert_eq!((late.start, late.length), (q(6), q(6)));
        assert!(matches!(
            late.content,
            ClipContent::Audio { offset: 1_000, .. }
        ));
        assert_eq!(late.audio_phase_seconds, 1.0);
        // The end past the file (10 s = 20 quarters from 4) stops there.
        let long = trim(&p, &tempo, clip, Edge::End, q(100), false);
        assert!(
            long.length <= q(20) && long.length > q(19),
            "{:?}",
            long.length
        );
        // With Shift it loops its current 8 quarters instead.
        let looped = trim(&p, &tempo, clip, Edge::End, q(100), true);
        assert_eq!((looped.length, looped.loop_length), (q(96), Some(q(8))));
        // Never shorter than a 1/64 note.
        assert_eq!(
            trim(&p, &tempo, clip, Edge::End, q(0), false).length,
            MIN_LENGTH
        );
    }

    #[test]
    fn trim_to_native_file_origin_never_leaves_a_negative_audio_phase() {
        let mut p = song();
        p.tempo[0].bpm = 123.0;
        p.media[0].frames = 441_000;
        p.media[0].sample_rate = 44_100;
        Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).content = ClipContent::Audio {
            media: Id(900),
            offset: 37,
            gain_db: 0.0,
        };
        let tempo = tempo_of(&p);
        let clip = &p.tracks[0].clips[0];
        let earliest = tempo.ticks_at_seconds(tempo.seconds_at(clip.start) - 37.0 / 44_100.0);
        let trimmed = trim(&p, &tempo, clip, Edge::Start, earliest, false);
        assert!(trimmed.audio_phase_seconds >= -37.0 / 44_100.0);
        Arc::make_mut(&mut p.tracks[0]).clips[0] = Arc::new(trimmed);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn slipping_moves_the_audio_not_the_clip_within_the_file() {
        let p = song();
        let tempo = tempo_of(&p);
        let clip = &p.tracks[0].clips[0]; // offset 1,000
        let earlier = slip(&p, &tempo, clip, Ticks(-q(2).0));
        assert_eq!(
            earlier.content,
            ClipContent::Audio {
                media: Id(900),
                offset: 49_000,
                gain_db: 0.0
            }
        );
        assert_eq!((earlier.start, earlier.length), (clip.start, clip.length));
        let back = slip(&p, &tempo, clip, q(2));
        assert_eq!(
            back.content,
            ClipContent::Audio {
                media: Id(900),
                offset: 0,
                gain_db: 0.0
            },
            "stops at the file's start"
        );
    }

    #[test]
    fn slipping_back_recovers_audio_a_trim_advanced_past() {
        let p = song();
        let tempo = tempo_of(&p);
        let mut clip = (*p.tracks[0].clips[0]).clone();
        clip.content = ClipContent::Audio {
            media: Id(900),
            offset: 0,
            gain_db: 0.0,
        };
        clip.audio_phase_seconds = 0.5;
        let back = slip(&p, &tempo, &clip, q(1));
        let ClipContent::Audio { offset, .. } = back.content else {
            panic!("audio")
        };
        let frames = 48_000.0;
        let before = 0.5 * frames;
        let after = offset as f64 + back.audio_phase_seconds * frames;
        assert_eq!(offset, 0);
        assert!(after < before, "the audio moved later: {after} < {before}");
        let all = slip(&p, &tempo, &clip, q(100));
        assert_eq!(all.audio_phase_seconds, 0.0, "stops at the file's start");
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn clearing_trims_crossing_clips_and_splits_one_spanning_the_span() {
        let mut p = song();
        clear(&mut p, &[Id(1), Id(2)], q(6), q(8), &mut ids());
        assert_eq!(clips(&p, 0), [(10, 4, 2, 1_000), (1001, 8, 4, 1_000)]);
        assert_eq!(clips(&p, 1), [(20, 0, 6, 1_000)]);
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn deleting_time_closes_the_gap_everywhere() {
        let mut p = song();
        p.markers = vec![
            crate::model::Marker {
                id: Id(50),
                at: q(7),
                name: "in".into(),
            },
            crate::model::Marker {
                id: Id(51),
                at: q(16),
                name: "after".into(),
            },
        ];
        p.transport.loop_start = q(0);
        p.transport.loop_end = q(16);
        delete_time(&mut p, q(6), q(8), &mut ids());
        assert_eq!(clips(&p, 0), [(10, 4, 2, 1_000), (1001, 6, 4, 1_000)]);
        assert_eq!(clips(&p, 1), [(20, 0, 6, 1_000)]);
        let markers: Vec<_> = p.markers.iter().map(|m| (m.name.as_str(), m.at)).collect();
        assert_eq!(markers, [("after", q(14))], "the one inside goes");
        assert_eq!(
            (p.transport.loop_start, p.transport.loop_end),
            (q(0), q(14))
        );
        assert_eq!(crate::validate(&p), Ok(()));
    }

    #[test]
    fn inserting_time_splits_what_crosses_and_moves_what_follows() {
        let mut p = song();
        insert_time(&mut p, q(6), q(4), &mut ids());
        assert_eq!(clips(&p, 0), [(10, 4, 2, 1_000), (1001, 10, 6, 1_000)]);
        assert_eq!(clips(&p, 1), [(20, 0, 6, 1_000), (1002, 10, 2, 1_000)]);
        assert_eq!(crate::validate(&p), Ok(()));
    }
}
