const SETTLE: f64 = 0.5;
const JUMP: f64 = 0.25;
const LEAD: f64 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Reading {
    pub playing: bool,
    pub sample: i64,
    pub sample_rate: u32,
    pub looped: Option<(i64, i64)>,
}

#[derive(Debug, Default)]
pub(crate) struct Clock {
    run: Option<Run>,
}

#[derive(Debug, Clone, Copy)]
struct Run {
    sample: f64,
    time: f64,
    reported: i64,
}

impl Clock {
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
        let sample = (predicted + error * lean)
            .max(run.sample)
            .min(reported as f64 + LEAD * rate);
        self.run = Some(Run {
            sample,
            time: now,
            reported,
        });
        match looped {
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
        for pair in shown[60..].windows(2) {
            let step = pair[1] - pair[0];
            assert!((step - ideal).abs() < ideal * 0.05, "step {step}");
        }
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
