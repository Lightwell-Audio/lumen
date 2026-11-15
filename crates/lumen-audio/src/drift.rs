use rubato::audioadapter_buffers::direct::{InterleavedSlice, SequentialSlice};
use rubato::{Adjustable, FixedAsync, Resampler, Slip};

const QUEUE_BLOCKS: usize = 16;

const SMOOTHING: f64 = 0.01;

const GAIN: f64 = 4.0e-6;

const BEND: f64 = 0.005;

fn ratio_for(target: f64, smoothed: f64) -> f64 {
    (1.0 + GAIN * (target - smoothed)).clamp(1.0 - BEND, 1.0 + BEND)
}

pub struct Corrected {
    channels: usize,
    period: usize,
    target: usize,
    slip: Slip<f32>,
    queue: Vec<f32>,
    queued: usize,
    smoothed: f64,
    primed: bool,
    padded: Vec<f32>,
    underruns: u64,
    overruns: u64,
    ratio: f64,
    ratio_failures: u64,
}

impl Corrected {
    pub fn new(channels: usize, period: usize, target: usize) -> Option<Self> {
        let slip = Slip::new(period, channels, FixedAsync::Output).ok()?;
        let target = target.max(period);
        let room = (QUEUE_BLOCKS * period).max(2 * target);
        Some(Self {
            channels,
            period,
            target,
            slip,
            queue: vec![0.0; room * channels],
            queued: 0,
            smoothed: target as f64,
            primed: false,
            padded: vec![0.0; 2 * period * channels],
            underruns: 0,
            overruns: 0,
            ratio: 1.0,
            ratio_failures: 0,
        })
    }

    pub fn target(&self) -> usize {
        self.target
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        self.queued
    }

    #[cfg(test)]
    fn underruns(&self) -> u64 {
        self.underruns
    }

    #[cfg(test)]
    fn overruns(&self) -> u64 {
        self.overruns
    }

    #[cfg(test)]
    fn ratio(&self) -> f64 {
        self.ratio
    }

    #[cfg(test)]
    fn ratio_failures(&self) -> u64 {
        self.ratio_failures
    }

