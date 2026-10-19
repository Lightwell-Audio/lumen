//! What a chase needs to find a clip's sounding notes and last controllers
//! without reading everything before the position (M6 review, M7).
//!
//! Notes are sorted by start, so the notes that began before a position are
//! a prefix, but which of them are still sounding is not: a long note can sit
//! among thousands of short ones. [`NoteIndex`] keeps, for each group of 64
//! notes, for each 64 groups, and so on, the latest end among them; a query
//! skips every group whose latest end is before the position. A chase then
//! reads the few notes that sound and a few groups, not the song so far.
//! [`ControlIndex`] keeps each controller's events apart, so the last one
//! before a position is a search, not a scan.
//!
//! Built off the audio thread (`MidiTrack::new`), read on it without
//! allocating. An index that does not match its clip (a clip whose notes were
//! changed after the track was made) is not used: the chase reads the clip
//! directly, as it did before.

use super::{MidiClip, MidiControl, MidiNote};

/// Notes in a group, groups in a group, and so on.
const FAN: usize = 64;

/// Slots in the controller table: 128 controllers, bend, pressure, program.
pub(super) const SLOTS: usize = 131;

/// The latest tick a note can end at, whatever its repeat cuts it to: the end
/// of its length, and at least a tick after its start (as `off_tick`).
fn latest_end(note: &MidiNote) -> i64 {
    (note.start.0 + note.length.0).max(note.start.0 + 1)
}

/// The latest end among groups of notes, level by level.
pub struct NoteIndex {
    len: usize,
    /// `maxes[0][g]` is the latest end among notes `g * 64..(g + 1) * 64`,
    /// `maxes[1][g]` among groups `g * 64..(g + 1) * 64` of `maxes[0]`, up to
    /// a level of one entry.
    maxes: Vec<Vec<i64>>,
}

impl NoteIndex {
    /// Allocates; call off the audio thread.
    pub fn new(notes: &[MidiNote]) -> Self {
        let mut maxes: Vec<Vec<i64>> = Vec::new();
        let mut level: Vec<i64> = notes
            .chunks(FAN)
            .map(|group| group.iter().map(latest_end).max().unwrap_or(i64::MIN))
            .collect();
        loop {
            let done = level.len() <= 1;
            let next: Vec<i64> = level
                .chunks(FAN)
                .map(|group| group.iter().copied().max().unwrap_or(i64::MIN))
                .collect();
            maxes.push(level);
            if done {
                break;
            }
            level = next;
        }
        Self {
            len: notes.len(),
            maxes,
        }
    }

    /// The notes it was made for.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Calls `visit(i)` for every note `i` in `from..to`, in order, that may
    /// end after tick `after` (a note whose latest possible end is not after
    /// it is skipped, and so is every group of such notes); stops when
    /// `visit` returns false. Never allocates.
    pub fn each_ending_after(
        &self,
        notes: &[MidiNote],
        (from, to): (usize, usize),
        after: i64,
        visit: &mut dyn FnMut(usize) -> bool,
    ) {
        let to = to.min(notes.len());
        if from >= to {
            return;
        }
        let top = self.maxes.len() - 1;
        self.walk(notes, top, 0, (from, to), after, visit);
    }

    /// Visits group `index` of `level` (level 0 holds groups of notes).
    /// False when `visit` asked to stop.
    fn walk(
        &self,
        notes: &[MidiNote],
        level: usize,
        index: usize,
        (from, to): (usize, usize),
        after: i64,
        visit: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        let width = FAN.pow(level as u32 + 1);
        let (a, b) = (index * width, ((index + 1) * width).min(notes.len()));
        if b <= from || a >= to || self.maxes[level][index] <= after {
            return true;
        }
        if level == 0 {
            let (lo, hi) = (a.max(from), b.min(to));
            for (i, note) in notes.iter().enumerate().take(hi).skip(lo) {
                if latest_end(note) > after && !visit(i) {
                    return false;
                }
            }
            return true;
        }
        let below = &self.maxes[level - 1];
        let first = index * FAN;
        for child in first..(first + FAN).min(below.len()) {
            if !self.walk(notes, level - 1, child, (from, to), after, visit) {
                return false;
            }
        }
        true
    }
}

/// A clip's controllers, one list a controller: the positions in the clip's
/// controls, in order.
pub struct ControlIndex {
    len: usize,
    slots: Vec<(usize, Vec<u32>)>,
}

