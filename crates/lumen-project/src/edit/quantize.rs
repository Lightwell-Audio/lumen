//! Quantize (M5 slice 5a): snapping notes to a grid, as a pure function over
//! a clip's note list. The piano roll wraps it in one undo step, so it is
//! tested here without a window.
//!
//! # The grid
//!
//! Grid line `i` sits at `i * step`. With swing, every odd line is delayed by
//! `swing * step / 2`, so the grid stays in order for any swing up to 1.
//! A position's target is its nearest line, with ties going to the later
//! line (Ableton: a note exactly halfway moves forward). A position is pulled toward its target by `strength` only when the
//! distance to it is within `window * step / 2` (plus one tick, for rounding).
//! A window of 1.0 is off: every position is pulled.
//!
//! # Modes
//!
//! - `Start`: the start is pulled; the length stays.
//! - `End`: the end is pulled; the start stays, so the length changes. If the
//!   nearest line is at or before the start, the end goes to the first line
//!   after the start, so a note never collapses.
//! - `StartAndEnd`: Ableton's order: the start is pulled first (the length
//!   stays), then the end of the note as it now is is pulled to its nearest
//!   line. Two adjacent notes therefore stay adjacent.
//! - `Length`: the length moves toward the nearest whole number of steps
//!   (halves round up, at least one step). The start stays; swing and the
//!   window do not apply.
//!
//! # Keeping notes whole
//!
//! A note keeps its pitch, velocity and mute. Its start stays in
//! `[0, length - MIN_NOTE]` and its end at most at the clip's content length
//! `length` (or its old end, when it already ran past it). Its length is at
//! least [`MIN_NOTE`].
//!
//! Only the picked notes change, and the selection contract of `notes.rs`
//! applies: the result is the picked notes' indices afterwards, moved or not,
//! as [`super::notes::legato`] returns them. A step of zero or less changes
//! nothing. Controller events are not touched.

use lumen_core::time::{PPQ, Ticks};

use super::notes::{MIN_NOTE, finish, picked, tag};
use crate::model::Note;

/// Which part of a note quantize moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The start moves; the length stays.
    #[default]
    Start,
    /// The end moves; the start stays, so the length changes.
    End,
    /// Both edges move, each to its own nearest line.
    StartAndEnd,
    /// The length moves toward a whole number of steps.
    Length,
}

/// What quantize snaps to and how hard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The grid's spacing: a 1/16 is `PPQ / 4`.
    pub step: Ticks,
    /// Which part of a note moves.
    pub mode: Mode,
    /// How far a note moves toward its line, 0 to 1 (clamped).
    pub strength: f32,
    /// How far every odd grid line is delayed, 0 to 1 of half a step (clamped).
    pub swing: f32,
    /// How far from a line a note may be to be pulled, 0 to 1 of half a
    /// step (clamped). 1.0 is off.
    pub window: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            step: Ticks(PPQ / 4),
            mode: Mode::Start,
            strength: 1.0,
            swing: 0.0,
            window: 1.0,
        }
    }
}

/// Quantizes the picked notes of `notes` (a clip of content `length`) to the
/// grid in `s`. Returns the picked notes' indices in the list as it is after
/// the edit.
pub fn quantize(
    notes: &mut Vec<Note>,
    selection: &[usize],
    length: Ticks,
    s: &Settings,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    if picked.is_empty() {
        return Vec::new();
    }
    if s.step.0 <= 0 {
        return picked;
    }
    let grid = Grid::new(s);
    let mut tagged = tag(notes, &picked);
    for (note, chosen) in &mut tagged {
        if *chosen {
            *note = quantized(*note, length, &grid, s.mode);
        }
    }
    finish(notes, tagged)
}

/// The settings as the arithmetic uses them, in ticks.
struct Grid {
    step: i64,
    /// How far an odd line is delayed.
    delay: i64,
    /// The pull range in ticks, or `None` when the window is off.
    range: Option<f64>,
    strength: f64,
}

impl Grid {
    fn new(s: &Settings) -> Self {
        let step = s.step.0;
        let swing = unit(s.swing, 0.0);
        let window = unit(s.window, 1.0);
        Self {
            step,
            delay: (swing * step as f64 / 2.0).round() as i64,
            range: (window < 1.0).then(|| window * step as f64 / 2.0 + 1.0),
            strength: unit(s.strength, 0.0),
        }
    }

    /// Where grid line `k` sits.
    fn line(&self, k: i64) -> i64 {
        k * self.step + if k.rem_euclid(2) == 1 { self.delay } else { 0 }
    }

