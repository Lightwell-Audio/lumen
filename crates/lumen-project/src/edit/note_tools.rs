use lumen_core::time::Ticks;

use super::notes::{MIN_NOTE, finish, picked, tag};
use crate::model::Note;

const MAX_TIMING: i64 = i64::MAX / 4;

const MAX_SEARCH: i32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Humanize {
    pub timing: Ticks,
    pub velocity: u8,
    pub seed: u64,
}

pub fn humanize(
    notes: &mut Vec<Note>,
    selection: &[usize],
    length: Ticks,
    h: &Humanize,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let timing_range = h.timing.0.clamp(0, MAX_TIMING);
    let velocity_range = i64::from(h.velocity);
    let mut rng = SplitMix64(h.seed);
    let mut tagged = tag(notes, &picked);
    for &i in &picked {
        let time_offset = rng.offset(timing_range);
        let velocity_offset = rng.offset(velocity_range);
        let n = tagged[i].0;
        let start = if timing_range == 0 {
            n.start()
        } else {
            let latest = (length - n.length())
                .min(length - Ticks(1))
                .max(Ticks::ZERO);
            Ticks((n.start().0 + time_offset).clamp(0, latest.0))
        };
        let velocity = if velocity_range == 0 {
            n.velocity()
        } else {
            (i64::from(n.velocity()) + velocity_offset).clamp(1, 127) as u8
        };
        tagged[i].0 = Note(
            start,
            n.length(),
            n.pitch(),
            velocity,
            n.muted(),
            n.carries_on(),
        );
    }
    finish(notes, tagged)
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn offset(&mut self, range: i64) -> i64 {
        let span = range as u64 * 2 + 1;
        (self.next_u64() % span) as i64 - range
    }
}

pub fn fixed_length(
    notes: &mut Vec<Note>,
    selection: &[usize],
    length: Ticks,
    len: Ticks,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let mut tagged = tag(notes, &picked);
    for &i in &picked {
        let n = tagged[i].0;
        let room = (length - n.start()).max(Ticks(1));
        let to = len.max(MIN_NOTE).min(room);
        tagged[i].0 = Note(
            n.start(),
            to,
            n.pitch(),
            n.velocity(),
            n.muted(),
            n.carries_on(),
        );
    }
    finish(notes, tagged)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Major,
    Minor,
    Dorian,
    Mixolydian,
    PentatonicMajor,
    PentatonicMinor,
    Chromatic,
}

impl Scale {
    pub const ALL: [Scale; 7] = [
        Scale::Major,
        Scale::Minor,
        Scale::Dorian,
        Scale::Mixolydian,
        Scale::PentatonicMajor,
        Scale::PentatonicMinor,
        Scale::Chromatic,
    ];

    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::PentatonicMajor => &[0, 2, 4, 7, 9],
            Scale::PentatonicMinor => &[0, 3, 5, 7, 10],
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::Dorian => "Dorian",
            Scale::Mixolydian => "Mixolydian",
            Scale::PentatonicMajor => "Major pentatonic",
            Scale::PentatonicMinor => "Minor pentatonic",
            Scale::Chromatic => "Chromatic",
        }
    }
}

pub fn scale_constrain(
    notes: &mut Vec<Note>,
    selection: &[usize],
    root: u8,
    scale: Scale,
) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let intervals = scale.intervals();
    let root = i32::from(root % 12);
    let mut tagged = tag(notes, &picked);
    for &i in &picked {
        let n = tagged[i].0;
        let pitch = nearest(i32::from(n.pitch()), intervals, root);
        tagged[i].0 = Note(
            n.start(),
            n.length(),
            pitch as u8,
            n.velocity(),
            n.muted(),
            n.carries_on(),
        );
    }
    finish(notes, tagged)
}

fn in_scale(pitch: i32, intervals: &[u8], root: i32) -> bool {
    let step = (pitch - root).rem_euclid(12) as u8;
    intervals.contains(&step)
}

fn nearest(pitch: i32, intervals: &[u8], root: i32) -> i32 {
    if in_scale(pitch, intervals, root) {
        return pitch;
    }
    (1..=MAX_SEARCH)
        .flat_map(|d| [pitch - d, pitch + d])
        .find(|&p| (0..=127).contains(&p) && in_scale(p, intervals, root))
        .unwrap_or(pitch)
}

