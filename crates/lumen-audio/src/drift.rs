//! Clock-drift correction for input that has its own clock (M9 slice R1,
//! ADR-0100).
//!
//! WASAPI opens the input of a device as a stream apart from its output, and
//! the two are not locked to each other: the input arrives in packets on the
//! capture clock while the callback runs on the render clock. [`Corrected`]
//! queues what arrives and hands the callback one block per render block. It
//! keeps the queue near a target by slipping a frame now and then
//! (rubato's `Slip`, with a short crossfade over each splice) instead of
//! resampling, so there is no filter delay and no change to the sound
//! between slips.
//!
//! Nothing here allocates after [`Corrected::new`], so it is safe to run
//! next to the real-time callback.

use rubato::audioadapter_buffers::direct::{InterleavedSlice, SequentialSlice};
use rubato::{Adjustable, FixedAsync, Resampler, Slip};

/// How many frames the queue can hold, in render blocks.
const QUEUE_BLOCKS: usize = 16;

/// Share of the distance to the queue level that each block's reading moves
/// the smoothed level: about a second of memory at 100 blocks per second.
const SMOOTHING: f64 = 0.01;

/// Change in resample ratio per frame the smoothed level is below the
/// target. Small on purpose: a real clock difference is tens of parts per
/// million, and this settles it to a few dozen frames of offset.
const GAIN: f64 = 4.0e-6;

/// A queue between a capture clock and the render clock.
pub struct Corrected {
    channels: usize,
    period: usize,
    /// Frames to keep queued: the level at which delivery starts and the one
    /// the correction steers to.
    target: usize,
    slip: Slip<f32>,
    /// Interleaved frames waiting, oldest first.
    queue: Vec<f32>,
    queued: usize,
    smoothed: f64,
    primed: bool,
    /// Scratch for a block that has to be padded with silence.
    padded: Vec<f32>,
    underruns: u64,
    overruns: u64,
}

impl Corrected {
    /// A queue for `channels` channels delivered `period` frames at a time,
    /// which keeps `target` frames in hand (a block plus the most the device
    /// hands over at once). `None` when `period` is too short to correct.
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
        })
    }

    /// Frames kept in hand: the delay this adds to the input.
    pub fn target(&self) -> usize {
        self.target
    }

    /// Frames waiting.
    #[cfg(test)]
    fn queued(&self) -> usize {
        self.queued
    }

    /// Times the queue ran dry and delivery restarted.
    #[cfg(test)]
    fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Times the queue overflowed and the oldest frames were dropped.
    #[cfg(test)]
    fn overruns(&self) -> u64 {
        self.overruns
    }

    /// Queues `samples`, interleaved. When the queue is full the oldest
    /// frames are dropped to make room.
    pub fn push(&mut self, samples: &[f32]) {
        let channels = self.channels;
        let room = self.queue.len() / channels;
        let mut frames = samples.len() / channels;
        let mut samples = &samples[..frames * channels];
        if frames > room {
            // More than the queue can ever hold: keep the newest.
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

    /// Fills `out` (`channels × period` samples, channel after channel) with
    /// the next block. Silence until the queue has reached the target, and
    /// again after it runs dry.
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
        // A queue running low asks for a ratio above one: fewer input frames
        // per output block. The ratio is clamped to what can be slipped.
        let ratio = 1.0 + GAIN * (self.target as f64 - self.smoothed);
        let _ = self.slip.set_resample_ratio(ratio, false);

        let channels = self.channels;
        let need = self.slip.input_frames_next();
        let Ok(mut output) = SequentialSlice::new_mut(out, channels, self.period) else {
            out.fill(0.0);
            return;
        };
        if self.queued < need {
            // Ran dry: play what there is, then silence, and wait for the
            // queue to fill again.
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

    /// A capture that runs `ratio` times as fast as the render clock,
    /// delivering `packet` frames at a time, for `seconds` of render time.
    /// Returns what was heard (channel 0) and the queue.
    fn run(ratio: f64, packet: usize, seconds: usize) -> (Vec<f32>, Corrected) {
        let mut queue = Corrected::new(CHANNELS, PERIOD, PERIOD + packet).expect("queue");
        let blocks = seconds * 48_000 / PERIOD;
        let mut heard = Vec::with_capacity(blocks * PERIOD);
        let mut produced = 0.0_f64; // capture frames made so far
        let mut delivered = 0_usize; // capture frames handed to the queue
        let mut out = vec![0.0; CHANNELS * PERIOD];
        for _ in 0..blocks {
            produced += PERIOD as f64 * ratio;
            // The device hands over whole packets as they complete.
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
        queue.pull(&mut out); // re-priming
        assert!(out.iter().all(|&s| s == 0.0));
        queue.push(&[0.25; 64]);
        queue.pull(&mut out);
        assert!(out.iter().all(|&s| s == 0.25));
    }

    #[test]
    fn a_slip_leaves_no_click() {
        // A pure tone through a clock that is 0.05% fast: the steepest step
        // between neighbouring samples stays near the tone's own.
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
}
