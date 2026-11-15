use lumen_core::time::{PPQ, Ticks};

use super::notes::{MIN_NOTE, finish, picked, tag};
use crate::model::Note;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Start,
    End,
    StartAndEnd,
    Length,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub step: Ticks,
    pub mode: Mode,
    pub strength: f32,
    pub swing: f32,
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

pub fn quantize_each(
    notes: &mut Vec<Note>,
    selection: &[usize],
    length: Ticks,
    s: &Settings,
    step_of: &dyn Fn(u8) -> Option<Ticks>,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    if picked.is_empty() {
        return Vec::new();
    }
    if s.step.0 <= 0 {
        return picked;
    }
    let mut grids: Vec<(i64, Grid)> = vec![(s.step.0, Grid::new(s))];
    let mut tagged = tag(notes, &picked);
    for (note, chosen) in &mut tagged {
        if !*chosen {
            continue;
        }
        let step = step_of(note.pitch()).unwrap_or(s.step);
        if step.0 <= 0 {
            continue;
        }
        if !grids.iter().any(|(ticks, _)| *ticks == step.0) {
            grids.push((step.0, Grid::new(&Settings { step, ..*s })));
        }
        let Some((_, grid)) = grids.iter().find(|(ticks, _)| *ticks == step.0) else {
            continue;
        };
        *note = quantized(*note, length, grid, s.mode);
    }
    finish(notes, tagged)
}

struct Grid {
    step: i64,
    delay: i64,
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

    fn line(&self, k: i64) -> i64 {
        k * self.step + if k.rem_euclid(2) == 1 { self.delay } else { 0 }
    }

    fn nearest(&self, p: i64) -> i64 {
        let cell = (2 * p + self.step).div_euclid(2 * self.step);
        (cell - 1..=cell + 1)
            .map(|k| self.line(k))
            .min_by_key(|line| ((line - p).abs(), -line))
            .unwrap_or(p)
    }

    fn line_after(&self, p: i64) -> i64 {
        let mut k = p.div_euclid(self.step) - 1;
        while self.line(k) <= p {
            k += 1;
        }
        self.line(k)
    }

    fn target(&self, p: i64) -> Option<i64> {
        let line = self.nearest(p);
        match self.range {
            Some(range) if ((line - p).abs() as f64) > range => None,
            _ => Some(line),
        }
    }

