//! The step sequencer's arithmetic (M8 slice D3): a drum clip's notes read and
//! edited as a grid of steps, one row per sound. Pure functions over a clip's
//! note list, like [`super::notes`]; the app wraps each in one undo step
//! (`Document::edit_steps`).
//!
//! # A step is a window of time, not a position
//!
//! The grid is `step` ticks wide from the clip's start. A note **belongs to**
//! the step its start falls in (`floor(start / step)`), whatever its exact
//! start is, so a hit played a little late still shows in its step and is
//! never moved by being looked at: only an edit changes a note, and no edit
//! here changes a start. A step is **on** when an unmuted note of the row's
//! pitch belongs to it. A note that does not start exactly on the step's edge
//! is **off the grid**, and the view marks it.
//!
//! # Toggling gives the clip back exactly
//!
//! A click on an off step makes the hit a click always makes (**plain**: the
//! step's start, the step's length cut at the clip's end, [`STEP_VELOCITY`], no
//! mute). A click on an on step takes it away again:
//!
//! - when the step holds exactly one note and it is plain, the note is
//!   **deleted** (it is what a click made, so the clip is as it was);
//! - otherwise (an off-grid hit, a different length or velocity, two hits in a
//!   step) every note of the step is **muted** and kept: nothing the user
//!   made is lost, and the next click on the step unmutes them as they were.
//!
//! So off-then-on returns the clip's notes exactly for any step whose notes
//! are all unmuted (or all muted); and on-then-off does for an empty step. Two
//! cases are outside the promise and tested as such: a step that holds muted
//! and unmuted notes together (the click unmutes them all), and a plain note
//! the user muted by hand and then clicked on and off (the click made it look
//! like a click's own hit, so it is deleted).
//!
//! The notes stay sorted by `(start, pitch)`, so a step's notes are found by
//! two binary searches and a frame of a hundred thousand hits costs the steps
//! on the page.

use lumen_core::time::Ticks;

use super::notes::{self, MIN_NOTE};
use crate::model::Note;

/// The velocity of a hit a click makes.
pub const STEP_VELOCITY: u8 = 100;
/// The velocity an accent is, until the user sets another.
pub const ACCENT_VELOCITY: u8 = 127;

/// One step of one row: the sound's note and the step's index from the clip's
/// start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub pitch: u8,
    pub index: i64,
}

/// What the grid shows for one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    /// An unmuted note belongs to the step.
    pub on: bool,
    /// The velocity of the step's first unmuted note (0 when off).
    pub velocity: u8,
    /// An unmuted note of the step starts away from the step's edge.
    pub off_grid: bool,
    /// Only muted notes belong to the step: off, with its hit kept.
    pub held: bool,
    /// How many notes belong to the step, muted or not.
    pub hits: u16,
}

/// One change to the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A click: on to off or off to on (a step turned on has `velocity`).
    Toggle(Target, u8),
    /// Turns the step on at `velocity`, or sets the velocity of an on step.
    Set(Target, u8),
    /// Turns the step off, as a toggle of an on step does.
    Off(Target),
}

/// The step a clip time falls in, or `None` before the clip's start.
pub fn index_of(start: Ticks, step: Ticks) -> Option<i64> {
    (start >= Ticks::ZERO && step > Ticks::ZERO).then(|| start.0.div_euclid(step.0))
}

/// Where step `index` starts, in clip time.
pub fn start_of(index: i64, step: Ticks) -> Ticks {
    Ticks(index.saturating_mul(step.0))
}

/// How many steps of `step` the clip's content holds, the last one possibly
/// partial: the steps a click can turn on.
pub fn steps_in(length: Ticks, step: Ticks) -> i64 {
    if length <= Ticks::ZERO || step <= Ticks::ZERO {
        0
    } else {
        (length.0 + step.0 - 1) / step.0
    }
}

