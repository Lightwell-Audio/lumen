//! Click-free transport jumps (M2 slice 6, MT-MEDIA-1). Every
//! discontinuity is a 2 ms linear crossfade: the audio that would have
//! followed the old position (the "tail") fades out while the new audio
//! fades in. Stopping fades the tail out over silence; starting fades in
//! from silence. Loops still wrap on their exact sample: only the gain
//! around the wrap changes.
//!
//! The tail is rendered ahead, at the moment of the jump, into a buffer
//! allocated once, so all of this is real-time safe.

/// Longest fade, in frames: 2 ms at 192 kHz.
pub const MAX_FADE: usize = 384;
/// Channels the tail buffer holds. Wider outputs get the fade-in but no
/// crossfade (none exist in M2).
pub const MAX_CHANNELS: usize = 8;

/// The fade length at `rate`: 2 ms.
pub fn fade_frames(rate: u32) -> usize {
    ((rate / 500) as usize).clamp(1, MAX_FADE)
}

pub struct Declick {
    /// Channel-major, `MAX_FADE` frames per channel.
    tail: Vec<f32>,
    tail_len: usize,
    tail_channels: usize,
    /// Tail frames already mixed in.
    tail_done: usize,
    /// Frames of fade-in still to apply, out of `fade_len`.
    fade_in_left: usize,
    fade_len: usize,
}

impl Default for Declick {
    fn default() -> Self {
        Self {
            tail: vec![0.0; MAX_FADE * MAX_CHANNELS],
            tail_len: 0,
            tail_channels: 0,
            tail_done: 0,
            fade_in_left: 0,
            fade_len: 1,
        }
    }
}

impl Declick {
    /// The buffer to render the tail into, `channels × MAX_FADE`, zeroed;
    /// `None` when there are more channels than the buffer holds.
    pub fn tail_buffer(&mut self, channels: usize) -> Option<&mut [f32]> {
        if channels > MAX_CHANNELS {
            return None;
        }
        let buffer = &mut self.tail[..channels * MAX_FADE];
        buffer.fill(0.0);
        Some(buffer)
    }

    /// Call after rendering `frames` of tail into [`Self::tail_buffer`]:
    /// applies the fade-out and queues it to be mixed in.
    pub fn tail_rendered(&mut self, channels: usize, frames: usize) {
        for channel in 0..channels {
            let samples = &mut self.tail[channel * MAX_FADE..channel * MAX_FADE + frames];
            for (k, sample) in samples.iter_mut().enumerate() {
                *sample *= fade_out(k, frames);
            }
        }
        self.tail_len = frames;
        self.tail_channels = channels;
        self.tail_done = 0;
    }

    /// The next `frames` of new audio fade in from silence.
    pub fn fade_in(&mut self, frames: usize) {
        self.fade_in_left = frames;
        self.fade_len = frames.max(1);
    }

    /// Drops any fade in progress (panic).
    pub fn clear(&mut self) {
        self.tail_len = 0;
        self.tail_done = 0;
        self.fade_in_left = 0;
    }

    /// Whether there is anything left to apply.
    pub fn active(&self) -> bool {
        self.fade_in_left > 0 || self.tail_done < self.tail_len
    }

    /// Applies the fade-in and mixes the tail into `count` frames of
    /// channel-major `data` (`stride` frames per channel) from `offset`.
    pub fn apply(
        &mut self,
        data: &mut [f32],
        stride: usize,
        channels: usize,
        offset: usize,
        count: usize,
    ) {
        for frame in offset..offset + count {
            if !self.active() {
                return;
            }
            if self.fade_in_left > 0 {
                let k = self.fade_len - self.fade_in_left;
                let gain = fade_in(k, self.fade_len);
                for channel in 0..channels {
                    data[channel * stride + frame] *= gain;
                }
                self.fade_in_left -= 1;
            }
            if self.tail_done < self.tail_len {
                for channel in 0..channels.min(self.tail_channels) {
                    data[channel * stride + frame] +=
                        self.tail[channel * MAX_FADE + self.tail_done];
                }
                self.tail_done += 1;
            }
        }
    }
}

/// Gain on frame `k` of an `n`-frame fade-in: 1/n up to 1.
pub fn fade_in(k: usize, n: usize) -> f32 {
    (k + 1) as f32 / n as f32
}

/// Gain on frame `k` of an `n`-frame fade-out: the complement of the
/// fade-in, so a crossfade keeps a constant level.
pub fn fade_out(k: usize, n: usize) -> f32 {
    1.0 - fade_in(k, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crossfade_keeps_a_constant_level() {
        for k in 0..96 {
            assert!((fade_in(k, 96) + fade_out(k, 96) - 1.0).abs() < 1e-6);
        }
        assert_eq!(fade_in(95, 96), 1.0);
        assert_eq!(fade_out(95, 96), 0.0);
    }

    #[test]
    fn a_crossfade_of_equal_signals_is_seamless() {
        // Old and new audio both 0.5: the crossfade must stay at 0.5.
        let mut declick = Declick::default();
        let tail = declick.tail_buffer(2).unwrap();
        tail[..4].fill(0.5);
        tail[MAX_FADE..MAX_FADE + 4].fill(0.5);
        declick.tail_rendered(2, 4);
        declick.fade_in(4);
        let mut data = vec![0.5; 2 * 8];
        declick.apply(&mut data, 8, 2, 0, 8);
        assert!(data.iter().all(|&s| (s - 0.5).abs() < 1e-6), "{data:?}");
        assert!(!declick.active());
    }

    #[test]
    fn the_fade_continues_across_blocks() {
        let mut declick = Declick::default();
        declick.fade_in(4);
        let mut data = vec![1.0; 2];
        declick.apply(&mut data, 2, 1, 0, 2);
        assert_eq!(data, [0.25, 0.5]);
        let mut data = vec![1.0; 3];
        declick.apply(&mut data, 3, 1, 0, 3);
        assert_eq!(data, [0.75, 1.0, 1.0]);
    }

    #[test]
    fn too_many_channels_get_no_tail() {
        assert!(Declick::default().tail_buffer(MAX_CHANNELS + 1).is_none());
    }
}
