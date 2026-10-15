//! Note editing (M5 slice 3a): the piano roll's operations as pure functions
//! over a clip's note list. The app wraps each in one undo step
//! (`Document::move_notes` and the others), so everything here is tested
//! without a window.
//!
//! # The selection contract
//!
//! Notes are picked by **index** into the clip's `notes`. Every function
//! takes the picked indices and returns the indices of the notes it made or
//! changed, in the list as it is *after* the edit, so a UI keeps its
//! selection through the edit by storing what comes back (a duplicate
//! returns the copies, a glue the merged notes, a delete nothing).
//!
//! - Indices that are out of range or repeated are ignored; order does not
//!   matter.
//! - The list is kept sorted by start, then pitch, with ties left in their
//!   old order, so indices move after an edit that changes the order. An
//!   index is therefore only meaningful against the list it was made for:
//!   that is what a gesture's *base* is (the project as the gesture found
//!   it), and why a selection that outlives an undo or another edit must be
//!   checked with [`super::NoteSelection`] before use.
//! - Group edits keep relative offsets: a move is clamped as a whole, so no
//!   note is squeezed against a limit while the others go on.
//! - Overlaps between notes are never removed or created by an edit unless
//!   the edit is about joining them (glue).
//! - Only notes change; the clip's controller, bend, pressure and program
//!   events are not touched.
//!
//! `length` is the clip's content length (see `super::content_length`):
//! edits keep notes inside `[0, length]`, or no further outside it than
//! they already are. A note outside the clip's window (kept, hidden in the
//! arrangement, ADR-0056) is shown by the clip editor (ADR-0059): it can be
//! moved and resized there, and back into the clip, but not further out.

use lumen_core::time::{PPQ, Ticks};

use crate::model::Note;

/// The shortest a note can be made: a 1/128 note.
pub const MIN_NOTE: Ticks = Ticks(PPQ / 32);

/// Which end of a note a resize drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Start,
    End,
}

/// The picked indices, sorted, without repeats or indices past the list.
pub fn picked(selection: &[usize], len: usize) -> Vec<usize> {
    let mut v: Vec<usize> = selection.iter().copied().filter(|&i| i < len).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Every index, for "select all".
pub fn all(notes: &[Note]) -> Vec<usize> {
    (0..notes.len()).collect()
}

/// Notes tagged with whether they are part of the result.
pub(super) type Tagged = Vec<(Note, bool)>;

pub(super) fn tag(notes: &[Note], picked: &[usize]) -> Tagged {
    let mut tagged: Tagged = notes.iter().map(|&n| (n, false)).collect();
    for &i in picked {
        tagged[i].1 = true;
    }
    tagged
}

/// Writes `tagged` back, in order, and returns where the tagged notes are.
pub(super) fn finish(notes: &mut Vec<Note>, mut tagged: Tagged) -> Vec<usize> {
    if !tagged.is_sorted_by_key(|(n, _)| (n.start(), n.pitch())) {
        tagged.sort_by_key(|(n, _)| (n.start(), n.pitch()));
    }
    notes.clear();
    notes.reserve(tagged.len());
    let mut result = Vec::new();
    for (i, (note, chosen)) in tagged.into_iter().enumerate() {
        notes.push(note);
        if chosen {
            result.push(i);
        }
    }
    result
}

/// Adds `note`, kept inside the clip: pitch to 127, velocity to 1 to 127,
/// the start inside `[0, length)` and the length at least [`MIN_NOTE`], cut
/// where the content ends. Returns the new note's index, or nothing when
/// the start is outside the clip.
pub fn add(notes: &mut Vec<Note>, note: Note, length: Ticks) -> Vec<usize> {
    if note.start() < Ticks::ZERO || note.start() >= length {
        return Vec::new();
    }
    let start = note.start();
    let len = note
        .length()
        .max(MIN_NOTE)
        .min((length - start).max(Ticks(1)));
    let added = Note(
        start,
        len,
        note.pitch().min(127),
        note.velocity().clamp(1, 127),
        note.muted(),
        note.carries_on(),
    );
    let mut tagged = tag(notes, &[]);
    tagged.push((added, true));
    finish(notes, tagged)
}

/// Removes the picked notes. Nothing remains selected.
pub fn delete(notes: &mut Vec<Note>, selection: &[usize]) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let mut at = 0;
    notes.retain(|_| {
        let keep = picked.binary_search(&at).is_err();
        at += 1;
        keep
    });
    Vec::new()
}

