use super::plan::{CHUNK, Tap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LineKey {
    Input {
        node: u64,
    },
    Device {
        node: u64,
        index: u32,
    },
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
    Cue {
        from: u64,
    },
}

#[derive(Debug, Clone, Copy)]
struct Line {
    key: LineKey,
    start: usize,
    frames: usize,
    write: usize,
}

pub(super) type LineId = u32;
pub(super) const NO_LINE: LineId = u32::MAX;

pub const MAX_DEVICE_LATENCY: u32 = 1 << 18;

#[derive(Debug, Default)]
pub(super) struct Lines {
    lines: Vec<Line>,
    by_key: Vec<(LineKey, LineId)>,
    data: Vec<f32>,
    bypass: bool,
}

impl Lines {
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

    pub fn finish(&mut self) {
        self.by_key = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, l)| (l.key, i as LineId))
            .collect();
        self.by_key.sort_unstable_by_key(|&(key, _)| key);
    }

    pub fn shape(&self) -> impl Iterator<Item = (LineKey, usize)> + '_ {
        self.by_key
            .iter()
            .map(|&(key, id)| (key, self.lines[id as usize].frames))
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

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
                        let (silence, kept) = ring.split_at_mut(line.frames - keep);
                        silence.fill(0.0);
                        if keep > 0 {
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

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

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
        let (mut left, mut right) = (ramp(0, 10), ramp(0, 10));
        old.run(line, &mut left, &mut right);
        assert_eq!(continue_from(&old, 5, 10, 5), ramp(5, 5));
        assert_eq!(continue_from(&old, 7, 10, 7), ramp(3, 7));
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

        let same = continue_from(&old, 4, 10, 6);
        assert_eq!(same, ramp(6, 6));
        let shorter = continue_from(&old, 2, 10, 4);
        assert_eq!(shorter, ramp(8, 4));
        let longer = continue_from(&old, 6, 10, 8);
        assert_eq!(longer, [0.0, 0.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    }
}