/// The hit a click makes at step `index`: what [`notes::add`] would keep.
fn plain_note(target: Target, step: Ticks, length: Ticks, velocity: u8) -> Option<Note> {
    let start = start_of(target.index, step);
    if start < Ticks::ZERO || start >= length {
        return None;
    }
    let len = step.max(MIN_NOTE).min((length - start).max(Ticks(1)));
    Some(Note(
        start,
        len,
        target.pitch.min(127),
        velocity.clamp(1, 127),
        false,
        false,
    ))
}

/// Whether `note` is the hit a click makes at its step.
fn is_plain(note: &Note, target: Target, step: Ticks, length: Ticks) -> bool {
    plain_note(target, step, length, STEP_VELOCITY).is_some_and(|plain| plain == *note)
}

/// The indices of the notes of `pitch` that belong to step `index`.
pub fn owned(notes: &[Note], target: Target, step: Ticks) -> std::ops::Range<usize> {
    let first = start_of(target.index, step);
    let end = first + step;
    let from = notes.partition_point(|n| n.start() < first);
    let to = notes.partition_point(|n| n.start() < end);
    from..to
}

fn owned_of_pitch(notes: &[Note], target: Target, step: Ticks) -> Vec<usize> {
    if step <= Ticks::ZERO || target.index < 0 {
        return Vec::new();
    }
    owned(notes, target, step)
        .filter(|&i| notes[i].pitch() == target.pitch)
        .collect()
}

/// What the grid shows for one step of the notes.
pub fn cell(notes: &[Note], target: Target, step: Ticks) -> Cell {
    let mut cell = Cell::default();
    for i in owned_of_pitch(notes, target, step) {
        cell.take(&notes[i], step);
    }
    cell
}

impl Cell {
    /// Folds in a note that belongs to the step (notes come in start order,
    /// so the first unmuted one gives the velocity).
    fn take(&mut self, note: &Note, step: Ticks) {
        self.hits = self.hits.saturating_add(1);
        if note.muted() {
            self.held = !self.on;
            return;
        }
        if !self.on {
            self.velocity = note.velocity();
        }
        self.on = true;
        self.held = false;
        if note.start().0.rem_euclid(step.0) != 0 {
            self.off_grid = true;
        }
    }
}

/// The cells of the rows `pitches` (one row each, in that order) for the
/// steps `first .. first + count`, row by row into `out`: one pass over the
/// notes that belong to those steps.
pub fn page_into(
    notes: &[Note],
    step: Ticks,
    first: i64,
    count: usize,
    pitches: &[u8],
    out: &mut Vec<Cell>,
) {
    out.clear();
    out.resize(pitches.len() * count, Cell::default());
    if step <= Ticks::ZERO || count == 0 || pitches.is_empty() {
        return;
    }
    let mut row_of = [usize::MAX; 128];
    for (row, &pitch) in pitches.iter().enumerate() {
        // The first row of a pitch wins; a map names a note once.
        if let Some(slot) = row_of.get_mut(usize::from(pitch))
            && *slot == usize::MAX
        {
            *slot = row;
        }
    }
    let from_tick = start_of(first.max(0), step);
    let to_tick = start_of(first.saturating_add(count as i64), step);
    let low = notes.partition_point(|n| n.start() < from_tick);
    let high = notes.partition_point(|n| n.start() < to_tick);
    for note in &notes[low..high] {
        let Some(row) = row_of.get(usize::from(note.pitch())).copied() else {
            continue;
        };
        if row == usize::MAX {
            continue;
        }
        let Some(index) = index_of(note.start(), step) else {
            continue;
        };
        let column = index - first;
        if (0..count as i64).contains(&column) {
            out[row * count + column as usize].take(note, step);
        }
    }
}

