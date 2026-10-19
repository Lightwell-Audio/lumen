//! Delay lines for delay compensation (M4 slice 6, ADR-0041).
//!
//! Every path that arrives early at a meeting point (a group, a return, a
//! sidechain input, the device outputs) goes through a line as long as the
//! difference, so all paths line up sample for sample. Latent devices are
//! lines too, until real devices arrive (the synthetic test device).
//!
//! The lines of a plan share one buffer, allocated when the plan is
//! compiled. A new plan takes over the contents of the lines it shares with
//! the old one (by [`LineKey`]), so a reroute while playing keeps what is
//! in flight.

use super::plan::{CHUNK, Tap};

/// What a line delays, so a new plan can take over the old one's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LineKey {
    /// What the tracks play into node `node`, which waits for a later
    /// input (a latent sidechain).
    Input {
        node: u64,
    },
    /// Device `index` of node `node`.
    Device {
        node: u64,
        index: u32,
    },
    /// Node `from`'s output, to a node or the device outputs.
    Output {
        from: u64,
    },
    Send {
        from: u64,
        to: u64,
        pre_fader: bool,
    },
    Sidechain {
        from: u64,
        to: u64,
        tap: Tap,
    },
    /// Node `from` on the cue bus.
    Cue {
        from: u64,
    },
}

/// A line's place in the shared buffer.
#[derive(Debug, Clone, Copy)]
struct Line {
    key: LineKey,
    /// Its left ring starts here, its right ring `frames` later.
    start: usize,
    frames: usize,
    /// Where the next frame is written (and the oldest read).
    write: usize,
}

/// A line's index; [`NO_LINE`] when a path needs no delay.
pub(super) type LineId = u32;
pub(super) const NO_LINE: LineId = u32::MAX;

/// The longest delay a device may report: about 5.5 s at 48 kHz. Longer
/// reports are taken as this.
pub const MAX_DEVICE_LATENCY: u32 = 1 << 18;

/// A plan's delay lines.
#[derive(Debug, Default)]
pub(super) struct Lines {
    lines: Vec<Line>,
    /// `(key, line)` sorted by key, to match lines across plans.
    by_key: Vec<(LineKey, LineId)>,
    data: Vec<f32>,
    /// Nothing is delayed: a scrub follows the hand, not the song, and is
    /// heard when it plays (M6 slice 6a). The rings are left as they are;
    /// playback clears them when it starts.
    bypass: bool,
}

impl Lines {
    /// Adds a line `frames` long (none when 0). Allocates; compile only.
    pub fn add(&mut self, key: LineKey, frames: u32) -> LineId {
        if frames == 0 {
            return NO_LINE;
        }
        let frames = frames as usize;
        let id = self.lines.len() as LineId;
        self.lines.push(Line {
            key,
            start: self.data.len(),
            frames,
            write: 0,
        });
        self.data.resize(self.data.len() + 2 * frames, 0.0);
        id
    }