    fn steer(&mut self, ratio: f64) {
        match self.slip.set_resample_ratio(ratio, false) {
            Ok(()) => self.ratio = ratio,
            Err(_) => {
                self.ratio_failures += 1;
                let _ = self.slip.set_resample_ratio(self.ratio, false);
            }
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        let channels = self.channels;
        let room = self.queue.len() / channels;
        let mut frames = samples.len() / channels;
        let mut samples = &samples[..frames * channels];
        if frames > room {
            samples = &samples[(frames - room) * channels..];
            frames = room;
            self.overruns += 1;
        }
        if self.queued + frames > room {
            let drop = self.queued + frames - room;
            self.queue
                .copy_within(drop * channels..self.queued * channels, 0);
            self.queued -= drop;
            self.overruns += 1;
        }
        let at = self.queued * channels;
        self.queue[at..at + samples.len()].copy_from_slice(samples);
        self.queued += frames;
    }

    pub fn pull(&mut self, out: &mut [f32]) {
        if !self.primed {
            if self.queued < self.target {
                out.fill(0.0);
                return;
            }
            self.primed = true;
            self.smoothed = self.queued as f64;
        }
        self.smoothed += SMOOTHING * (self.queued as f64 - self.smoothed);
        self.steer(ratio_for(self.target as f64, self.smoothed));

        let channels = self.channels;
        let need = self.slip.input_frames_next();
        let Ok(mut output) = SequentialSlice::new_mut(out, channels, self.period) else {
            out.fill(0.0);
            return;
        };
        if self.queued < need {
            self.underruns += 1;
            let have = self.queued * channels;
            self.padded[..have].copy_from_slice(&self.queue[..have]);
            self.padded[have..need * channels].fill(0.0);
            if let Ok(input) =
                InterleavedSlice::new(&self.padded[..need * channels], channels, need)
            {
                let _ = self.slip.process_into_buffer(&input, &mut output, None);
            }
            self.queued = 0;
            self.primed = false;
            return;
        }
        if let Ok(input) = InterleavedSlice::new(&self.queue[..need * channels], channels, need) {
            let _ = self.slip.process_into_buffer(&input, &mut output, None);
        }
        self.queue
            .copy_within(need * channels..self.queued * channels, 0);
        self.queued -= need;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: usize = 480;
    const CHANNELS: usize = 2;

    fn run(ratio: f64, packet: usize, seconds: usize) -> (Vec<f32>, Corrected) {
        let mut queue = Corrected::new(CHANNELS, PERIOD, PERIOD + packet).expect("queue");
        let blocks = seconds * 48_000 / PERIOD;
        let mut heard = Vec::with_capacity(blocks * PERIOD);
        let mut produced = 0.0_f64;
        let mut delivered = 0_usize;
        let mut out = vec![0.0; CHANNELS * PERIOD];
        for _ in 0..blocks {
            produced += PERIOD as f64 * ratio;
            while (delivered + packet) as f64 <= produced {
                let mut samples = Vec::with_capacity(packet * CHANNELS);
                for frame in delivered..delivered + packet {
                    let value = (frame as f32 * 0.01).sin();
                    samples.extend([value, -value]);
                }
                queue.push(&samples);
                delivered += packet;
            }
            queue.pull(&mut out);
            heard.extend_from_slice(&out[..PERIOD]);
        }
        (heard, queue)
    }

    #[test]
    fn a_capture_clock_that_is_fast_or_slow_is_followed_without_running_dry() {
        for ratio in [1.0, 1.0001, 0.9999, 1.0003, 0.9997] {
            let (_, queue) = run(ratio, 480, 60);
            assert_eq!(queue.underruns(), 0, "ratio {ratio}");
            assert_eq!(queue.overruns(), 0, "ratio {ratio}");
            let level = queue.queued() as i64 - queue.target() as i64;
            assert!(level.abs() < 1_000, "ratio {ratio}: {level} frames off");
        }
    }

    #[test]
    fn nothing_is_delivered_until_the_target_is_queued_and_then_the_signal_is_whole() {
        let mut queue = Corrected::new(1, 64, 128).expect("queue");
        let mut out = vec![1.0; 64];
        queue.push(&[0.5; 100]);
        queue.pull(&mut out);
        assert!(out.iter().all(|&s| s == 0.0), "still filling");
        queue.push(&[0.5; 100]);
        queue.pull(&mut out);
        assert!(out.iter().all(|&s| s == 0.5), "a plain copy with no drift");
    }

    #[test]
    fn a_queue_that_runs_dry_plays_silence_and_starts_again() {
        let mut queue = Corrected::new(1, 64, 64).expect("queue");
        let mut out = vec![0.0; 64];
        queue.push(&[0.25; 64]);
        queue.pull(&mut out);
        assert_eq!(queue.underruns(), 0);
        queue.pull(&mut out);
        assert_eq!(queue.underruns(), 1);
        assert!(out.iter().all(|&s| s == 0.0));
        queue.pull(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        queue.push(&[0.25; 64]);
        queue.pull(&mut out);
        assert!(out.iter().all(|&s| s == 0.25));
    }

    #[test]
    fn a_slip_leaves_no_click() {
        let (heard, queue) = run(1.0005, 480, 20);
        assert_eq!(queue.underruns(), 0);
        let steepest = heard[PERIOD * 100..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(steepest < 0.05, "a step of {steepest}");
    }

    #[test]
    fn a_flood_keeps_the_newest_frames() {
        let mut queue = Corrected::new(1, 64, 64).expect("queue");
        let room = QUEUE_BLOCKS * 64;
        let flood: Vec<f32> = (0..room * 3).map(|i| i as f32).collect();
        queue.push(&flood);
        assert_eq!(queue.queued(), room);
        let mut out = vec![0.0; 64];
        queue.pull(&mut out);
        assert_eq!(out[0], (room * 3 - room) as f32);
    }

    #[test]
    fn a_period_too_short_to_correct_is_refused() {
        assert!(Corrected::new(2, 2, 2).is_none());
    }

    #[test]
    fn an_extreme_estimate_is_clamped_to_what_a_slip_can_follow() {
        assert_eq!(ratio_for(1_000.0, -1.0e9), 1.0 + BEND);
        assert_eq!(ratio_for(1_000.0, 1.0e9), 1.0 - BEND);
        assert_eq!(ratio_for(1_000.0, 1_000.0), 1.0);
        let ratio = ratio_for(1_000.0, 900.0);
        assert!((ratio - (1.0 + 4.0e-4)).abs() < 1e-12, "{ratio}");
    }

    #[test]
    fn a_runaway_level_run_stays_inside_the_clamp() {
        let mut queue = Corrected::new(1, 64, 64).expect("queue");
        queue.push(&[0.0; 64]);
        let mut out = vec![0.0; 64];
        queue.pull(&mut out);
        queue.smoothed = -1.0e9;
        queue.pull(&mut out);
        assert_eq!(queue.ratio(), 1.0 + BEND);
        assert_eq!(queue.ratio_failures(), 0, "the clamp kept it in range");
    }

    #[test]
    fn a_refused_ratio_keeps_the_last_one_and_is_counted() {
        let mut queue = Corrected::new(CHANNELS, PERIOD, PERIOD).expect("queue");
        assert_eq!(queue.ratio(), 1.0);
        queue.steer(1.5);
        assert_eq!(queue.ratio_failures(), 1);
        assert_eq!(queue.ratio(), 1.0, "the last ratio that worked is back");
        queue.steer(1.001);
        assert_eq!(
            queue.ratio_failures(),
            1,
            "an accepted change is no failure"
        );
        assert!((queue.ratio() - 1.001).abs() < 1e-12, "{}", queue.ratio());
        let mut out = vec![0.0; CHANNELS * PERIOD];
        queue.push(&[0.5; CHANNELS * PERIOD]);
        queue.pull(&mut out);
        assert!(out.iter().all(|&x| x == 0.5));
    }
}
