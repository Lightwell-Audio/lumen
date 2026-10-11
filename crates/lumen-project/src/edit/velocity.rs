//! Velocity edits (M5 slice 4a): the velocity lane's operations as pure
//! functions over a clip's note list. The app wraps each in one undo step,
//! so everything here is tested without a window.
//!
//! # The selection contract
//!
//! As in [`super::notes`], notes are picked by **index** into the clip's
//! `notes`. Each function takes the picked indices and returns the indices
//! it set, sorted and without repeats.
//!
//! - Indices out of range or repeated are ignored; order does not matter.
//! - A velocity is always kept in `1..=127`: below 1 it is raised to 1,
//!   above 127 lowered to 127. Velocity 0 is a note-off in MIDI, so these
//!   functions never silence a note; mute it with [`super::notes::mute`].
//! - Only velocities change. Start, length, pitch, mute and the order of the
//!   list stay as they are, so the indices returned, and any the caller
//!   holds, stay valid after the edit.
//! - [`at_start`] and [`stems`] group notes by start time: the velocity
//!   lane draws one stem per start, and widens a pick to the whole chord.

use std::collections::BTreeMap;

use lumen_core::curve::Curve;
use lumen_core::time::Ticks;

use super::notes::picked;
use crate::model::Note;

/// The lowest velocity a note can have.
const LOW: i64 = 1;
/// The highest velocity a note can have.
const HIGH: i64 = 127;

/// `v` brought into the valid velocity range.
fn valid(v: i64) -> u8 {
    v.clamp(LOW, HIGH) as u8
}

/// Sets every picked note's velocity to `velocity` (clamped to 1..=127),
/// muted notes included. Returns the picked indices.
pub fn set(notes: &mut [Note], selection: &[usize], velocity: u8) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let velocity = valid(i64::from(velocity));
    for &i in &picked {
        notes[i].3 = velocity;
    }
    picked
}

/// Sets each listed note's velocity (clamped to 1..=127). Entries are applied
/// in order, so a later entry for the same index wins. Indices out of range
/// are ignored. Returns the indices that were set, sorted and without
/// repeats; a note named here is returned even if it already had that
/// velocity. Used when a drag draws across several stems.
pub fn set_each(notes: &mut [Note], changes: &[(usize, u8)]) -> Vec<usize> {
    let mut changed = Vec::with_capacity(changes.len());
    for &(i, velocity) in changes {
        if let Some(note) = notes.get_mut(i) {
            note.3 = valid(i64::from(velocity));
            changed.push(i);
        }
    }
    changed.sort_unstable();
    changed.dedup();
    changed
}

/// Adds `delta` to each picked note's velocity, clamped to 1..=127. A note
/// already at a limit that the delta pushes past it stays there. Returns the
/// picked indices.
pub fn offset(notes: &mut [Note], selection: &[usize], delta: i32) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    for &i in &picked {
        notes[i].3 = valid(i64::from(notes[i].velocity()) + i64::from(delta));
    }
    picked
}

/// Multiplies each picked note's velocity by `percent / 100`, rounded to the
/// nearest whole value (halves go up), then clamped to 1..=127: 0 percent
/// gives 1, not silence. Returns the picked indices.
pub fn scale(notes: &mut [Note], selection: &[usize], percent: u32) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    for &i in &picked {
        let product = i64::from(notes[i].velocity()) * i64::from(percent);
        notes[i].3 = valid((product + 50) / 100);
    }
    picked
}

/// Sets each picked note's velocity from a line across time, shaped by
/// `curve`: the velocity goes from `from.1` at `from.0` to `to.1` at `to.0`,
/// with the time of each note mapped to `0..=1` between them and held at
/// the ends outside. The result is rounded to the nearest velocity and
/// clamped to 1..=127. If `to` is earlier than `from` the two swap places.
///
/// When the two times are equal there is no line: notes before that time
/// take `from`'s velocity, notes at or after it take `to`'s (the values as
/// passed, so `to` wins at the time itself). Returns the picked indices.
pub fn ramp(
    notes: &mut [Note],
    selection: &[usize],
    from: (Ticks, u8),
    to: (Ticks, u8),
    curve: &Curve,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let (from, to) = if to.0 < from.0 {
        (to, from)
    } else {
        (from, to)
    };
    for &i in &picked {
        let start = notes[i].start();
        let velocity = if from.0 == to.0 {
            if start >= to.0 {
                f64::from(to.1)
            } else {
                f64::from(from.1)
            }
        } else {
            let along = (start - from.0).0 as f64 / (to.0 - from.0).0 as f64;
            let level = f64::from(curve.at(along.clamp(0.0, 1.0) as f32));
            f64::from(from.1) + (f64::from(to.1) - f64::from(from.1)) * level
        };
        // A float cast saturates, and NaN becomes 0, which `valid` raises to 1.
        notes[i].3 = valid(velocity.round() as i64);
    }
    picked
}