/// How far a group may move in time: the whole group stays inside
/// `[0, length]`, or no further out than it is. A note that already runs
/// past the end can still move earlier, but not later; one before the start
/// later, but not earlier.
fn time_room(notes: &[Note], picked: &[usize], length: Ticks) -> (Ticks, Ticks) {
    let first = picked.iter().map(|&i| notes[i].start()).min();
    let last = picked.iter().map(|&i| notes[i].end()).max();
    match (first, last) {
        (Some(first), Some(last)) => (
            (Ticks::ZERO - first).min(Ticks::ZERO),
            (length - last).max(Ticks::ZERO),
        ),
        _ => (Ticks::ZERO, Ticks::ZERO),
    }
}

fn pitch_room(notes: &[Note], picked: &[usize]) -> (i32, i32) {
    let low = picked.iter().map(|&i| notes[i].pitch()).min();
    let high = picked.iter().map(|&i| notes[i].pitch()).max();
    match (low, high) {
        (Some(low), Some(high)) => (-i32::from(low), 127 - i32::from(high)),
        _ => (0, 0),
    }
}

/// What a move by `(time, pitch)` really is for this group: each part is
/// brought inside the limits as a whole.
pub fn clamp_move(
    notes: &[Note],
    selection: &[usize],
    time: Ticks,
    pitch: i32,
    length: Ticks,
) -> (Ticks, i32) {
    let picked = picked(selection, notes.len());
    let (earliest, latest) = time_room(notes, &picked, length);
    let (lowest, highest) = pitch_room(notes, &picked);
    (
        time.min(latest).max(earliest),
        pitch.min(highest).max(lowest),
    )
}

/// Moves the picked notes by `time` and `pitch` (semitones), the whole
/// group clamped at the limits ([`clamp_move`]).
pub fn move_notes(
    notes: &mut Vec<Note>,
    selection: &[usize],
    time: Ticks,
    pitch: i32,
    length: Ticks,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let (time, pitch) = clamp_move(notes, &picked, time, pitch, length);
    let mut tagged = tag(notes, &picked);
    for (note, chosen) in &mut tagged {
        if *chosen {
            *note = shifted(*note, time, pitch);
        }
    }
    finish(notes, tagged)
}

fn shifted(n: Note, time: Ticks, pitch: i32) -> Note {
    Note(
        n.start() + time,
        n.length(),
        (i32::from(n.pitch()) + pitch).clamp(0, 127) as u8,
        n.velocity(),
        n.muted(),
        n.carries_on(),
    )
}

/// Copies the picked notes, moved by `time` and `pitch` as a group
/// ([`clamp_move`]), leaving the originals. Returns the copies.
pub fn copy_moved(
    notes: &mut Vec<Note>,
    selection: &[usize],
    time: Ticks,
    pitch: i32,
    length: Ticks,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let (time, pitch) = clamp_move(notes, &picked, time, pitch, length);
    let mut tagged = tag(notes, &[]);
    tagged.extend(
        picked
            .iter()
            .map(|&i| (shifted(notes[i], time, pitch), true)),
    );
    finish(notes, tagged)
}

/// Moves the picked notes `by` in time (arrow keys), clamped as a group.
pub fn nudge(notes: &mut Vec<Note>, selection: &[usize], by: Ticks, length: Ticks) -> Vec<usize> {
    move_notes(notes, selection, by, 0, length)
}

/// Moves the picked notes by `semitones` (12 is an octave), clamped as a
/// group: a chord at the top of the range stays a chord.
pub fn transpose(notes: &mut Vec<Note>, selection: &[usize], semitones: i32) -> Vec<usize> {
    move_notes(
        notes,
        selection,
        Ticks::ZERO,
        semitones,
        Ticks(i64::MAX / 4),
    )
}