impl ControlIndex {
    /// Allocates; call off the audio thread.
    pub fn new(controls: &[MidiControl]) -> Self {
        let mut by_slot: Vec<Vec<u32>> = vec![Vec::new(); SLOTS];
        for (i, control) in controls.iter().enumerate() {
            if let Some(slot) = super::hold::slot(control.msg) {
                by_slot[slot].push(i as u32);
            }
        }
        Self {
            len: controls.len(),
            slots: by_slot
                .into_iter()
                .enumerate()
                .filter(|(_, list)| !list.is_empty())
                .collect(),
        }
    }

    /// The controls it was made for.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Each controller present: its slot and the positions of its events.
    pub fn slots(&self) -> impl Iterator<Item = (usize, &[u32])> {
        self.slots
            .iter()
            .map(|(slot, list)| (*slot, list.as_slice()))
    }
}

/// Both indexes of one clip.
pub struct ClipIndex {
    pub notes: NoteIndex,
    pub controls: ControlIndex,
}

impl ClipIndex {
    /// Allocates; call off the audio thread.
    pub fn new(clip: &MidiClip) -> Self {
        Self {
            notes: NoteIndex::new(&clip.notes),
            controls: ControlIndex::new(&clip.controls),
        }
    }

    /// The note index, if it was made for this clip's notes.
    pub fn notes_for(&self, clip: &MidiClip) -> Option<&NoteIndex> {
        (self.notes.len() == clip.notes.len()).then_some(&self.notes)
    }

    /// The controller index, if it was made for this clip's controls.
    pub fn controls_for(&self, clip: &MidiClip) -> Option<&ControlIndex> {
        (self.controls.len() == clip.controls.len()).then_some(&self.controls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;

    fn note(start: i64, length: i64) -> MidiNote {
        MidiNote {
            start: Ticks(start),
            length: Ticks(length),
            pitch: 60,
            velocity: 100,
        }
    }

    fn found(
        index: &NoteIndex,
        notes: &[MidiNote],
        range: (usize, usize),
        after: i64,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        index.each_ending_after(notes, range, after, &mut |i| {
            out.push(i);
            true
        });
        out
    }

    #[test]
    fn it_finds_exactly_the_notes_that_end_after_the_tick_in_order() {
        // Every length from short to very long, at sizes that fill one, a few
        // and many groups, and several levels.
        let mut seed = 0x2545_f491_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for count in [0, 1, 63, 64, 65, 4_095, 4_096, 4_097, 20_000] {
            let notes: Vec<MidiNote> = (0..count)
                .map(|i| {
                    let long = next().is_multiple_of(50);
                    note(
                        i as i64 * 10,
                        if long {
                            (next() % 90_000) as i64
                        } else {
                            (next() % 40) as i64
                        },
                    )
                })
                .collect();
            let index = NoteIndex::new(&notes);
            for _ in 0..40 {
                let after = (next() % (count as u64 * 10 + 50)) as i64;
                let a = (next() % (count as u64 + 1)) as usize;
                let b = (next() % (count as u64 + 1)) as usize;
                let (from, to) = (a.min(b), a.max(b));
                let want: Vec<usize> = (from..to)
                    .filter(|&i| latest_end(&notes[i]) > after)
                    .collect();
                assert_eq!(
                    found(&index, &notes, (from, to), after),
                    want,
                    "{count} notes, {from}..{to}, after {after}"
                );
            }
        }
    }

    #[test]
    fn a_query_for_the_end_of_a_long_song_reads_a_few_notes_not_the_song() {
        // 100,000 short notes and one long one near the start.
        let mut notes: Vec<MidiNote> = (0..100_000).map(|i| note(i * 100, 50)).collect();
        notes[10] = note(1_000, 20_000_000);
        let index = NoteIndex::new(&notes);
        let mut seen = 0;
        index.each_ending_after(&notes, (0, notes.len()), 9_990_000, &mut |_| {
            seen += 1;
            true
        });
        assert_eq!(seen, 101, "the last hundred short notes and the long one");
        let mut seen = Vec::new();
        index.each_ending_after(&notes, (0, notes.len()), 10_000_000, &mut |i| {
            seen.push(i);
            true
        });
        assert_eq!(seen, [10], "only the long note can still be sounding");
    }

    #[test]
    fn stopping_stops() {
        let notes: Vec<MidiNote> = (0..1_000).map(|i| note(i, 10_000)).collect();
        let index = NoteIndex::new(&notes);
        let mut seen = 0;
        index.each_ending_after(&notes, (0, 1_000), 0, &mut |_| {
            seen += 1;
            seen < 5
        });
        assert_eq!(seen, 5);
    }
}