/// The indices of every note (any pitch) that starts at the same tick as
/// note `index`, `index` included, ascending. Empty if `index` is out of
/// range.
pub fn at_start(notes: &[Note], index: usize) -> Vec<usize> {
    let Some(note) = notes.get(index) else {
        return Vec::new();
    };
    let start = note.start();
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.start() == start)
        .map(|(i, _)| i)
        .collect()
}

/// The notes grouped by start tick, in time order. Each group's indices are
/// ascending. The notes need not be sorted.
pub fn stems(notes: &[Note]) -> Vec<(Ticks, Vec<usize>)> {
    let mut by_start: BTreeMap<Ticks, Vec<usize>> = BTreeMap::new();
    for (i, note) in notes.iter().enumerate() {
        by_start.entry(note.start()).or_default().push(i);
    }
    by_start.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    /// A one-quarter note at quarter `start`, unmuted.
    fn note(start: i64, pitch: u8, velocity: u8) -> Note {
        Note::new(q(start), q(1), pitch, velocity)
    }

    fn muted(mut n: Note) -> Note {
        n.4 = true;
        n
    }

    /// Everything but the velocity: start, length, pitch and mute.
    fn shape(notes: &[Note]) -> Vec<(Ticks, Ticks, u8, bool)> {
        notes
            .iter()
            .map(|n| (n.start(), n.length(), n.pitch(), n.muted()))
            .collect()
    }

    fn velocities(notes: &[Note]) -> Vec<u8> {
        notes.iter().map(|n| n.velocity()).collect()
    }

    /// A chord of three at quarter 0, one note at 1 and one at 2; the
    /// chord's top note is muted. Sorted by (start, pitch).
    fn sample() -> Vec<Note> {
        vec![
            note(0, 60, 40),
            note(0, 64, 60),
            muted(note(0, 67, 80)),
            note(1, 60, 100),
            note(2, 62, 120),
        ]
    }

    #[test]
    fn set_gives_the_picked_notes_one_velocity_and_returns_them() {
        let mut notes = sample();
        let before = shape(&notes);
        assert_eq!(set(&mut notes, &[3, 0], 90), [0, 3]);
        assert_eq!(velocities(&notes), [90, 60, 80, 90, 120]);
        assert_eq!(shape(&notes), before);
        assert!(set(&mut notes, &[], 9).is_empty());
    }

    #[test]
    fn set_clamps_to_the_valid_range_and_reaches_muted_notes() {
        let mut notes = sample();
        set(&mut notes, &[0], 0);
        assert_eq!(notes[0].velocity(), 1);
        set(&mut notes, &[0], 255);
        assert_eq!(notes[0].velocity(), 127);
        // The muted note is set like the others, and stays muted.
        set(&mut notes, &[2], 10);
        assert_eq!((notes[2].velocity(), notes[2].muted()), (10, true));
    }

    #[test]
    fn out_of_range_and_repeated_indices_are_ignored_by_set() {
        let mut notes = sample();
        assert_eq!(set(&mut notes, &[4, 4, 4, 99, 1], 50), [1, 4]);
        assert_eq!(velocities(&notes), [40, 50, 80, 100, 50]);
        assert!(set(&mut notes, &[99], 9).is_empty());
        assert_eq!(velocities(&notes), [40, 50, 80, 100, 50]);
    }

    #[test]
    fn set_each_sets_each_listed_note_and_the_last_entry_wins() {
        let mut notes = sample();
        let changed = set_each(&mut notes, &[(0, 50), (2, 0), (0, 70), (9, 40), (4, 255)]);
        assert_eq!(changed, [0, 2, 4]);
        assert_eq!(velocities(&notes), [70, 60, 1, 100, 127]);
        assert_eq!(shape(&notes), shape(&sample()));
        assert!(set_each(&mut notes, &[]).is_empty());
        assert!(set_each(&mut notes, &[(9, 40)]).is_empty());
    }

    #[test]
    fn offset_adds_to_each_picked_note() {
        let mut notes = sample();
        let before = shape(&notes);
        assert_eq!(offset(&mut notes, &[0, 3], 10), [0, 3]);
        assert_eq!(velocities(&notes), [50, 60, 80, 110, 120]);
        assert_eq!(shape(&notes), before);
        // Repeats and out-of-range indices add once and are ignored.
        assert_eq!(offset(&mut notes, &[1, 1, 7], -5), [1]);
        assert_eq!(velocities(&notes), [50, 55, 80, 110, 120]);
    }

    #[test]
    fn offset_clamps_and_notes_at_a_limit_stay_there() {
        let mut notes = vec![note(0, 60, 120), note(1, 60, 5)];
        offset(&mut notes, &[0, 1], 20);
        assert_eq!(velocities(&notes), [127, 25]);
        offset(&mut notes, &[0, 1], -200);
        assert_eq!(velocities(&notes), [1, 1]);
        // At a limit and pushed past it: stays at the limit.
        let mut notes = vec![note(0, 60, 127), note(1, 60, 1)];
        offset(&mut notes, &[0], 5);
        offset(&mut notes, &[1], -5);
        assert_eq!(velocities(&notes), [127, 1]);
        // Extreme deltas do not overflow.
        offset(&mut notes, &[0], i32::MAX);
        offset(&mut notes, &[1], i32::MIN);
        assert_eq!(velocities(&notes), [127, 1]);
        offset(&mut notes, &[0], i32::MIN);
        assert_eq!(notes[0].velocity(), 1);
    }

    #[test]
    fn scale_multiplies_and_rounds_to_the_nearest_velocity() {
        let mut notes = vec![
            note(0, 60, 100),
            note(1, 60, 3),
            note(2, 60, 5),
            note(3, 60, 10),
            note(4, 60, 80),
        ];
        assert_eq!(scale(&mut notes, &[0], 50), [0]);
        assert_eq!(notes[0].velocity(), 50);
        // 3 * 0.5 = 1.5 rounds up to 2; 5 * 0.5 = 2.5 rounds up to 3.
        assert_eq!(scale(&mut notes, &[1, 2, 3], 50), [1, 2, 3]);
        assert_eq!(velocities(&notes), [50, 2, 3, 5, 80]);
        // Clamped at 127, and 0 percent gives 1, not silence.
        scale(&mut notes, &[4], 200);
        assert_eq!(notes[4].velocity(), 127);
        scale(&mut notes, &[4], 0);
        assert_eq!(notes[4].velocity(), 1);
        scale(&mut notes, &[0], u32::MAX);
        assert_eq!(notes[0].velocity(), 127);
        // 100 percent changes nothing; out-of-range indices are ignored.
        scale(&mut notes, &[3], 100);
        assert_eq!(notes[3].velocity(), 5);
        assert!(scale(&mut notes, &[99], 10).is_empty());
    }

    #[test]
    fn scale_keeps_muted_notes_and_the_rest_of_each_note() {
        let mut notes = sample();
        let before = shape(&notes);
        scale(&mut notes, &[0, 1, 2, 3, 4], 150);
        assert_eq!(velocities(&notes), [60, 90, 120, 127, 127]);
        assert_eq!(shape(&notes), before);
    }

    #[test]
    fn ramp_draws_a_line_across_time_and_holds_past_its_ends() {
        let mut notes = vec![
            note(0, 60, 1),
            note(1, 60, 1),
            note(2, 60, 1),
            note(4, 60, 1),
            note(8, 60, 1),
        ];
        let before = shape(&notes);
        let sel = ramp(
            &mut notes,
            &[0, 1, 2, 4, 1, 99],
            (q(0), 20),
            (q(4), 100),
            &Curve::LINEAR,
        );
        assert_eq!(sel, [0, 1, 2, 4]);
        // At 0, 1 and 2 quarters: 20, 40, 60. Past the end holds at 100.
        // Note 3 is not picked, so it keeps its velocity.
        assert_eq!(velocities(&notes), [20, 40, 60, 1, 100]);
        assert_eq!(shape(&notes), before);
    }

    #[test]
    fn ramp_follows_the_curve_and_slow_bends_below_the_line() {
        let mut notes = vec![note(0, 60, 1), note(2, 60, 1)];
        ramp(&mut notes, &[0, 1], (q(0), 20), (q(4), 100), &Curve::SLOW);
        // Halfway in time SLOW is at 0.25 of the way up: 20 + 80 * 0.25 = 40,
        // below the straight line's 60.
        assert_eq!(notes[1].velocity(), 40);
        assert!(notes[1].velocity() < 60);
        assert_eq!(notes[0].velocity(), 20);
    }

    #[test]
    fn ramp_with_reversed_times_is_the_same_ramp() {
        let mut forward = vec![note(0, 60, 1), note(2, 60, 1), note(4, 60, 1)];
        let mut reversed = forward.clone();
        ramp(
            &mut forward,
            &[0, 1, 2],
            (q(0), 20),
            (q(4), 100),
            &Curve::LINEAR,
        );
        ramp(
            &mut reversed,
            &[0, 1, 2],
            (q(4), 100),
            (q(0), 20),
            &Curve::LINEAR,
        );
        assert_eq!(velocities(&forward), [20, 60, 100]);
        assert_eq!(velocities(&reversed), velocities(&forward));
    }

    #[test]
    fn ramp_with_equal_times_steps_at_that_time() {
        let mut notes = vec![note(0, 60, 1), note(2, 60, 1), note(3, 60, 1)];
        ramp(
            &mut notes,
            &[0, 1, 2],
            (q(2), 10),
            (q(2), 90),
            &Curve::LINEAR,
        );
        // Before the time: from's velocity. At and after it: to's.
        assert_eq!(velocities(&notes), [10, 90, 90]);
    }

    #[test]
    fn ramp_clamps_and_rounds_to_whole_velocities() {
        let mut notes = vec![note(0, 60, 1), note(4, 60, 1)];
        ramp(&mut notes, &[0, 1], (q(0), 0), (q(4), 255), &Curve::LINEAR);
        assert_eq!(velocities(&notes), [1, 127]);
        // 10 to 12 over three quarters: 10.67 at 1 quarter rounds to 11,
        // 11.33 at 2 quarters rounds to 11.
        let mut notes = vec![
            note(0, 60, 1),
            note(1, 60, 1),
            note(2, 60, 1),
            note(3, 60, 1),
        ];
        ramp(
            &mut notes,
            &[0, 1, 2, 3],
            (q(0), 10),
            (q(3), 12),
            &Curve::LINEAR,
        );
        assert_eq!(velocities(&notes), [10, 11, 11, 12]);
    }

    #[test]
    fn at_start_widens_a_pick_to_its_chord() {
        let notes = sample();
        assert_eq!(at_start(&notes, 0), [0, 1, 2]);
        assert_eq!(at_start(&notes, 2), [0, 1, 2]);
        assert_eq!(at_start(&notes, 3), [3], "a lone note is its own start");
        assert!(at_start(&notes, 9).is_empty());
        assert!(at_start(&[], 0).is_empty());
    }

    #[test]
    fn stems_group_notes_by_start_in_time_order() {
        let notes = sample();
        assert_eq!(
            stems(&notes),
            [(q(0), vec![0, 1, 2]), (q(1), vec![3]), (q(2), vec![4])]
        );
        // The notes need not be sorted: groups still come in time order.
        let scrambled = vec![note(2, 60, 1), note(0, 64, 1), note(2, 67, 1)];
        assert_eq!(stems(&scrambled), [(q(0), vec![1]), (q(2), vec![0, 2])]);
        assert!(stems(&[]).is_empty());
    }

    #[test]
    fn no_function_changes_start_length_pitch_mute_or_order() {
        let before = shape(&sample());
        let all = [0, 1, 2, 3, 4];

        let mut notes = sample();
        set(&mut notes, &all, 7);
        assert_eq!(shape(&notes), before, "set");

        let mut notes = sample();
        set_each(&mut notes, &[(0, 7), (4, 200), (2, 0)]);
        assert_eq!(shape(&notes), before, "set_each");

        let mut notes = sample();
        offset(&mut notes, &all, -90);
        assert_eq!(shape(&notes), before, "offset");

        let mut notes = sample();
        scale(&mut notes, &all, 333);
        assert_eq!(shape(&notes), before, "scale");

        let mut notes = sample();
        ramp(&mut notes, &all, (q(3), 1), (q(0), 127), &Curve::S_CURVE);
        assert_eq!(shape(&notes), before, "ramp");
    }
}