    fn pull(&self, p: i64) -> i64 {
        toward(p, self.target(p).unwrap_or(p), self.strength)
    }

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

fn unit(value: f32, nan: f64) -> f64 {
    if value.is_nan() {
        nan
    } else {
        f64::from(value.clamp(0.0, 1.0))
    }
}

fn toward(from: i64, to: i64, strength: f64) -> i64 {
    (from as f64 + (to - from) as f64 * strength).round() as i64
}

fn whole_steps(len: i64, step: i64) -> i64 {
    let n = (2 * len.max(0) + step) / (2 * step);
    (n * step).max(step)
}

fn place(start: i64, end: i64, length: Ticks, old_end: Ticks) -> (Ticks, Ticks) {
    let min = MIN_NOTE.0;
    let start = start.clamp(0, (length.0 - min).max(0));
    let limit = length.0.max(old_end.0);
    let end = end.min(limit).max(start + min);
    (Ticks(start), Ticks(end - start))
}

fn quantized(note: Note, length: Ticks, grid: &Grid, mode: Mode) -> Note {
    let (start, end, len) = (note.start().0, note.end().0, note.length().0);
    let (start, end) = match mode {
        Mode::Start => {
            let start = grid.pull(start);
            (start, start + len)
        }
        Mode::End => (start, grid.pull_end(end, start)),
        Mode::StartAndEnd => {
            let start = grid.pull(start);
            (start, grid.pull_end(start + len, start))
        }
        Mode::Length => {
            let target = whole_steps(len, grid.step);
            (start, start + toward(len, target, grid.strength))
        }
    };
    let (start, len) = place(start, end, length, note.end());
    Note(
        start,
        len,
        note.pitch(),
        note.velocity(),
        note.muted(),
        note.carries_on(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn one(note: Note, s: &Settings) -> Note {
        let mut notes = vec![note];
        quantize(&mut notes, &[0], LEN, s);
        notes[0]
    }

    #[test]
    fn start_mode_moves_the_start_and_keeps_the_length() {
        let n = one(note(beat(2) + 30_000, beat(1), 60), &mode(Mode::Start));
        assert_eq!(n, note(beat(2), beat(1), 60));
    }

    #[test]
    fn end_mode_moves_the_end_and_changes_the_length() {
        let n = one(note(beat(1), beat(1) + 30_000, 60), &mode(Mode::End));
        assert_eq!(n, note(beat(1), beat(1), 60));
    }

    #[test]
    fn start_and_end_mode_moves_both_edges() {
        let n = one(
            note(beat(1) + 40_000, 2 * beat(1) - 90_000, 60),
            &mode(Mode::StartAndEnd),
        );
        assert_eq!(n, note(beat(1), 2 * beat(1), 60));
    }

    #[test]
    fn length_mode_snaps_the_length_and_keeps_the_start() {
        let n = one(note(beat(1), 1_834_560, 60), &mode(Mode::Length));
        assert_eq!(n, note(beat(1), 5 * PPQ / 4, 60));
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
        let n = one(note(5 * S, PPQ / 8, 60), &swing);
        assert_eq!(n, note(5 * S + S / 2, PPQ / 8, 60));
        assert_eq!(n.start(), Ticks(1_940_400));
        let n = one(note(beat(2), PPQ / 8, 60), &swing);
        assert_eq!(n.start(), Ticks(beat(2)));
    }

    #[test]
    fn window_leaves_far_notes_and_pulls_near_ones() {
        let s = Settings {
            window: 0.5,
            ..Settings::default()
        };
        let far = note(beat(2) + 141_120, PPQ / 8, 60);
        let near = note(beat(3) + 70_560, PPQ / 8, 62);
        let mut notes = vec![far, near];
        assert_eq!(quantize(&mut notes, &[0, 1], LEN, &s), [0, 1]);
        assert_eq!(notes[0], far);
        assert_eq!(notes[1], note(beat(3), PPQ / 8, 62));
    }

    #[test]
    fn the_window_has_one_tick_of_tolerance() {
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
        let triplet = Settings {
            step: Ticks::whole_note_fraction(1, 12).unwrap(),
            ..Settings::default()
        };
        let t = PPQ / 3;
        assert_eq!(t, 470_400);
        let n = one(note(t + 1_000, PPQ / 10, 60), &triplet);
        assert_eq!(n.start(), Ticks(t));
        let n = one(note(PPQ / 2, PPQ / 10, 60), &triplet);
        assert_eq!(n.start(), Ticks(2 * t));
    }

    #[test]
    fn unpicked_notes_and_pitch_velocity_and_mute_are_kept() {
        let picked_note = Note(Ticks(beat(1) + 40_000), Ticks(beat(1)), 61, 33, true, true);
        let other = Note(Ticks(beat(2) + 40_000), Ticks(beat(1)), 62, 90, false, true);
        let mut notes = vec![picked_note, other];
        assert_eq!(quantize(&mut notes, &[0], LEN, &mode(Mode::Start)), [0]);
        assert_eq!(
            notes[0],
            Note(Ticks(beat(1)), Ticks(beat(1)), 61, 33, true, true)
        );
        assert_eq!(notes[1], other);
    }

    #[test]
    fn a_note_near_the_end_stays_inside_the_clip() {
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
    fn pair_start_keeps_the_first_note_and_moves_the_halfway_note_forward() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::Start));
        assert_eq!(notes[0].start(), Ticks(beat(2)));
        assert_eq!(notes[1].start(), Ticks(beat(3)));
        assert_eq!(notes[1].length(), Ticks(PPQ / 2));
    }

    #[test]
    fn pair_end_stretches_the_first_note_and_leaves_the_second() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::End));
        assert_eq!(notes[0].start(), Ticks(beat(2)));
        assert_eq!(notes[0].end(), Ticks(beat(3)));
        assert_eq!(notes[1].start(), Ticks(beat(2) + PPQ / 2));
        assert_eq!(notes[1].end(), Ticks(beat(3)));
    }

    #[test]
    fn pair_both_does_start_then_end_and_the_notes_stay_adjacent() {
        let mut notes = pair();
        quantize(&mut notes, &[0, 1], LEN, &quarters(Mode::StartAndEnd));
        assert_eq!(
            (notes[0].start(), notes[0].end()),
            (Ticks(beat(2)), Ticks(beat(3)))
        );
        assert_eq!(
            (notes[1].start(), notes[1].end()),
            (Ticks(beat(3)), Ticks(beat(4)))
        );
    }

    fn short() -> Note {
        note(S + 100, 50, 60)
    }

    #[test]
    fn an_end_that_would_land_before_the_start_goes_to_the_next_line() {
        let mut notes = vec![short()];
        quantize(&mut notes, &[0], LEN, &mode(Mode::End));
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
        quantize(&mut notes, &[0], LEN, &narrow);
        assert_eq!(notes[0], note(S + 100, S / 2 - 10, 60));
    }

    #[test]
    fn each_note_takes_its_own_step_where_it_has_one() {
        let quarter = Settings {
            step: Ticks(4 * S),
            ..Settings::default()
        };
        let steps = |pitch: u8| match pitch {
            36 => Some(Ticks(4 * S)),
            42 => Some(Ticks(S)),
            _ => None,
        };
        let mut notes = vec![
            note(S + 30, S / 2, 36),
            note(S + 30, S / 2, 42),
            note(S + 30, S / 2, 38),
        ];
        let picked = quantize_each(&mut notes, &[0, 1, 2], LEN, &quarter, &steps);
        assert_eq!(picked.len(), 3, "the picks are kept");
        let start_of = |pitch: u8| {
            notes
                .iter()
                .find(|n| n.pitch() == pitch)
                .map(|n| n.start())
                .expect("the note")
        };
        assert_eq!(start_of(36), Ticks(0), "the kick to the quarter");
        assert_eq!(start_of(42), Ticks(S), "the hat to the sixteenth");
        assert_eq!(start_of(38), Ticks(0), "the rest to the options' grid");
        let mut alone = vec![note(S + 30, S / 2, 36)];
        let mut same = alone.clone();
        quantize_each(&mut alone, &[0], LEN, &quarter, &|_| None);
        quantize(&mut same, &[0], LEN, &quarter);
        assert_eq!(alone, same);
    }
}