    /// Call once every line is added.
    pub fn finish(&mut self) {
        self.by_key = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, l)| (l.key, i as LineId))
            .collect();
        self.by_key.sort_unstable_by_key(|&(key, _)| key);
    }

    /// Every line's key and length, in key order: part of a plan's shape.
    pub fn shape(&self) -> impl Iterator<Item = (LineKey, usize)> + '_ {
        self.by_key
            .iter()
            .map(|&(key, id)| (key, self.lines[id as usize].frames))
    }

    /// Silences every line (playback starts). Real-time safe.
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Takes over `old`'s lines whole when both plans hold the same lines
    /// in the same places (a plan that changes only levels): the buffers
    /// swap and nothing is copied. Otherwise as [`Lines::carry`]. `old` is
    /// left with this plan's silent buffer, so only for a plan that will
    /// not play again. Real-time safe: a comparison per line and a swap.
    pub fn take_over(&mut self, old: &mut Lines) {
        let same = self.data.len() == old.data.len()
            && self.lines.len() == old.lines.len()
            && self
                .lines
                .iter()
                .zip(&old.lines)
                .all(|(a, b)| a.key == b.key && a.start == b.start && a.frames == b.frames);
        if !same {
            return self.carry(old);
        }
        std::mem::swap(&mut self.data, &mut old.data);
        for (line, from) in self.lines.iter_mut().zip(&old.lines) {
            line.write = from.write;
        }
    }

    /// Takes over the contents of `old`'s lines with the same keys: the
    /// newest frames, as many as both hold (a longer line starts with
    /// silence before them). Real-time safe: one merge over two sorted
    /// lists and a copy per line.
    pub fn carry(&mut self, old: &Lines) {
        let (mut i, mut j) = (0, 0);
        while let (Some(&(a, here)), Some(&(b, there))) = (self.by_key.get(i), old.by_key.get(j)) {
            match a.cmp(&b) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    let from = old.lines[there as usize];
                    let line = &mut self.lines[here as usize];
                    let keep = from.frames.min(line.frames);
                    for side in 0..2 {
                        let ring = &mut self.data[line.start + side * line.frames..][..line.frames];
                        let source = &old.data[from.start + side * from.frames..][..from.frames];
                        // Reads start at 0: silence first, then the newest
                        // `keep` frames, oldest first, so the newest is
                        // read last.
                        let (silence, kept) = ring.split_at_mut(line.frames - keep);
                        silence.fill(0.0);
                        if keep > 0 {
                            // The newest `keep` frames lie in the ring from
                            // `begin`, wrapping at most once: two copies.
                            let begin = (from.write + from.frames - keep) % from.frames;
                            let first = keep.min(from.frames - begin);
                            kept[..first].copy_from_slice(&source[begin..begin + first]);
                            kept[first..].copy_from_slice(&source[..keep - first]);
                        }
                    }
                    line.write = 0;
                    i += 1;
                    j += 1;
                }
            }
        }
    }

    /// Passes audio straight through (a scrub) or delays it again.
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    /// Delays `left` and `right` in place through `line`. Real-time safe.
    pub fn run(&mut self, line: LineId, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }
        let Some(l) = self.lines.get_mut(line as usize) else {
            return;
        };
        let (start, frames, write) = (l.start, l.frames, l.write);
        let (ring_left, rest) = self.data[start..start + 2 * frames].split_at_mut(frames);
        let next = delay(ring_left, write, left);
        delay(rest, write, right);
        l.write = next;
    }

    /// `left` and `right` through `line`, into `scratch` (both sides,
    /// `CHUNK` frames each); without a line, `left` and `right` themselves.
    pub fn through<'a>(
        &mut self,
        line: LineId,
        left: &'a [f32],
        right: &'a [f32],
        scratch: &'a mut [f32; 2 * CHUNK],
    ) -> (&'a [f32], &'a [f32]) {
        if line == NO_LINE || self.bypass {
            return (left, right);
        }
        let n = left.len();
        let (a, b) = scratch.split_at_mut(CHUNK);
        let (a, b) = (&mut a[..n], &mut b[..n]);
        a.copy_from_slice(left);
        b.copy_from_slice(right);
        self.run(line, a, b);
        (a, b)
    }
}

