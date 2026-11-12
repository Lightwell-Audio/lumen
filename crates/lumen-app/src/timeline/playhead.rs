//! A smooth playhead while playing (owner, after slice 7: Follow was
//! choppy and jittered now and then).
//!
//! The engine publishes its position once per audio block, and blocks do not
//! line up with frames: at 512 samples and 48 kHz a block is 10.7 ms and a
//! frame 16.7 ms, so one frame moves the playhead one block and the next two.
//! [`Clock`] runs on the frame clock instead, at the sample rate, and leans
//! gently towards what the engine reports so it never drifts. Jumps (Play,
//! a locate) start it again at the engine's position; a loop wrap moves it
//! back by exactly the loop, so it stays smooth across the wrap.

/// How quickly the clock leans towards the engine (seconds): long enough
/// that the block-sized steps in what the engine reports average out.
const SETTLE: f64 = 0.5;
/// A difference larger than this (seconds) is a jump, not drift.
const JUMP: f64 = 0.25;
/// The clock runs ahead of the engine by at most this (seconds), so a stalled
/// device does not carry the playhead on.
const LEAD: f64 = 0.1;

/// What the engine reported this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Reading {
    pub playing: bool,
    /// The next sample the engine will play.
    pub sample: i64,
    pub sample_rate: u32,
    /// The active loop in samples, `[start, end)`.
    pub looped: Option<(i64, i64)>,
}

#[derive(Debug, Default)]
pub(crate) struct Clock {
    run: Option<Run>,
}

#[derive(Debug, Clone, Copy)]
struct Run {
    /// Where the clock was at `time`; past the loop end until the engine
    /// wraps.
    sample: f64,
    time: f64,
    /// The engine's sample at the last frame.
    reported: i64,
}

impl Clock {
    /// The playhead's sample at `now` (seconds on the frame clock). Called
    /// once per frame.
    pub(crate) fn update(&mut self, now: f64, reading: Reading) -> f64 {
        let reported = reading.sample;
        if !reading.playing || reading.sample_rate == 0 {
            self.run = None;
            return reported as f64;
        }
        let rate = f64::from(reading.sample_rate);
        let Some(mut run) = self.run else {
            return self.restart(now, reported);
        };
        let looped = reading.looped.filter(|(start, end)| end > start);
        if reported < run.reported {
            match looped {
                // The engine wrapped at the loop end: so does the clock.
                Some((start, end)) if run.reported < end && reported >= start => {
                    run.sample -= (end - start) as f64;
                }
                _ => return self.restart(now, reported),
            }
        }
        let elapsed = (now - run.time).max(0.0);
        let predicted = run.sample + elapsed * rate;
        let error = reported as f64 - predicted;
        if error.abs() > JUMP * rate {
            return self.restart(now, reported);
        }
        let lean = 1.0 - (-elapsed / SETTLE).exp();
        // Never backwards, and never far ahead of the engine.
        let sample = (predicted + error * lean)
            .max(run.sample)
            .min(reported as f64 + LEAD * rate);
        self.run = Some(Run {
            sample,
            time: now,
            reported,
        });
        match looped {
            // Until the engine wraps, show the wrap already.
            Some((start, end)) if reported < end && sample >= end as f64 => {
                sample - (end - start) as f64
            }
            _ => sample,
        }
    }

    fn restart(&mut self, now: f64, reported: i64) -> f64 {
        self.run = Some(Run {
            sample: reported as f64,
            time: now,
            reported,
        });
        reported as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const BLOCK: i64 = 512;
    const FRAME: f64 = 1.0 / 60.0;

    fn playing(sample: i64) -> Reading {
        Reading {
            playing: true,
            sample,
            sample_rate: RATE,
            looped: None,
        }
    }

    /// The engine's position at `time`, reported once per block.
    fn engine(time: f64) -> i64 {
        (time * f64::from(RATE)) as i64 / BLOCK * BLOCK
    }

    #[test]
    fn steps_are_even_although_blocks_and_frames_do_not_line_up() {
        let mut clock = Clock::default();
        let mut shown = Vec::new();
        for frame in 0..600 {
            let now = frame as f64 * FRAME;
            shown.push(clock.update(now, playing(engine(now))));
        }
        let ideal = f64::from(RATE) * FRAME;
        // The raw position moves by one block or two; after settling the
        // clock moves by the frame's worth within a few percent.
        for pair in shown[60..].windows(2) {
            let step = pair[1] - pair[0];
            assert!((step - ideal).abs() < ideal * 0.05, "step {step}");
        }
        // And it keeps up with the engine.
        let last = *shown.last().unwrap();
        let now = 599.0 * FRAME;
        assert!((last - now * f64::from(RATE)).abs() < 2.0 * BLOCK as f64);
    }

    #[test]
    fn a_locate_jumps_at_once() {
        let mut clock = Clock::default();
        clock.update(0.0, playing(0));
        clock.update(FRAME, playing(800));
        assert_eq!(clock.update(2.0 * FRAME, playing(480_000)), 480_000.0);
        // Back, too.
        assert_eq!(clock.update(3.0 * FRAME, playing(1_000)), 1_000.0);
    }

    #[test]
    fn stopped_shows_the_engine_exactly() {
        let mut clock = Clock::default();
        clock.update(0.0, playing(0));
        let stopped = Reading {
            playing: false,
            ..playing(12_345)
        };
        assert_eq!(clock.update(FRAME, stopped), 12_345.0);
    }

    #[test]
    fn a_loop_wrap_stays_smooth() {
        let looped = Some((0, 48_000));
        let at = |sample| Reading {
            looped,
            ..playing(sample)
        };
        let mut clock = Clock::default();
        let mut shown = Vec::new();
        for frame in 0..240 {
            let now = frame as f64 * FRAME;
            shown.push(clock.update(now, at(engine(now) % 48_000)));
        }
        let ideal = f64::from(RATE) * FRAME;
        for pair in shown[60..].windows(2) {
            let mut step = pair[1] - pair[0];
            if step < 0.0 {
                step += 48_000.0;
            }
            assert!((step - ideal).abs() < ideal * 0.05, "step {step}");
            assert!((0.0..48_000.0).contains(&pair[1]), "inside the loop");
        }
    }

    #[test]
    fn a_stalled_device_does_not_carry_the_playhead_on() {
        let mut clock = Clock::default();
        clock.update(0.0, playing(0));
        let mut shown = 0.0;
        for frame in 1..120 {
            shown = clock.update(frame as f64 * FRAME, playing(BLOCK));
        }
        assert!(shown <= BLOCK as f64 + LEAD * f64::from(RATE));
    }
}
