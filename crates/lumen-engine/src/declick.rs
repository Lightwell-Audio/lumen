pub const MAX_FADE: usize = 384;
pub const MAX_CHANNELS: usize = 8;

pub fn fade_frames(rate: u32) -> usize {
    ((rate / 500) as usize).clamp(1, MAX_FADE)
}

pub struct Declick {
    tail: Vec<f32>,
    tail_len: usize,
    tail_channels: usize,
    tail_done: usize,
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
    pub fn tail_buffer(&mut self, channels: usize) -> Option<&mut [f32]> {
        if channels > MAX_CHANNELS {
            return None;
        }
        let buffer = &mut self.tail[..channels * MAX_FADE];
        buffer.fill(0.0);
        Some(buffer)
    }

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

    pub fn fade_in(&mut self, frames: usize) {
        self.fade_in_left = frames;
        self.fade_len = frames.max(1);
    }

    pub fn clear(&mut self) {
        self.tail_len = 0;
        self.tail_done = 0;
        self.fade_in_left = 0;
    }

    pub fn active(&self) -> bool {
        self.fade_in_left > 0 || self.tail_done < self.tail_len
    }

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

pub fn fade_in(k: usize, n: usize) -> f32 {
    (k + 1) as f32 / n as f32
}

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