pub fn reverse(notes: &mut Vec<Note>, selection: &[usize]) -> Vec<usize> {
    let picked = picked(selection, notes.len());
    let (Some(first), Some(last)) = (
        picked.iter().map(|&i| notes[i].start()).min(),
        picked.iter().map(|&i| notes[i].end()).max(),
    ) else {
        return Vec::new();
    };
    let mut tagged = tag(notes, &picked);
    for &i in &picked {
        let n = tagged[i].0;
        tagged[i].0 = Note(
            first + last - n.end(),
            n.length(),
            n.pitch(),
            n.velocity(),
            n.muted(),
            n.carries_on(),
        );
    }
    finish(notes, tagged)
}

#[cfg(test)]
mod tests {
    use lumen_core::time::PPQ;

    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks(PPQ * n)
    }

    fn n(start: i64, len: i64, pitch: u8) -> Note {
        Note::new(q(start), q(len), pitch, 100)
    }

    const LEN: Ticks = Ticks(PPQ * 16);

    fn row() -> Vec<Note> {
        (0..8u8).map(|i| n(i64::from(i), 1, 60 + i)).collect()
    }

    fn hum(timing: i64, velocity: u8, seed: u64) -> Humanize {
        Humanize {
            timing: Ticks(timing),
            velocity,
            seed,
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_result_and_another_seed_another() {
        let all: Vec<usize> = (0..8).collect();
        let mut a = row();
        let mut b = row();
        humanize(&mut a, &all, LEN, &hum(PPQ / 4, 20, 42));
        humanize(&mut b, &all, LEN, &hum(PPQ / 4, 20, 42));
        assert_eq!(a, b);
        let mut c = row();
        humanize(&mut c, &all, LEN, &hum(PPQ / 4, 20, 43));
        assert_ne!(a, c);
        assert_ne!(a, row(), "the notes do move");
    }

    #[test]
    fn starts_and_velocities_move_within_their_ranges() {
        let all: Vec<usize> = (0..8).collect();
        let timing = PPQ / 8;
        let mut notes = row();
        let sel = humanize(&mut notes, &all, LEN, &hum(timing, 5, 7));
        assert_eq!(sel, all);
        for (note, original) in notes.iter().zip(row()) {
            let moved = note.start().0 - original.start().0;
            assert!(moved.abs() <= timing, "moved {moved}");
            let delta = i64::from(note.velocity()) - i64::from(original.velocity());
            assert!(delta.abs() <= 5, "velocity moved {delta}");
            assert_eq!(note.pitch(), original.pitch());
            assert_eq!(note.length(), original.length());
        }
    }

    #[test]
    fn starts_stay_inside_the_clip() {
        let mut notes = vec![
            n(0, 1, 60),
            n(7, 2, 64),
            n(15, 1, 62),
            Note::new(q(0), q(20), 66, 100),
        ];
        humanize(&mut notes, &[0, 1, 2, 3], LEN, &hum(PPQ * 64, 0, 9));
        for note in &notes {
            assert!(
                note.start() >= Ticks::ZERO && note.start() < LEN,
                "{note:?}"
            );
            if note.pitch() == 66 {
                assert_eq!(note.start(), Ticks::ZERO);
            } else {
                assert!(note.start() + note.length() <= LEN, "{note:?}");
            }
        }
    }

    #[test]
    fn velocities_stay_between_1_and_127() {
        let mut notes = vec![
            Note::new(q(0), q(1), 60, 1),
            Note::new(q(2), q(1), 62, 127),
            Note::new(q(4), q(1), 64, 64),
        ];
        humanize(&mut notes, &[0, 1, 2], LEN, &hum(0, 255, 5));
        assert!(
            notes
                .iter()
                .all(|note| (1..=127).contains(&note.velocity()))
        );
    }

    #[test]
    fn a_zero_axis_is_left_alone() {
        let mut notes = vec![n(0, 1, 60), Note::new(q(15), q(3), 62, 100)];
        humanize(&mut notes, &[0, 1], LEN, &hum(0, 9, 3));
        assert_eq!(notes[1].start(), q(15));
        assert!(
            notes
                .iter()
                .all(|note| (91..=109).contains(&note.velocity()))
        );

        let mut notes = vec![n(0, 1, 60), n(2, 1, 62)];
        let before = notes.clone();
        humanize(&mut notes, &[0, 1], LEN, &hum(PPQ / 4, 0, 3));
        assert!(
            notes
                .iter()
                .zip(&before)
                .all(|(a, b)| a.velocity() == b.velocity())
        );
    }

    #[test]
    fn unpicked_notes_and_the_mute_are_kept() {
        let mut notes = row();
        notes[3] = Note(q(3), q(1), 63, 100, true, true);
        let before = notes.clone();
        humanize(&mut notes, &[3, 5], LEN, &hum(PPQ / 8, 30, 11));
        for i in [0, 1, 2, 4, 6, 7] {
            assert_eq!(notes[i], before[i], "note {i} was not picked");
        }
        assert!(notes[3].muted());
        assert_eq!(notes[3].pitch(), 63);
    }

    #[test]
    fn the_returned_indices_follow_the_notes_after_they_move() {
        let mut notes = row();
        let sel = humanize(&mut notes, &[1, 4, 7], LEN, &hum(PPQ * 8, 0, 21));
        assert_eq!(sel.len(), 3);
        assert!(sel.is_sorted());
        let mut pitches: Vec<u8> = sel.iter().map(|&i| notes[i].pitch()).collect();
        pitches.sort_unstable();
        assert_eq!(pitches, [61, 64, 67]);
        assert!(notes.is_sorted_by_key(|note| (note.start(), note.pitch())));
        assert!(
            !notes.iter().map(|note| note.pitch()).eq(60..68),
            "the list was reordered, so the indices had to follow"
        );
    }

    #[test]
    fn an_empty_selection_changes_nothing() {
        let mut notes = row();
        let before = notes.clone();
        assert!(humanize(&mut notes, &[], LEN, &hum(PPQ, 50, 1)).is_empty());
        assert_eq!(notes, before);
    }

    #[test]
    fn picked_notes_take_the_length_and_the_rest_do_not() {
        let mut notes = vec![n(0, 4, 60), n(2, 1, 62), n(8, 4, 64)];
        let sel = fixed_length(&mut notes, &[0, 1], LEN, q(1));
        assert_eq!(sel, [0, 1]);
        assert_eq!(notes, [n(0, 1, 60), n(2, 1, 62), n(8, 4, 64)]);
    }

    #[test]
    fn a_note_is_cut_where_the_content_ends() {
        let mut notes = vec![n(0, 1, 62), n(14, 1, 60)];
        fixed_length(&mut notes, &[0, 1], LEN, q(4));
        assert_eq!(notes, [n(0, 4, 62), n(14, 2, 60)]);
    }

    #[test]
    fn a_length_is_at_least_the_shortest_note() {
        let mut notes = vec![n(0, 1, 60)];
        fixed_length(&mut notes, &[0], LEN, Ticks(1));
        assert_eq!(notes[0].length(), MIN_NOTE);
    }

    #[test]
    fn pitch_velocity_and_mute_are_kept() {
        let mut notes = vec![Note(q(0), q(2), 60, 33, true, true)];
        fixed_length(&mut notes, &[0], LEN, q(1));
        assert_eq!(notes[0], Note(q(0), q(1), 60, 33, true, true));
    }

    fn constrained(pitch: u8, root: u8, scale: Scale) -> u8 {
        let mut notes = vec![Note::new(Ticks::ZERO, MIN_NOTE, pitch, 100)];
        scale_constrain(&mut notes, &[0], root, scale);
        notes[0].pitch()
    }

    #[test]
    fn a_tie_goes_down() {
        assert_eq!(constrained(61, 0, Scale::Major), 60);
        assert_eq!(constrained(63, 0, Scale::Major), 62);
        assert_eq!(constrained(66, 0, Scale::Major), 65);
    }

    #[test]
    fn the_nearest_tone_wins_on_either_side() {
        assert_eq!(constrained(61, 0, Scale::PentatonicMinor), 60);
        assert_eq!(constrained(62, 0, Scale::PentatonicMinor), 63);
    }

    #[test]
    fn in_scale_pitches_and_the_chromatic_scale_stay_put() {
        for pitch in [60, 62, 64, 65, 67, 69, 71] {
            assert_eq!(constrained(pitch, 0, Scale::Major), pitch);
        }
        for pitch in 0..=127 {
            assert_eq!(constrained(pitch, 3, Scale::Chromatic), pitch);
        }
    }

    #[test]
    fn the_search_stays_inside_the_midi_range() {
        assert_eq!(constrained(0, 1, Scale::PentatonicMinor), 1);
        assert_eq!(constrained(126, 0, Scale::PentatonicMajor), 127);
        assert_eq!(constrained(127, 0, Scale::PentatonicMajor), 127);
    }

    #[test]
    fn every_pitch_lands_on_a_tone_of_its_scale_within_two_semitones() {
        for scale in Scale::ALL {
            let intervals = scale.intervals();
            for root in 0..12u8 {
                for pitch in 0..=127u8 {
                    let out = constrained(pitch, root, scale);
                    assert!(out <= 127);
                    let step = (out + 12 - root) % 12;
                    assert!(
                        intervals.contains(&step),
                        "{pitch} to {out} in {}",
                        scale.name()
                    );
                    assert!(out.abs_diff(pitch) <= 2, "{pitch} to {out}");
                }
            }
        }
    }

    #[test]
    fn the_returned_indices_point_at_the_picked_notes() {
        let mut notes = vec![
            Note::new(q(0), MIN_NOTE, 60, 11),
            Note::new(q(0), MIN_NOTE, 61, 10),
            Note::new(q(0), MIN_NOTE, 62, 12),
            Note::new(q(1), MIN_NOTE, 66, 13),
        ];
        let sel = scale_constrain(&mut notes, &[1, 3], 0, Scale::Major);
        assert_eq!(sel, [1, 3]);
        assert_eq!((notes[1].pitch(), notes[1].velocity()), (60, 10));
        assert_eq!((notes[3].pitch(), notes[3].velocity()), (65, 13));
        assert_eq!(notes[0].velocity(), 11, "unpicked");
    }

    #[test]
    fn every_scale_is_a_sane_set_of_steps() {
        let counts: [usize; 7] = [7, 7, 7, 7, 5, 5, 12];
        for (scale, count) in Scale::ALL.iter().zip(counts) {
            let steps = scale.intervals();
            assert_eq!(steps.len(), count, "{}", scale.name());
            assert_eq!(steps[0], 0, "{}", scale.name());
            assert!(steps.windows(2).all(|w| w[0] < w[1]), "{}", scale.name());
            assert!(steps.iter().all(|&s| s < 12), "{}", scale.name());
        }
        assert_eq!(Scale::Major.intervals(), [0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(Scale::Minor.intervals(), [0, 2, 3, 5, 7, 8, 10]);
        assert_eq!(Scale::Dorian.intervals(), [0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(Scale::Mixolydian.intervals(), [0, 2, 4, 5, 7, 9, 10]);
        assert_eq!(Scale::PentatonicMajor.intervals(), [0, 2, 4, 7, 9]);
        assert_eq!(Scale::PentatonicMinor.intervals(), [0, 3, 5, 7, 10]);
        let names: Vec<&str> = Scale::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            [
                "Major",
                "Minor",
                "Dorian",
                "Mixolydian",
                "Major pentatonic",
                "Minor pentatonic",
                "Chromatic",
            ]
        );
    }

    #[test]
    fn reversing_three_notes_of_different_lengths_mirrors_them_in_their_span() {
        let mut notes = vec![n(0, 1, 60), n(2, 2, 62), n(3, 4, 64)];
        let sel = reverse(&mut notes, &[0, 1, 2]);
        assert_eq!(notes, [n(0, 4, 64), n(3, 2, 62), n(6, 1, 60)]);
        assert_eq!(sel, [0, 1, 2]);
    }

    #[test]
    fn only_the_picked_notes_move_and_the_indices_follow_them() {
        let mut notes = vec![n(0, 1, 60), n(2, 2, 62), n(3, 4, 64), n(10, 1, 70)];
        let sel = reverse(&mut notes, &[0, 2]);
        assert_eq!(notes, [n(0, 4, 64), n(2, 2, 62), n(6, 1, 60), n(10, 1, 70)]);
        assert_eq!(sel, [0, 2]);
    }

    #[test]
    fn a_single_note_is_unchanged_and_an_empty_selection_changes_nothing() {
        let mut notes = vec![n(0, 1, 60), n(4, 2, 62)];
        let before = notes.clone();
        assert_eq!(reverse(&mut notes, &[1]), [1]);
        assert_eq!(notes, before);
        assert!(reverse(&mut notes, &[]).is_empty());
        assert_eq!(notes, before);
    }

    #[test]
    fn reversing_twice_gives_the_original_back() {
        let mut notes = vec![n(0, 1, 60), n(2, 2, 62), n(3, 4, 64), n(10, 1, 70)];
        let before = notes.clone();
        reverse(&mut notes, &[0, 1, 2, 3]);
        reverse(&mut notes, &[0, 1, 2, 3]);
        assert_eq!(notes, before);
    }
}