/// Applies `changes` in order. `length` is the clip's content length. Returns
/// the indices of the unmuted notes of the last step changed, in the list as it
/// is after the edit (the editor's selection).
pub fn apply(notes: &mut Vec<Note>, step: Ticks, length: Ticks, changes: &[Change]) -> Vec<usize> {
    let mut last = None;
    for change in changes {
        let target = match *change {
            Change::Toggle(target, velocity) => {
                toggle(notes, target, step, length, velocity);
                target
            }
            Change::Set(target, velocity) => {
                set(notes, target, step, length, velocity);
                target
            }
            Change::Off(target) => {
                turn_off(notes, target, step, length);
                target
            }
        };
        last = Some(target);
    }
    last.map_or_else(Vec::new, |target| {
        owned_of_pitch(notes, target, step)
            .into_iter()
            .filter(|&i| !notes[i].muted())
            .collect()
    })
}

fn is_on(notes: &[Note], owned: &[usize]) -> bool {
    owned.iter().any(|&i| !notes[i].muted())
}

fn toggle(notes: &mut Vec<Note>, target: Target, step: Ticks, length: Ticks, velocity: u8) {
    let owned = owned_of_pitch(notes, target, step);
    if is_on(notes, &owned) {
        turn_off(notes, target, step, length);
    } else {
        turn_on(notes, target, step, length, velocity);
    }
}

fn turn_on(notes: &mut Vec<Note>, target: Target, step: Ticks, length: Ticks, velocity: u8) {
    let owned = owned_of_pitch(notes, target, step);
    if owned.is_empty() {
        if let Some(hit) = plain_note(target, step, length, velocity) {
            notes::add(notes, hit, length);
        }
    } else {
        // The step held its hits muted: they come back as they were.
        notes::mute(notes, &owned, Some(false));
    }
}

fn turn_off(notes: &mut Vec<Note>, target: Target, step: Ticks, length: Ticks) {
    let owned = owned_of_pitch(notes, target, step);
    let live: Vec<usize> = owned
        .iter()
        .copied()
        .filter(|&i| !notes[i].muted())
        .collect();
    if live.is_empty() {
        return;
    }
    if owned.len() == 1 && is_plain(&notes[owned[0]], target, step, length) {
        notes::delete(notes, &owned);
    } else {
        notes::mute(notes, &live, Some(true));
    }
}