/// Drags one edge of the picked notes by `by`: the end edge changes their
/// lengths, the start edge their starts and lengths (the ends stay). As a
/// group, so every note keeps at least `min` and none runs past `length`
/// or before zero (no further than it already is, for a note outside the
/// clip's window); the notes keep their differences while the edge is
/// within those limits.
pub fn resize(
    notes: &mut Vec<Note>,
    selection: &[usize],
    end: End,
    by: Ticks,
    min: Ticks,
    length: Ticks,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let min = min.max(Ticks(1));
    let by = match end {
        End::End => {
            let lowest = picked.iter().map(|&i| min - notes[i].length()).max();
            let highest = picked
                .iter()
                .map(|&i| (length - notes[i].end()).max(Ticks::ZERO))
                .min();
            match (lowest, highest) {
                (Some(lowest), Some(highest)) => by.min(highest).max(lowest),
                _ => return Vec::new(),
            }
        }
        End::Start => {
            let lowest = picked
                .iter()
                .map(|&i| (Ticks::ZERO - notes[i].start()).min(Ticks::ZERO))
                .max();
            let highest = picked.iter().map(|&i| notes[i].length() - min).min();
            match (lowest, highest) {
                (Some(lowest), Some(highest)) => by.min(highest).max(lowest),
                _ => return Vec::new(),
            }
        }
    };
    let mut tagged = tag(notes, &picked);
    for (n, chosen) in &mut tagged {
        if !*chosen {
            continue;
        }
        *n = match end {
            End::End => Note(
                n.start(),
                n.length() + by,
                n.pitch(),
                n.velocity(),
                n.muted(),
                n.carries_on(),
            ),
            End::Start => Note(
                n.start() + by,
                n.length() - by,
                n.pitch(),
                n.velocity(),
                n.muted(),
                n.carries_on(),
            ),
        };
    }
    finish(notes, tagged)
}

/// Splits the picked notes that sound at `at` (start before it, end after
/// it) in two, there. Returns both halves of each; picked notes `at` does
/// not cut stay picked.
pub fn split(notes: &mut Vec<Note>, selection: &[usize], at: Ticks) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let mut tagged = tag(notes, &[]);
    let mut parts = Vec::new();
    for &i in &picked {
        let n = notes[i];
        if n.start() < at && at < n.end() {
            tagged[i] = (
                Note(
                    n.start(),
                    at - n.start(),
                    n.pitch(),
                    n.velocity(),
                    n.muted(),
                    n.carries_on(),
                ),
                true,
            );
            parts.push((
                Note(
                    at,
                    n.end() - at,
                    n.pitch(),
                    n.velocity(),
                    n.muted(),
                    n.carries_on(),
                ),
                true,
            ));
        } else {
            tagged[i].1 = true;
        }
    }
    tagged.extend(parts);
    finish(notes, tagged)
}

/// Joins the picked notes of one pitch that touch or overlap into single
/// notes (from the earliest start to the latest end, with the first one's
/// velocity; muted only when all were). Other picked notes stay as they
/// are. Returns the joined and the untouched picked notes.
pub fn glue(notes: &mut Vec<Note>, selection: &[usize]) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let mut by_pitch = picked.clone();
    by_pitch.sort_by_key(|&i| (notes[i].pitch(), notes[i].start()));
    let mut joined: Vec<Note> = Vec::new();
    let mut mute_all = true;
    for &i in &by_pitch {
        let n = notes[i];
        match joined.last_mut() {
            Some(last) if last.pitch() == n.pitch() && n.start() <= last.end() => {
                let end = last.end().max(n.end());
                mute_all &= n.muted();
                *last = Note(
                    last.start(),
                    end - last.start(),
                    last.pitch(),
                    last.velocity(),
                    mute_all && last.muted(),
                    last.carries_on() && n.carries_on(),
                );
            }
            _ => {
                mute_all = n.muted();
                joined.push(n);
            }
        }
    }
    let mut tagged: Tagged = Vec::with_capacity(notes.len());
    let mut taken = picked.iter().peekable();
    for (i, &n) in notes.iter().enumerate() {
        if taken.peek() == Some(&&i) {
            taken.next();
        } else {
            tagged.push((n, false));
        }
    }
    tagged.extend(joined.into_iter().map(|n| (n, true)));
    finish(notes, tagged)
}

/// Mutes the picked notes, unmutes them, or (with `None`) toggles: they
/// are all unmuted if every one was muted, otherwise all muted.
pub fn mute(notes: &mut [Note], selection: &[usize], muted: Option<bool>) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let to = muted.unwrap_or_else(|| !picked.iter().all(|&i| notes[i].muted()));
    for &i in &picked {
        notes[i].4 = to;
    }
    picked
}

