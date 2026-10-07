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