/// Delays `samples` in place through `ring`, from `write`; returns the
/// next write position.
#[inline]
fn delay(ring: &mut [f32], mut write: usize, samples: &mut [f32]) -> usize {
    for sample in samples {
        std::mem::swap(sample, &mut ring[write]);
        write += 1;
        if write == ring.len() {
            write = 0;
        }
    }
    write
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: LineKey = LineKey::Output { from: 1 };

    fn ramp(from: usize, n: usize) -> Vec<f32> {
        (from..from + n).map(|i| i as f32 + 1.0).collect()
    }

    #[test]
    fn a_line_delays_by_its_length_in_any_block_size() {
        let mut lines = Lines::default();
        let line = lines.add(KEY, 5);
        lines.finish();
        let mut heard = Vec::new();
        let mut at = 0;
        for n in [1, 3, 7, 2, 11] {
            let mut left = ramp(at, n);
            let mut right = ramp(at, n);
            lines.run(line, &mut left, &mut right);
            assert_eq!(left, right);
            heard.extend(left);
            at += n;
        }
        let expected: Vec<f32> = (0..at)
            .map(|i| if i < 5 { 0.0 } else { (i - 5) as f32 + 1.0 })
            .collect();
        assert_eq!(heard, expected);
    }

    #[test]
    fn no_delay_needs_no_line() {
        let mut lines = Lines::default();
        assert_eq!(lines.add(KEY, 0), NO_LINE);
        let (mut left, mut right) = (ramp(0, 4), ramp(0, 4));
        lines.run(NO_LINE, &mut left, &mut right);
        assert_eq!(left, ramp(0, 4));
    }

    /// Plays `frames` of the ramp from `from` through a line of `length`
    /// that took over from `old`.
    fn continue_from(old: &Lines, length: u32, from: usize, frames: usize) -> Vec<f32> {
        let mut lines = Lines::default();
        let line = lines.add(KEY, length);
        lines.finish();
        lines.carry(old);
        let (mut left, mut right) = (ramp(from, frames), ramp(from, frames));
        lines.run(line, &mut left, &mut right);
        left
    }

    #[test]
    fn a_plan_of_the_same_lines_takes_them_over_without_a_copy() {
        let mut old = Lines::default();
        let line = old.add(KEY, 4);
        old.finish();
        let (mut left, mut right) = (ramp(0, 10), ramp(0, 10));
        old.run(line, &mut left, &mut right);
        let held = old.data.as_ptr();

        let mut new = Lines::default();
        let line = new.add(KEY, 4);
        new.finish();
        new.take_over(&mut old);
        assert_eq!(new.data.as_ptr(), held, "the buffer moved, not copied");
        let (mut left, mut right) = (ramp(10, 6), ramp(10, 6));
        new.run(line, &mut left, &mut right);
        assert_eq!(left, ramp(6, 6), "the stream carries on");

        // Another length: copied as `carry` does.
        let mut longer = Lines::default();
        let line = longer.add(KEY, 6);
        longer.finish();
        longer.take_over(&mut new);
        let (mut left, mut right) = (ramp(16, 8), ramp(16, 8));
        longer.run(line, &mut left, &mut right);
        assert_eq!(left, [0.0, 0.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0]);
    }

    #[test]
    fn a_shorter_or_longer_line_takes_the_newest_frames_from_a_ring_that_has_wrapped() {
        let mut old = Lines::default();
        let line = old.add(KEY, 7);
        old.finish();
        // Ten frames through a ring of seven: the write position has wrapped.
        let (mut left, mut right) = (ramp(0, 10), ramp(0, 10));
        old.run(line, &mut left, &mut right);
        // Shorter: the newest five frames in flight are 6 to 10.
        assert_eq!(continue_from(&old, 5, 10, 5), ramp(5, 5));
        // As long: all seven, 4 to 10.
        assert_eq!(continue_from(&old, 7, 10, 7), ramp(3, 7));
        // Longer: silence first, then the seven.
        let longer = continue_from(&old, 10, 10, 10);
        assert_eq!(&longer[..3], [0.0; 3]);
        assert_eq!(&longer[3..], ramp(3, 7));
    }

    #[test]
    fn a_new_plan_carries_what_is_in_flight() {
        let mut old = Lines::default();
        let line = old.add(KEY, 4);
        old.finish();
        let (mut left, mut right) = (ramp(0, 10), ramp(0, 10));
        old.run(line, &mut left, &mut right);

        // The same length: the stream carries on as if nothing changed.
        let same = continue_from(&old, 4, 10, 6);
        assert_eq!(same, ramp(6, 6));
        // Shorter: the newest frames, so the stream jumps ahead by 2.
        let shorter = continue_from(&old, 2, 10, 4);
        assert_eq!(shorter, ramp(8, 4));
        // Longer: silence, then what the old line held.
        let longer = continue_from(&old, 6, 10, 8);
        assert_eq!(longer, [0.0, 0.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    }
}