    /// The nearest line to `p`; a tie goes to the later line (Ableton).
    fn nearest(&self, p: i64) -> i64 {
        let cell = (2 * p + self.step).div_euclid(2 * self.step);
        (cell - 1..=cell + 1)
            .map(|k| self.line(k))
            .min_by_key(|line| ((line - p).abs(), -line))
            .unwrap_or(p)
    }

    /// The first line after `p`.
    fn line_after(&self, p: i64) -> i64 {
        // Lines only ever move forward (a swing delay is at most half a
        // step), so the search ends within a few lines.
        let mut k = p.div_euclid(self.step) - 1;
        while self.line(k) <= p {
            k += 1;
        }
        self.line(k)
    }

    /// The line `p` is pulled toward, or `None` when it is outside the
    /// window.
    fn target(&self, p: i64) -> Option<i64> {
        let line = self.nearest(p);
        match self.range {
            Some(range) if ((line - p).abs() as f64) > range => None,
            _ => Some(line),
        }
    }

    /// `p` pulled toward its nearest line, or `p` itself when it is outside
    /// the window.
    fn pull(&self, p: i64) -> i64 {
        toward(p, self.target(p).unwrap_or(p), self.strength)
    }

    /// A note end pulled toward its line, for a note that starts at `start`.
    /// A line at or before the start would collapse the note, so the end goes
    /// to the first line after the start instead.
    fn pull_end(&self, end: i64, start: i64) -> i64 {
        let Some(line) = self.target(end) else {
            return end;
        };
        let line = if line <= start {
            self.line_after(start)
        } else {
            line
        };
        toward(end, line, self.strength)
    }
}

/// A setting clamped to 0 to 1; `nan` stands in for a NaN.
fn unit(value: f32, nan: f64) -> f64 {
    if value.is_nan() {
        nan
    } else {
        f64::from(value.clamp(0.0, 1.0))
    }
}

/// `from` moved `strength` of the way to `to`, in whole ticks (halves away
/// from zero).
fn toward(from: i64, to: i64, strength: f64) -> i64 {
    (from as f64 + (to - from) as f64 * strength).round() as i64
}

/// The nearest whole number of steps to `len` (halves round up), and at
/// least one step.
fn whole_steps(len: i64, step: i64) -> i64 {
    let n = (2 * len.max(0) + step) / (2 * step);
    (n * step).max(step)
}

/// Keeps a note inside the content: start in `[0, length - MIN_NOTE]`, end at
/// most `length` (or `old_end`, when the note already ran past it), length at
/// least `MIN_NOTE`. Returns the start and the length.
fn place(start: i64, end: i64, length: Ticks, old_end: Ticks) -> (Ticks, Ticks) {
    let min = MIN_NOTE.0;
    let start = start.clamp(0, (length.0 - min).max(0));
    let limit = length.0.max(old_end.0);
    let end = end.min(limit).max(start + min);
    (Ticks(start), Ticks(end - start))
}

