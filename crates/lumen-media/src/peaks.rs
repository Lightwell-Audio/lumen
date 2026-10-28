//! Waveform peaks (M3 slice 4c, ADR-0027): the minimum and maximum of the
//! audio in buckets of frames, at several resolutions, so a waveform can be
//! drawn crisply at any zoom without reading the audio itself.
//!
//! Level 0 has a bucket for every [`BASE`] frames; each level above covers
//! [`FACTOR`] buckets of the one below. Values are signed 8-bit fractions of
//! full scale: plenty for a waveform a few hundred pixels tall, at a quarter
//! of the memory of floats. The minimum rounds down and the maximum up, so
//! quiet audio still shows.

use std::sync::atomic::{AtomicBool, Ordering};

/// Frames per bucket at the finest level.
pub const BASE: usize = 16;
/// Buckets of the level below that one bucket covers.
pub const FACTOR: usize = 4;
/// Levels: 16 frames per bucket up to 16 × 4⁶ = 65,536.
pub const LEVELS: usize = 7;

/// One bucket: left minimum, left maximum, right minimum, right maximum,
/// as fractions of full scale times 127. Mono repeats its one channel.
pub type Bucket = [i8; 4];

/// The peaks of one piece of audio, finest level first.
#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    /// 1 or 2: how many channels the waveform shows.
    pub channels: usize,
    levels: Vec<Vec<Bucket>>,
}

impl Peaks {
    /// Builds the peaks of planar `channels` (only the first two count, as
    /// in playback). `None` if `cancel` was set.
    pub fn build(channels: &[Vec<f32>], cancel: &AtomicBool) -> Option<Self> {
        let left = channels.first().map_or(&[][..], Vec::as_slice);
        let right = channels.get(1).map_or(left, Vec::as_slice);
        let frames = left.len().min(right.len());
        let mut finest = Vec::with_capacity(frames.div_ceil(BASE));
        for (i, (l, r)) in left[..frames]
            .chunks(BASE)
            .zip(right[..frames].chunks(BASE))
            .enumerate()
        {
            if i % 65_536 == 0 && cancel.load(Ordering::Relaxed) {
                return None;
            }
            let (l_min, l_max) = extent(l);
            let (r_min, r_max) = extent(r);
            finest.push([down(l_min), up(l_max), down(r_min), up(r_max)]);
        }
        let mut levels = vec![finest];
        while levels.len() < LEVELS {
            let below = levels.last().map_or(&[][..], Vec::as_slice);
            levels.push(below.chunks(FACTOR).map(merge).collect());
        }
        Some(Self {
            channels: channels.len().clamp(1, 2),
            levels,
        })
    }

    /// The levels, finest first: level `n` has a bucket for every
    /// `BASE × FACTOR^n` frames.
    pub fn levels(&self) -> &[Vec<Bucket>] {
        &self.levels
    }

    /// Bytes held.
    pub fn memory(&self) -> usize {
        self.levels
            .iter()
            .map(|l| l.len() * size_of::<Bucket>())
            .sum()
    }
}

fn extent(samples: &[f32]) -> (f32, f32) {
    samples
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)))
}

fn down(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).floor() as i8
}

fn up(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).ceil() as i8
}

fn merge(buckets: &[Bucket]) -> Bucket {
    buckets
        .iter()
        .fold([i8::MAX, i8::MIN, i8::MAX, i8::MIN], |a, b| {
            [
                a[0].min(b[0]),
                a[1].max(b[1]),
                a[2].min(b[2]),
                a[3].max(b[3]),
            ]
        })
}

/// Fine buckets in one coarse bucket of [`Growing`]: 16 × 16 = 256 frames.
pub const GROWING_FACTOR: usize = 16;

/// The peaks of audio that is still arriving (a take being recorded), built
/// with the finished waveform's own buckets (the same extent, rounded the
/// same way) so the live picture is the final one drawn early. It keeps the
/// finest level and one coarse level of 256 frames, which a zoomed-out view
/// reads instead of every fine bucket. A partly filled bucket is not shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Growing {
    /// 1 or 2, as [`Peaks::channels`]: how many channels the take has.
    pub channels: usize,
    fine: Vec<Bucket>,
    coarse: Vec<Bucket>,
    /// Extent of the bucket in progress, as `[min, max]` of each channel.
    pending: [f32; 4],
    filled: usize,
}