/// Copies the picked notes after themselves: the copies sit one selection
/// span (earliest start to latest end) later. Copies that would start past
/// `length` are left out and the rest are cut at it. Returns the copies, or
/// the picked notes themselves if none fit.
pub fn duplicate(notes: &mut Vec<Note>, selection: &[usize], length: Ticks) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let (Some(first), Some(last)) = (
        picked.iter().map(|&i| notes[i].start()).min(),
        picked.iter().map(|&i| notes[i].end()).max(),
    ) else {
        return Vec::new();
    };
    let span = last - first;
    let copies: Vec<Note> = picked
        .iter()
        .filter_map(|&i| {
            let n = notes[i];
            let start = n.start() + span;
            (start < length).then(|| {
                Note(
                    start,
                    n.length().min(length - start),
                    n.pitch(),
                    n.velocity(),
                    n.muted(),
                    n.carries_on(),
                )
            })
        })
        .collect();
    if copies.is_empty() {
        return picked;
    }
    let mut tagged = tag(notes, &[]);
    tagged.extend(copies.into_iter().map(|n| (n, true)));
    finish(notes, tagged)
}

/// Lengthens each picked note to the next note onset after its own start
/// (any pitch), or to the end of the content for the last. Notes are only
/// ever lengthened: one already past that onset keeps its overlap.
pub fn legato(notes: &mut Vec<Note>, selection: &[usize], length: Ticks) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let mut onsets: Vec<Ticks> = notes.iter().map(|n| n.start()).collect();
    onsets.sort_unstable();
    onsets.dedup();
    let mut tagged = tag(notes, &picked);
    for (n, chosen) in &mut tagged {
        if !*chosen {
            continue;
        }
        let next = onsets.partition_point(|&t| t <= n.start());
        let to = onsets.get(next).copied().unwrap_or(length).min(length);
        if to > n.end() {
            n.1 = to - n.start();
        }
    }
    finish(notes, tagged)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: Ticks = Ticks(PPQ);

    fn q(n: i64) -> Ticks {
        Ticks(PPQ * n)
    }

    fn n(start: i64, len: i64, pitch: u8) -> Note {
        Note::new(q(start), q(len), pitch, 100)
    }

    fn muted(mut note: Note) -> Note {
        note.4 = true;
        note
    }

    const LEN: Ticks = Ticks(PPQ * 16);

    #[test]
    fn adding_keeps_a_note_inside_the_clip_and_sorted() {
        let mut notes = vec![n(0, 1, 60), n(4, 1, 60)];
        assert_eq!(add(&mut notes, n(2, 1, 64), LEN), [1]);
        assert_eq!(notes[1], n(2, 1, 64));
        // Cut at the end, at least the shortest, pitch and velocity clamped.
        let last = Note::new(q(15), q(5), 200, 0);
        let at = add(&mut notes, last, LEN);
        assert_eq!(notes[at[0]], Note::new(q(15), q(1), 127, 1));
        let tiny = add(&mut notes, Note::new(q(8), Ticks(1), 60, 90), LEN);
        assert_eq!(notes[tiny[0]].length(), MIN_NOTE);
        // Outside the clip: refused.
        assert!(add(&mut notes, n(16, 1, 60), LEN).is_empty());
        assert!(add(&mut notes, Note::new(Ticks(-1), Q, 60, 90), LEN).is_empty());
        assert_eq!(notes.len(), 5);
    }

    #[test]
    fn deleting_removes_exactly_the_picked_notes_and_ignores_bad_indices() {
        let mut notes = vec![n(0, 1, 60), n(1, 1, 62), n(2, 1, 64)];
        assert!(delete(&mut notes, &[2, 0, 0, 9]).is_empty());
        assert_eq!(notes, [n(1, 1, 62)]);
    }

    #[test]
    fn a_note_outside_the_clip_moves_back_in_but_no_further_out() {
        // Hidden before the clip's start, and past its end (ADR-0059).
        let mut notes = vec![n(-3, 2, 60), n(LEN.0 / q(1).0 + 1, 1, 62)];
        move_notes(&mut notes, &[0], q(-5), 0, LEN);
        assert_eq!(notes[0], n(-3, 2, 60), "no further out");
        move_notes(&mut notes, &[0], q(4), 0, LEN);
        assert_eq!(notes[0], n(1, 2, 60), "back in the clip");
        let past = notes[1].start();
        move_notes(&mut notes, &[1], q(2), 0, LEN);
        assert_eq!(notes[1].start(), past, "no later past the end");
        move_notes(&mut notes, &[1], q(-3), 0, LEN);
        assert_eq!(notes[1].start(), past - q(3));
        // Its edges move too, the start no earlier than it is.
        let mut notes = vec![n(-3, 2, 60)];
        resize(&mut notes, &[0], End::Start, q(-1), q(1), LEN);
        assert_eq!(notes[0], n(-3, 2, 60));
        resize(&mut notes, &[0], End::End, q(4), q(1), LEN);
        assert_eq!(notes[0], n(-3, 6, 60));
    }

    #[test]
    fn a_group_moves_together_and_clamps_as_a_whole() {
        let mut notes = vec![n(1, 1, 60), n(2, 2, 72), n(9, 1, 40)];
        // Earlier than the earliest note allows: the group stops at 0 and
        // keeps its offsets.
        let sel = move_notes(&mut notes, &[0, 1], q(-5), 0, LEN);
        assert_eq!(sel, [0, 1]);
        assert_eq!(notes[0], n(0, 1, 60));
        assert_eq!(notes[1], n(1, 2, 72));
        // Later than the clip: the group's end stops at the content's.
        move_notes(&mut notes, &[0, 1], q(100), 0, LEN);
        assert_eq!((notes[1].start(), notes[2].start()), (q(13), q(14)));
        assert_eq!(notes[2].end(), LEN);
        // Pitch: 72 may rise 55 and 60 is the lowest at 60, so +60 stops at +55.
        let mut notes = vec![n(0, 1, 60), n(0, 1, 72)];
        move_notes(&mut notes, &[0, 1], Ticks::ZERO, 60, LEN);
        assert_eq!((notes[0].pitch(), notes[1].pitch()), (115, 127));
        move_notes(&mut notes, &[0, 1], Ticks::ZERO, -200, LEN);
        assert_eq!((notes[0].pitch(), notes[1].pitch()), (0, 12));
    }

    #[test]
    fn a_move_reorders_and_returns_the_new_indices() {
        let mut notes = vec![n(0, 1, 60), n(1, 1, 62), n(2, 1, 64)];
        let sel = move_notes(&mut notes, &[0], q(3), 0, LEN);
        assert_eq!(sel, [2]);
        assert_eq!(notes[2], n(3, 1, 60));
        assert!(notes.is_sorted_by_key(|x| x.start()));
    }

    #[test]
    fn a_note_already_past_the_end_can_only_move_earlier() {
        let mut notes = vec![n(15, 3, 60)];
        move_notes(&mut notes, &[0], q(1), 0, LEN);
        assert_eq!(notes[0], n(15, 3, 60));
        move_notes(&mut notes, &[0], q(-1), 0, LEN);
        assert_eq!(notes[0], n(14, 3, 60));
    }

    #[test]
    fn copy_moved_leaves_the_originals() {
        let mut notes = vec![n(0, 1, 60), n(2, 1, 64)];
        let sel = copy_moved(&mut notes, &[0], q(4), 2, LEN);
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[sel[0]], n(4, 1, 62));
        assert_eq!(notes[0], n(0, 1, 60));
    }

    #[test]
    fn nudging_and_transposing_move_one_dimension() {
        let mut notes = vec![n(0, 1, 60), n(1, 1, 125)];
        nudge(&mut notes, &[0, 1], q(1), LEN);
        assert_eq!((notes[0].start(), notes[1].start()), (q(1), q(2)));
        assert_eq!(notes[0].pitch(), 60);
        // The chord stays a chord at the top of the range.
        transpose(&mut notes, &[0, 1], 12);
        assert_eq!((notes[0].pitch(), notes[1].pitch()), (62, 127));
        transpose(&mut notes, &[0, 1], -12);
        assert_eq!((notes[0].pitch(), notes[1].pitch()), (50, 115));
    }

    #[test]
    fn resizing_an_end_keeps_the_minimum_and_the_clip_end() {
        let mut notes = vec![n(0, 2, 60), n(14, 1, 62)];
        resize(&mut notes, &[0], End::End, q(-9), MIN_NOTE, LEN);
        assert_eq!(notes[0].length(), MIN_NOTE);
        resize(&mut notes, &[1], End::End, q(9), MIN_NOTE, LEN);
        assert_eq!(notes[1].end(), LEN);
        // A group: the shorter note sets the limit for both.
        let mut notes = vec![n(0, 1, 60), n(2, 4, 62)];
        resize(&mut notes, &[0, 1], End::End, q(-3), MIN_NOTE, LEN);
        assert_eq!(notes[0].length(), MIN_NOTE);
        assert_eq!(notes[1].length(), q(4) - (q(1) - MIN_NOTE));
    }

    #[test]
    fn resizing_a_start_keeps_the_end_in_place() {
        let mut notes = vec![n(2, 4, 60)];
        resize(&mut notes, &[0], End::Start, q(1), MIN_NOTE, LEN);
        assert_eq!((notes[0].start(), notes[0].end()), (q(3), q(6)));
        resize(&mut notes, &[0], End::Start, q(-10), MIN_NOTE, LEN);
        assert_eq!((notes[0].start(), notes[0].end()), (Ticks::ZERO, q(6)));
        resize(&mut notes, &[0], End::Start, q(10), MIN_NOTE, LEN);
        assert_eq!(notes[0].length(), MIN_NOTE);
        assert_eq!(notes[0].end(), q(6));
    }

    #[test]
    fn splitting_cuts_only_notes_that_sound_at_the_time() {
        let mut notes = vec![n(0, 4, 60), n(1, 1, 62), n(8, 2, 64)];
        let sel = split(&mut notes, &[0, 1, 2], q(2));
        assert_eq!(notes.len(), 4);
        assert_eq!(notes[0], n(0, 2, 60));
        assert_eq!(notes[2], n(2, 2, 60));
        assert_eq!(sel.len(), 4, "both halves and the notes it did not cut");
        // On a boundary nothing is cut.
        let mut notes = vec![n(0, 4, 60)];
        split(&mut notes, &[0], q(4));
        split(&mut notes, &[0], Ticks::ZERO);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn split_then_glue_returns_the_original() {
        let mut notes = vec![muted(n(0, 4, 60)), n(6, 1, 61)];
        let original = notes.clone();
        let parts = split(&mut notes, &[0], q(1));
        assert_eq!(notes.len(), 3);
        let joined = glue(&mut notes, &parts);
        assert_eq!(notes, original);
        assert_eq!(joined, [0]);
    }

    #[test]
    fn glue_joins_touching_and_overlapping_notes_of_one_pitch() {
        let mut notes = vec![
            n(0, 2, 60),
            n(2, 2, 60),
            n(3, 3, 60),
            n(8, 1, 60),
            n(0, 1, 64),
            n(1, 1, 64),
        ];
        let sel = glue(&mut notes, &[0, 1, 2, 3, 4, 5]);
        assert_eq!(notes.len(), 3);
        assert!(notes.contains(&n(0, 6, 60)));
        assert!(notes.contains(&n(8, 1, 60)));
        assert!(notes.contains(&n(0, 2, 64)));
        assert_eq!(sel.len(), 3);
        // A note outside the selection is not swallowed.
        let mut notes = vec![n(0, 2, 60), n(2, 2, 60)];
        glue(&mut notes, &[0]);
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn glued_notes_are_muted_only_when_all_were() {
        let mut notes = vec![muted(n(0, 1, 60)), n(1, 1, 60)];
        glue(&mut notes, &[0, 1]);
        assert!(!notes[0].muted());
        let mut notes = vec![muted(n(0, 1, 60)), muted(n(1, 1, 60))];
        glue(&mut notes, &[0, 1]);
        assert!(notes[0].muted());
    }

    #[test]
    fn muting_toggles_the_group() {
        let mut notes = vec![n(0, 1, 60), muted(n(1, 1, 62)), n(2, 1, 64)];
        assert_eq!(mute(&mut notes, &[0, 1], None), [0, 1]);
        assert!(notes[0].muted() && notes[1].muted() && !notes[2].muted());
        mute(&mut notes, &[0, 1], None);
        assert!(!notes[0].muted() && !notes[1].muted());
        mute(&mut notes, &[2], Some(true));
        assert!(notes[2].muted());
        mute(&mut notes, &[2], Some(true));
        assert!(notes[2].muted());
        mute(&mut notes, &[2], Some(false));
        assert!(!notes[2].muted());
    }

    #[test]
    fn duplicating_puts_the_copies_one_selection_after() {
        let mut notes = vec![n(0, 1, 60), n(2, 1, 64), n(10, 1, 70)];
        let sel = duplicate(&mut notes, &[0, 1], LEN);
        // Span is 0..3 so the copies start at 3 and 5.
        assert_eq!(notes.len(), 5);
        let copies: Vec<_> = sel.iter().map(|&i| notes[i]).collect();
        assert_eq!(copies, [n(3, 1, 60), n(5, 1, 64)]);
    }

    #[test]
    fn duplicating_near_the_end_drops_and_cuts_what_does_not_fit() {
        let mut notes = vec![n(12, 2, 60), n(14, 2, 62)];
        // Span 4, copies at 16 (outside) and 18 (outside): none fit.
        let sel = duplicate(&mut notes, &[0, 1], LEN);
        assert_eq!((notes.len(), sel), (2, vec![0, 1]));
        let mut notes = vec![n(10, 4, 60), n(12, 6, 62)];
        // Span 8: copies at 18 and 20, outside.
        duplicate(&mut notes, &[0], LEN);
        // Span 4: copy at 14 is cut to 2.
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[2], n(14, 2, 60));
    }

    #[test]
    fn legato_extends_to_the_next_onset_and_never_shortens() {
        let mut notes = vec![
            n(0, 1, 60),
            n(2, 1, 62),
            n(2, 1, 64),
            n(4, 8, 65),
            n(6, 1, 67),
        ];
        let sel = legato(&mut notes, &[0, 1, 2, 3, 4], LEN);
        assert_eq!(sel, [0, 1, 2, 3, 4]);
        assert_eq!(notes[0].length(), q(2));
        assert_eq!(
            notes[1].length(),
            q(2),
            "to the next onset, not its chord mate"
        );
        assert_eq!(notes[2].length(), q(2));
        assert_eq!(
            notes[3].length(),
            q(8),
            "already past the onset at 6: untouched"
        );
        assert_eq!(
            notes[4].end(),
            LEN,
            "the last runs to the end of the content"
        );
        // Unpicked notes stay.
        let mut notes = vec![n(0, 1, 60), n(2, 1, 62)];
        legato(&mut notes, &[1], LEN);
        assert_eq!(notes[0].length(), q(1));
    }

    #[test]
    fn existing_overlaps_survive_every_move() {
        let mut notes = vec![n(0, 4, 60), n(1, 4, 60)];
        move_notes(&mut notes, &[0, 1], q(2), 3, LEN);
        assert_eq!(notes, [n(2, 4, 63), n(3, 4, 63)]);
    }

    #[test]
    fn muted_notes_and_velocities_survive_edits() {
        let mut notes = vec![muted(Note::new(q(0), q(2), 60, 33))];
        move_notes(&mut notes, &[0], q(1), 1, LEN);
        resize(&mut notes, &[0], End::End, q(1), MIN_NOTE, LEN);
        let parts = split(&mut notes, &[0], q(2));
        assert!(
            parts
                .iter()
                .all(|&i| notes[i].muted() && notes[i].velocity() == 33)
        );
    }

    #[test]
    fn the_list_stays_sorted_and_empty_selections_change_nothing() {
        let mut notes = vec![n(0, 1, 60), n(1, 1, 62)];
        let before = notes.clone();
        assert!(move_notes(&mut notes, &[], q(1), 1, LEN).is_empty());
        assert!(split(&mut notes, &[], q(0)).is_empty());
        assert!(glue(&mut notes, &[]).is_empty());
        assert!(duplicate(&mut notes, &[], LEN).is_empty());
        assert!(legato(&mut notes, &[], LEN).is_empty());
        assert!(resize(&mut notes, &[], End::End, q(1), MIN_NOTE, LEN).is_empty());
        assert_eq!(notes, before);
    }
}