/// One note moved by `mode`.
fn quantized(note: Note, length: Ticks, grid: &Grid, mode: Mode) -> Note {
    let (start, end, len) = (note.start().0, note.end().0, note.length().0);
    let (start, end) = match mode {
        Mode::Start => {
            let start = grid.pull(start);
            (start, start + len)
        }
        Mode::End => (start, grid.pull_end(end, start)),
        Mode::StartAndEnd => {
            // Start first, keeping the length; then the end as it now is.
            let start = grid.pull(start);
            (start, grid.pull_end(start + len, start))
        }
        Mode::Length => {
            let target = whole_steps(len, grid.step);
            (start, start + toward(len, target, grid.strength))
        }
    };
    let (start, len) = place(start, end, length, note.end());
    Note(start, len, note.pitch(), note.velocity(), note.muted())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sixteenth note in ticks.
    const S: i64 = PPQ / 4;
    const LEN: Ticks = Ticks(PPQ * 16);

    fn beat(n: i64) -> i64 {
        PPQ * n
    }

    fn note(start: i64, len: i64, pitch: u8) -> Note {
        Note::new(Ticks(start), Ticks(len), pitch, 100)
    }

    fn mode(mode: Mode) -> Settings {
        Settings {
            mode,
            ..Settings::default()
        }
    }

    /// Quantizes one note in a clip of `LEN` and returns it.
    fn one(note: Note, s: &Settings) -> Note {
        let mut notes = vec![note];
        quantize(&mut notes, &[0], LEN, s);
        notes[0]
    }

    #[test]
    fn start_mode_moves_the_start_and_keeps_the_length() {
        // 30000 ticks after beat 2 is nearest the line at beat 2.
        let n = one(note(beat(2) + 30_000, beat(1), 60), &mode(Mode::Start));
        assert_eq!(n, note(beat(2), beat(1), 60));
    }

    #[test]
    fn end_mode_moves_the_end_and_changes_the_length() {
        // The end is at beat 2 + 30000, so it snaps to beat 2.
        let n = one(note(beat(1), beat(1) + 30_000, 60), &mode(Mode::End));
        assert_eq!(n, note(beat(1), beat(1), 60));
    }

    #[test]
    fn start_and_end_mode_moves_both_edges() {
        // Start 1 + 40000 snaps to beat 1, end 3 - 50000 to beat 3.
        let n = one(
            note(beat(1) + 40_000, 2 * beat(1) - 90_000, 60),
            &mode(Mode::StartAndEnd),
        );
        assert_eq!(n, note(beat(1), 2 * beat(1), 60));
    }

    #[test]
    fn length_mode_snaps_the_length_and_keeps_the_start() {
        // 1.3 beats is 5.2 sixteenths: five sixteenths, which is 1.25 beats.
        let n = one(note(beat(1), 1_834_560, 60), &mode(Mode::Length));
        assert_eq!(n, note(beat(1), 5 * PPQ / 4, 60));
        // Half a step rounds up to a whole step.
        let n = one(note(0, S / 2, 60), &mode(Mode::Length));
        assert_eq!(n, note(0, S, 60));
    }

    #[test]
    fn length_mode_ignores_swing_and_the_window() {
        let s = Settings {
            mode: Mode::Length,
            swing: 1.0,
            window: 0.1,
            ..Settings::default()
        };
        let n = one(note(beat(1) + 40_000, 1_834_560, 60), &s);
        assert_eq!(n, note(beat(1) + 40_000, 5 * PPQ / 4, 60));
    }

    #[test]
    fn strength_is_the_fraction_of_the_way_to_the_line() {
        // 40000 ticks away, half strength: 20000 ticks away.
        let s = Settings {
            strength: 0.5,
            ..Settings::default()
        };
        let n = one(note(beat(1) + 40_000, beat(1), 60), &s);
        assert_eq!(n, note(beat(1) + 20_000, beat(1), 60));
    }

    #[test]
    fn zero_strength_changes_nothing_and_still_returns_the_picked_notes() {
        let s0 = Settings {
            strength: 0.0,
            ..Settings::default()
        };
        for m in [Mode::Start, Mode::End, Mode::StartAndEnd, Mode::Length] {
            let original = note(beat(1) + 40_000, beat(1) + 30_000, 60);
            let mut notes = vec![original];
            let s = Settings { mode: m, ..s0 };
            assert_eq!(quantize(&mut notes, &[0], LEN, &s), [0]);
            assert_eq!(notes[0], original, "{m:?}");
        }
    }

    #[test]
    fn out_of_range_settings_are_clamped() {
        let off_grid = note(beat(1) + 40_000, beat(1), 60);
        let snapped = note(beat(1), beat(1), 60);
        let strong = Settings {
            strength: 2.0,
            ..Settings::default()
        };
        assert_eq!(one(off_grid, &strong), snapped);
        let swing = Settings {
            swing: -1.0,
            ..Settings::default()
        };
        assert_eq!(one(off_grid, &swing), snapped);
        let window = Settings {
            window: 5.0,
            ..Settings::default()
        };
        assert_eq!(one(off_grid, &window), snapped);
    }

    #[test]
    fn full_swing_delays_odd_lines_by_half_a_step() {
        let swing = Settings {
            swing: 1.0,
            ..Settings::default()
        };
        // Line 5 (1.25 beats) is odd: at full swing it sits at 1.25 + 1/8
        // beats, so a note on it moves by half a step.
        let n = one(note(5 * S, PPQ / 8, 60), &swing);
        assert_eq!(n, note(5 * S + S / 2, PPQ / 8, 60));
        assert_eq!(n.start(), Ticks(1_940_400));
        // Even lines do not move.
        let n = one(note(beat(2), PPQ / 8, 60), &swing);
        assert_eq!(n.start(), Ticks(beat(2)));
    }

    #[test]
    fn window_leaves_far_notes_and_pulls_near_ones() {
        // The window is half of a half step: 0.5 of a step / 2 = 88200 ticks.
        let s = Settings {
            window: 0.5,
            ..Settings::default()
        };
        let far = note(beat(2) + 141_120, PPQ / 8, 60); // 0.4 steps from beat 2
        let near = note(beat(3) + 70_560, PPQ / 8, 62); // 0.2 steps from beat 3
        let mut notes = vec![far, near];
        assert_eq!(quantize(&mut notes, &[0, 1], LEN, &s), [0, 1]);
        assert_eq!(notes[0], far);
        assert_eq!(notes[1], note(beat(3), PPQ / 8, 62));
    }

    #[test]
    fn the_window_has_one_tick_of_tolerance() {
        // Its range is 88200 ticks, plus one: 88201 is pulled, 88202 is not.
        let s = Settings {
            window: 0.5,
            ..Settings::default()
        };
        assert_eq!(
            one(note(beat(1) + 88_201, PPQ / 8, 60), &s).start(),
            Ticks(beat(1))
        );
        assert_eq!(
            one(note(beat(3) + 88_202, PPQ / 8, 60), &s).start(),
            Ticks(beat(3) + 88_202)
        );
    }

    #[test]
    fn a_triplet_grid_snaps_to_triplet_lines() {
        // An eighth-note triplet is a third of a beat: 470400 ticks.
        let triplet = Settings {
            step: Ticks::whole_note_fraction(1, 12).unwrap(),
            ..Settings::default()
        };
        let t = PPQ / 3;
        assert_eq!(t, 470_400);
        // 1000 ticks after the first triplet line.
        let n = one(note(t + 1_000, PPQ / 10, 60), &triplet);
        assert_eq!(n.start(), Ticks(t));
        // A straight eighth (half a beat) is equally near the first and
        // second triplet lines: the later one wins (Ableton).
        let n = one(note(PPQ / 2, PPQ / 10, 60), &triplet);
        assert_eq!(n.start(), Ticks(2 * t));
    }

    #[test]
    fn unpicked_notes_and_pitch_velocity_and_mute_are_kept() {
        let picked_note = Note(Ticks(beat(1) + 40_000), Ticks(beat(1)), 61, 33, true);
        let other = Note(Ticks(beat(2) + 40_000), Ticks(beat(1)), 62, 90, false);
        let mut notes = vec![picked_note, other];
        assert_eq!(quantize(&mut notes, &[0], LEN, &mode(Mode::Start)), [0]);
        assert_eq!(notes[0], Note(Ticks(beat(1)), Ticks(beat(1)), 61, 33, true));
        assert_eq!(notes[1], other);
    }

    #[test]
    fn a_note_near_the_end_stays_inside_the_clip() {
        // 50000 ticks before the end of a 4-beat clip, the nearest line is the
        // end itself: the start stops one minimum note short of it.
        let length = Ticks(beat(4));
        let min = MIN_NOTE.0;
        let mut notes = vec![note(beat(4) - 50_000, 50_000, 60)];
        quantize(&mut notes, &[0], length, &mode(Mode::Start));
        assert_eq!(notes[0], note(beat(4) - min, min, 60));
        assert_eq!(notes[0].end(), length);
    }

    #[test]
    fn empty_and_out_of_range_selections_change_nothing() {
        let original = note(beat(1) + 40_000, beat(1), 60);
        let mut notes = vec![original];
        assert!(quantize(&mut notes, &[], LEN, &Settings::default()).is_empty());
        assert!(quantize(&mut notes, &[7], LEN, &Settings::default()).is_empty());
        assert_eq!(notes, [original]);
    }

    #[test]
    fn a_step_of_zero_or_less_changes_nothing() {
        for step in [0, -5] {
            let s = Settings {
                step: Ticks(step),
                ..Settings::default()
            };
            let original = note(beat(1) + 40_000, beat(1), 60);
            let mut notes = vec![original];
            assert_eq!(quantize(&mut notes, &[0], LEN, &s), [0]);
            assert_eq!(notes, [original]);
        }
    }

    #[test]
    fn returned_indices_follow_notes_that_cross() {
        // A (pitch 72) moves later onto the line at S, B (pitch 48) moves
        // earlier onto it: B now sorts before A. D (pitch 50) is unpicked and
        // already on the line.
        let mut notes = vec![
            note(S - 100, PPQ / 8, 72),
            note(S, PPQ / 8, 50),
            note(S + 200, PPQ / 8, 48),
        ];
        let sel = quantize(&mut notes, &[0, 2], LEN, &mode(Mode::Start));
        assert_eq!(sel, [0, 2]);
        assert_eq!(notes[sel[0]].pitch(), 48);
        assert_eq!(notes[sel[1]].pitch(), 72);
        assert_eq!(notes[1].pitch(), 50);
        assert!(notes.iter().all(|n| n.start() == Ticks(S)));
    }

    // The owner's test in Ableton (two adjacent eighth notes, grid 1/4): the
    // first starts on beat 3 (1.3), the second half a beat later (1.3.3),
    // exactly between lines 3 and 4. Positions are in beats from the start.
    fn pair() -> Vec<Note> {
        vec![
            Note::new(Ticks(beat(2)), Ticks(PPQ / 2), 60, 100),
            Note::new(Ticks(beat(2) + PPQ / 2), Ticks(PPQ / 2), 64, 100),
        ]
    }

    fn quarters(mode: Mode) -> Settings {
        Settings {
            step: Ticks(PPQ),
            mode,
            ..Settings::default()
        }
    }

    #[test]
    fn ableton_start_keeps_the_first_note_and_moves_the_halfway_note_forward() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::Start));
        assert_eq!(notes[0].start(), Ticks(beat(2)));
        assert_eq!(notes[1].start(), Ticks(beat(3)));
        assert_eq!(notes[1].length(), Ticks(PPQ / 2));
    }

    #[test]
    fn ableton_end_stretches_the_first_note_and_leaves_the_second() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::End));
        // 1.3 to 1.4, and the second note already ends on the grid.
        assert_eq!(notes[0].start(), Ticks(beat(2)));
        assert_eq!(notes[0].end(), Ticks(beat(3)));
        assert_eq!(notes[1].start(), Ticks(beat(2) + PPQ / 2));
        assert_eq!(notes[1].end(), Ticks(beat(3)));
    }

    #[test]
    fn ableton_both_does_start_then_end_and_the_notes_stay_adjacent() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::StartAndEnd));
        // First 1.3 to 1.4; second starts on 1.4 (start step, length kept)
        // then its end, now half a beat past 1.4, goes up to 2.1.
        assert_eq!(
            (notes[0].start(), notes[0].end()),
            (Ticks(beat(2)), Ticks(beat(3)))
        );
        assert_eq!(
            (notes[1].start(), notes[1].end()),
            (Ticks(beat(3)), Ticks(beat(4)))
        );
    }

    // A note shorter than the grid, played loose: start a little after
    // line 1 (S), its end 50 ticks later.
    fn short() -> Note {
        note(S + 100, 50, 60)
    }

    #[test]
    fn an_end_that_would_land_before_the_start_goes_to_the_next_line() {
        let mut notes = vec![short()];
        quantize(&mut notes, &[0], LEN, &mode(Mode::End));
        // Its nearest line (S) is before the start: the end takes 2S instead
        // of leaving a hairline.
        assert_eq!(notes[0], note(S + 100, S - 100, 60));
    }

    #[test]
    fn an_end_already_on_the_grid_is_left_alone() {
        let mut notes = vec![note(S + 100, S - 100, 60)];
        quantize(&mut notes, &[0], LEN, &mode(Mode::End));
        assert_eq!(notes[0], note(S + 100, S - 100, 60));
    }

    #[test]
    fn both_mode_turns_a_short_note_into_one_step_on_the_grid() {
        let mut notes = vec![short()];
        quantize(&mut notes, &[0], LEN, &mode(Mode::StartAndEnd));
        // Start to S (length 50 kept, end S+50 would snap back to S), so the
        // end takes the next line: 2S.
        assert_eq!(notes[0], note(S, S, 60));
    }

    #[test]
    fn a_partial_strength_moves_start_then_end_part_of_the_way() {
        let mut notes = vec![short()];
        let half = Settings {
            mode: Mode::StartAndEnd,
            strength: 0.5,
            ..Settings::default()
        };
        quantize(&mut notes, &[0], LEN, &half);
        // Start halfway from S+100 to S; the end (S+100 after that) halfway
        // to 2S, the line after the start.
        assert_eq!(notes[0].start(), Ticks(S + 50));
        assert_eq!(notes[0].end(), Ticks((S + 100 + 2 * S) / 2));
    }

    #[test]
    fn a_short_note_outside_the_window_is_left_alone_in_end_mode() {
        let mut notes = vec![note(S + 100, S / 2 - 10, 60)];
        let narrow = Settings {
            mode: Mode::End,
            window: 0.2,
            ..Settings::default()
        };
        // Its end (S + S/2 + 90) is far from any line: untouched.
        quantize(&mut notes, &[0], LEN, &narrow);
        assert_eq!(notes[0], note(S + 100, S / 2 - 10, 60));
    }
}