fn set(notes: &mut Vec<Note>, target: Target, step: Ticks, length: Ticks, velocity: u8) {
    let owned = owned_of_pitch(notes, target, step);
    match owned.iter().copied().find(|&i| !notes[i].muted()) {
        Some(first) => notes[first].3 = velocity.clamp(1, 127),
        None => turn_on(notes, target, step, length, velocity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Ticks = Ticks(44_100);
    const LEN: Ticks = Ticks(44_100 * 16);

    fn at(pitch: u8, index: i64) -> Target {
        Target { pitch, index }
    }

    fn hit(index: i64, pitch: u8) -> Note {
        plain_note(at(pitch, index), STEP, LEN, STEP_VELOCITY).expect("inside the clip")
    }

    #[test]
    fn a_note_belongs_to_the_step_its_start_falls_in() {
        let late = Note::new(Ticks(STEP.0 * 3 + 900), Ticks(100), 36, 90);
        let notes = vec![late];
        let found = cell(&notes, at(36, 3), STEP);
        assert!(found.on && found.off_grid, "shown at its step, marked");
        assert_eq!(found.velocity, 90);
        assert!(!cell(&notes, at(36, 4), STEP).on);
        assert!(!cell(&notes, at(38, 3), STEP).on, "another sound's row");
        let exact = vec![hit(3, 36)];
        let found = cell(&exact, at(36, 3), STEP);
        assert!(found.on && !found.off_grid);
    }

    #[test]
    fn a_click_makes_the_plain_hit_and_a_second_click_takes_it_away() {
        let mut notes = Vec::new();
        apply(
            &mut notes,
            STEP,
            LEN,
            &[Change::Toggle(at(36, 2), STEP_VELOCITY)],
        );
        assert_eq!(notes, vec![hit(2, 36)]);
        apply(
            &mut notes,
            STEP,
            LEN,
            &[Change::Toggle(at(36, 2), STEP_VELOCITY)],
        );
        assert!(
            notes.is_empty(),
            "the plain hit is deleted: the clip is as it was"
        );
    }

    #[test]
    fn a_hit_that_is_not_plain_is_muted_and_comes_back_as_it_was() {
        let off_grid = Note::new(Ticks(STEP.0 * 5 + 700), Ticks(1_000), 38, 64);
        let mut notes = vec![off_grid];
        let before = notes.clone();
        apply(&mut notes, STEP, LEN, &[Change::Off(at(38, 5))]);
        assert_eq!(notes.len(), 1, "nothing is lost");
        assert!(notes[0].muted());
        assert!(!cell(&notes, at(38, 5), STEP).on);
        assert!(cell(&notes, at(38, 5), STEP).held);
        apply(
            &mut notes,
            STEP,
            LEN,
            &[Change::Toggle(at(38, 5), STEP_VELOCITY)],
        );
        assert_eq!(notes, before, "unmuted exactly as it was");
    }

    #[test]
    fn the_last_step_is_cut_at_the_clips_end() {
        let length = Ticks(STEP.0 * 3 + 1_000);
        let mut notes = Vec::new();
        apply(
            &mut notes,
            STEP,
            length,
            &[Change::Toggle(at(36, 3), STEP_VELOCITY)],
        );
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].length(), Ticks(1_000));
        apply(
            &mut notes,
            STEP,
            length,
            &[Change::Toggle(at(36, 3), STEP_VELOCITY)],
        );
        assert!(notes.is_empty(), "a cut hit is still plain");
        // A step past the end cannot be turned on.
        apply(
            &mut notes,
            STEP,
            length,
            &[Change::Toggle(at(36, 4), STEP_VELOCITY)],
        );
        assert!(notes.is_empty());
        assert_eq!(steps_in(length, STEP), 4);
    }

    #[test]
    fn set_gives_the_first_unmuted_hit_the_velocity_or_turns_the_step_on() {
        let mut notes = vec![hit(1, 36)];
        apply(&mut notes, STEP, LEN, &[Change::Set(at(36, 1), 40)]);
        assert_eq!(notes[0].velocity(), 40);
        assert_eq!(notes.len(), 1);
        apply(&mut notes, STEP, LEN, &[Change::Set(at(36, 2), 127)]);
        assert_eq!(notes.len(), 2);
        assert_eq!(cell(&notes, at(36, 2), STEP).velocity, 127);
        apply(&mut notes, STEP, LEN, &[Change::Set(at(36, 2), 0)]);
        assert_eq!(
            cell(&notes, at(36, 2), STEP).velocity,
            1,
            "a velocity is 1 to 127"
        );
    }

    #[test]
    fn a_page_gathers_the_cells_of_its_steps_only() {
        let notes = vec![
            hit(0, 36),
            hit(2, 38),
            hit(2, 42),
            Note::new(Ticks(STEP.0 * 17), STEP, 36, STEP_VELOCITY),
        ];
        let mut out = Vec::new();
        page_into(&notes, STEP, 0, 16, &[36, 38], &mut out);
        assert_eq!(out.len(), 32);
        assert!(out[0].on, "kick on step 0");
        assert!(out[16 + 2].on, "snare on step 2 of row 1");
        assert!(!out[17].on, "step 17 is on the next page");
        assert!(
            out.iter().filter(|c| c.on).count() == 2,
            "a sound the rows do not name is left out"
        );
        page_into(&notes, STEP, 16, 16, &[36], &mut out);
        assert!(out[1].on);
        assert_eq!(out.iter().filter(|c| c.on).count(), 1);
    }

    /// A small deterministic generator: the property tests need no crate.
    struct Rolls(u64);
    impl Rolls {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn random_notes(rolls: &mut Rolls) -> Vec<Note> {
        let mut notes: Vec<Note> = (0..rolls.below(60))
            .map(|_| {
                let index = rolls.below(18) as i64;
                let pitch = [36, 38, 42][rolls.below(3) as usize];
                let shape = rolls.below(6);
                let mut note = hit(index.min(15), pitch);
                match shape {
                    0 => note.0 = Ticks(note.0.0 + 1 + rolls.below(STEP.0 as u64 - 1) as i64),
                    1 => note.1 = Ticks(500 + rolls.below(30_000) as i64),
                    2 => note.3 = 1 + rolls.below(127) as u8,
                    3 => note.4 = true,
                    _ => {}
                }
                note
            })
            .collect();
        notes.sort_by_key(|n| (n.start(), n.pitch()));
        notes
    }

    /// The step's notes mix muted and unmuted (the promise does not cover it).
    fn mixed(notes: &[Note], target: Target) -> bool {
        let owned = owned_of_pitch(notes, target, STEP);
        owned.iter().any(|&i| notes[i].muted()) && owned.iter().any(|&i| !notes[i].muted())
    }

    #[test]
    fn toggling_a_step_off_and_on_again_returns_the_notes_exactly() {
        let mut rolls = Rolls(0x9e37_79b9_7f4a_7c15);
        let mut checked = 0;
        for _ in 0..400 {
            let original = random_notes(&mut rolls);
            let target = at(
                [36, 38, 42][rolls.below(3) as usize],
                rolls.below(16) as i64,
            );
            if mixed(&original, target) {
                continue;
            }
            let mut notes = original.clone();
            let was_on = cell(&notes, target, STEP).on;
            apply(
                &mut notes,
                STEP,
                LEN,
                &[
                    Change::Toggle(target, STEP_VELOCITY),
                    Change::Toggle(target, STEP_VELOCITY),
                ],
            );
            // Two clicks always return to where they began: an on step goes
            // off and on, an off step on and off.
            if was_on {
                assert_eq!(notes, original, "off then on: {target:?}");
                checked += 1;
            } else if owned_of_pitch(&original, target, STEP).is_empty() {
                assert_eq!(notes, original, "on then off: {target:?}");
                checked += 1;
            }
        }
        assert!(checked > 100, "the run checked {checked} steps");
    }

    #[test]
    fn a_run_of_toggles_changes_only_the_steps_it_names() {
        let mut rolls = Rolls(0x1234_5678_9abc_def1);
        for round in 0..200 {
            let original = random_notes(&mut rolls);
            let mut notes = original.clone();
            let target = at(
                [36, 38, 42][rolls.below(3) as usize],
                rolls.below(16) as i64,
            );
            apply(
                &mut notes,
                STEP,
                LEN,
                &[Change::Toggle(target, STEP_VELOCITY)],
            );
            let elsewhere = |list: &[Note]| -> Vec<Note> {
                let owned = owned_of_pitch(list, target, STEP);
                list.iter()
                    .enumerate()
                    .filter(|(i, _)| !owned.contains(i))
                    .map(|(_, n)| *n)
                    .collect()
            };
            assert_eq!(
                elsewhere(&notes),
                elsewhere(&original),
                "round {round}: no other step moved"
            );
            assert!(
                notes.is_sorted_by_key(|n| (n.start(), n.pitch())),
                "round {round}: the list stays sorted"
            );
        }
    }

    #[test]
    fn viewing_never_moves_a_note() {
        // `page_into` and `cell` take the notes by shared reference: a view
        // cannot change them. What it must do is show an off-grid hit.
        let late = Note::new(Ticks(STEP.0 * 2 + 5), Ticks(300), 36, 80);
        let notes = vec![late];
        let mut out = Vec::new();
        page_into(&notes, STEP, 0, 16, &[36], &mut out);
        assert!(out[2].on && out[2].off_grid);
        assert_eq!(notes[0].start(), Ticks(STEP.0 * 2 + 5));
    }
}