impl Growing {
    /// Takes in frames of `left` and, for a take of two channels, `right`.
    pub fn add(&mut self, left: &[f32], right: Option<&[f32]>) {
        self.channels = if right.is_some() { 2 } else { 1 };
        let right = right.unwrap_or(left);
        for (l, r) in left.iter().zip(right) {
            if self.filled == 0 {
                self.pending = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];
            }
            self.pending[0] = self.pending[0].min(*l);
            self.pending[1] = self.pending[1].max(*l);
            self.pending[2] = self.pending[2].min(*r);
            self.pending[3] = self.pending[3].max(*r);
            self.filled += 1;
            if self.filled == BASE {
                let [l_min, l_max, r_min, r_max] = self.pending;
                self.fine
                    .push([down(l_min), up(l_max), down(r_min), up(r_max)]);
                self.filled = 0;
                if self.fine.len().is_multiple_of(GROWING_FACTOR) {
                    let from = self.fine.len() - GROWING_FACTOR;
                    self.coarse.push(merge(&self.fine[from..]));
                }
            }
        }
    }

    /// The finished peaks, the partly filled last bucket included: the same
    /// as [`Peaks::build`] over every frame added (a file read in blocks).
    pub fn finish(mut self) -> Peaks {
        if self.filled > 0 {
            let [l_min, l_max, r_min, r_max] = self.pending;
            self.fine
                .push([down(l_min), up(l_max), down(r_min), up(r_max)]);
        }
        let mut levels = vec![self.fine];
        while levels.len() < LEVELS {
            let below = levels.last().map_or(&[][..], Vec::as_slice);
            levels.push(below.chunks(FACTOR).map(merge).collect());
        }
        Peaks {
            channels: self.channels.clamp(1, 2),
            levels,
        }
    }

    /// Frames in the buckets so far.
    pub fn frames(&self) -> usize {
        self.fine.len() * BASE
    }

    /// The bucket covering frames `from..to` of the take: the extent of
    /// every bucket under them, from the coarse level when `to - from` holds
    /// several of its buckets and the fine level otherwise. Frames past the
    /// end are silent, and a range is never narrower than one bucket.
    pub fn extent(&self, from: f64, to: f64) -> Bucket {
        let coarse = to - from >= (BASE * GROWING_FACTOR * 4) as f64;
        let (buckets, size) = if coarse {
            (&self.coarse, BASE * GROWING_FACTOR)
        } else {
            (&self.fine, BASE)
        };
        let first = ((from.max(0.0) / size as f64) as usize).min(buckets.len());
        let last = ((to.max(0.0) / size as f64).ceil() as usize).max(first + 1);
        buckets
            .get(first..last.min(buckets.len()))
            .filter(|s| !s.is_empty())
            .map_or([0; 4], merge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(channels: &[Vec<f32>]) -> Peaks {
        Peaks::build(channels, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn each_bucket_holds_the_extent_of_its_frames() {
        // 40 frames: two whole buckets and a partial one.
        let left: Vec<f32> = (0..40).map(|i| (i as f32 - 20.0) / 20.0).collect();
        let right: Vec<f32> = left.iter().map(|v| -v * 0.5).collect();
        let peaks = build(&[left, right]);
        let finest = &peaks.levels()[0];
        assert_eq!(finest.len(), 3);
        // Frames 0–15: left −1.0 to −0.25; right +0.5 to +0.125.
        assert_eq!(finest[0], [-127, -31, 15, 64]);
        // Frames 32–39, the partial bucket: left 0.6 to 0.95.
        assert_eq!(finest[2][..2], [76, 121]);
        assert_eq!(peaks.channels, 2);
    }

    #[test]
    fn a_growing_take_has_the_buckets_the_finished_file_will_have() {
        let left: Vec<f32> = (0..1_000)
            .map(|i| ((i * 7) % 200) as f32 / 150.0 - 0.6)
            .collect();
        let right: Vec<f32> = left.iter().map(|v| v * -0.3).collect();
        let finished = build(&[left.clone(), right.clone()]);
        // Fed in uneven chunks, as the audio arrives.
        let mut live = Growing::default();
        let mut at = 0;
        for step in [1, 17, 300, 5, 677] {
            let end = (at + step).min(left.len());
            live.add(&left[at..end], Some(&right[at..end]));
            at = end;
        }
        assert_eq!(live.channels, 2);
        // 62 whole buckets; the partial one is not shown.
        assert_eq!(live.frames(), 992);
        assert_eq!(live.fine, finished.levels()[0][..62]);
        // A range reads the extent of the buckets under it.
        assert_eq!(live.extent(0.0, 16.0), finished.levels()[0][0]);
        assert_eq!(live.extent(16.0, 48.0), merge(&finished.levels()[0][1..3]));
        // A wide range reads the coarse level, past the end is silent.
        assert_eq!(live.extent(0.0, 512.0), merge(&finished.levels()[0][..32]));
        assert_eq!(live.extent(5_000.0, 5_016.0), [0; 4]);
    }

    #[test]
    fn a_growing_mono_take_repeats_its_channel_and_quiet_audio_shows() {
        let mut live = Growing::default();
        live.add(&[0.001; 16], None);
        assert_eq!(live.channels, 1);
        assert_eq!(live.extent(0.0, 16.0), [0, 1, 0, 1]);
    }

    #[test]
    fn quiet_audio_still_shows_and_silence_is_flat() {
        let quiet = build(&[vec![0.001; 32]]);
        assert_eq!(quiet.levels()[0][0][..2], [0, 1], "rounded up to one step");
        let silent = build(&[vec![0.0; 32]]);
        assert_eq!(silent.levels()[0][0], [0, 0, 0, 0]);
        // Over full scale is clamped.
        let loud = build(&[vec![1.5, -2.0]]);
        assert_eq!(loud.levels()[0][0][..2], [-127, 127]);
    }

    #[test]
    fn coarser_levels_cover_four_buckets_each() {
        let frames = BASE * FACTOR * 3 + 1;
        let left: Vec<f32> = (0..frames).map(|i| ((i % 7) as f32 - 3.0) / 3.0).collect();
        let peaks = build(&[left]);
        assert_eq!(peaks.levels().len(), LEVELS);
        for (n, level) in peaks.levels().iter().enumerate() {
            let size = BASE * FACTOR.pow(n as u32);
            assert_eq!(level.len(), frames.div_ceil(size), "level {n}");
        }
        let (fine, coarse) = (&peaks.levels()[0], &peaks.levels()[1]);
        for (i, bucket) in coarse.iter().enumerate() {
            let covered = &fine[i * FACTOR..((i + 1) * FACTOR).min(fine.len())];
            assert_eq!(*bucket, merge(covered));
        }
    }

    #[test]
    fn mono_repeats_and_extra_channels_are_ignored() {
        let mono = build(&[vec![0.5; 16]]);
        assert_eq!(mono.channels, 1);
        assert_eq!(mono.levels()[0][0], [63, 64, 63, 64]);
        let wide = build(&[vec![0.5; 16], vec![-0.5; 16], vec![1.0; 16]]);
        assert_eq!(wide.channels, 2);
        assert_eq!(wide.levels()[0][0], [63, 64, -64, -63]);
    }

    #[test]
    fn cancelling_stops_the_build() {
        assert!(Peaks::build(&[vec![0.0; 64]], &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn five_minutes_of_stereo_take_about_five_megabytes() {
        let frames: usize = 48_000 * 300;
        let buckets: usize = (0..LEVELS)
            .map(|n| frames.div_ceil(BASE * FACTOR.pow(n as u32)))
            .sum();
        assert!(buckets * size_of::<Bucket>() < 5_000_000);
    }
}
