use lumen_core::time::Ticks;

use crate::declick::{MAX_CHANNELS, fade_frames};
use crate::source::{Output, RenderContext, Source};
use crate::timeline::Timeline;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrubStyle {
    Loop { length: Ticks },
    Tape,
}

pub const MAX_SPEED: f64 = 4.0;
const CHUNK: usize = 256;
const SPAN: usize = CHUNK * MAX_SPEED as usize + 4;
const MIN_LOOP_SECONDS: f64 = 0.05;
const FOLLOW_SECONDS: f64 = 0.03;
const INERTIA_SECONDS: f64 = 0.01;
const STILL_SPEED: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Loop(i64, i64),
    Tape(f64),
}

pub(crate) struct Scrubber {
    now: Option<Target>,
    next: Option<Option<Target>>,
    envelope: f32,
    position: i64,
    head: f64,
    speed: f64,
    heads: Vec<f64>,
    scratch: Vec<f32>,
}

impl Default for Scrubber {
    fn default() -> Self {
        Self {
            now: None,
            next: None,
            envelope: 0.0,
            position: 0,
            head: 0.0,
            speed: 0.0,
            heads: vec![0.0; CHUNK],
            scratch: vec![0.0; SPAN * MAX_CHANNELS],
        }
    }
}

impl Scrubber {
    pub fn active(&self) -> bool {
        self.now.is_some()
    }

    pub fn scrub(&mut self, at: i64, end: i64, style: ScrubStyle, rate: u32) {
        let target = match style {
            ScrubStyle::Loop { .. } => {
                let shortest = (MIN_LOOP_SECONDS * f64::from(rate)) as i64;
                Target::Loop(at, end.max(at + shortest.max(1)))
            }
            ScrubStyle::Tape => Target::Tape(at as f64),
        };
        match (self.now, target) {
            (None, _) => self.start(target),
            (Some(Target::Tape(_)), Target::Tape(_)) if self.next.is_none() => {
                self.now = Some(target);
            }
            (Some(now), _) if now == target && self.next.is_none() => {}
            _ => self.next = Some(Some(target)),
        }
    }

    pub fn end(&mut self) {
        if self.now.is_some() {
            self.next = Some(None);
        }
    }

    pub fn silence(&mut self) {
        self.now = None;
        self.next = None;
        self.envelope = 0.0;
    }

    fn start(&mut self, target: Target) {
        self.now = Some(target);
        self.next = None;
        self.envelope = 0.0;
        match target {
            Target::Loop(start, _) => self.position = start,
            Target::Tape(at) => {
                self.head = at;
                self.speed = 0.0;
            }
        }
    }

    pub fn render<S: Source>(
        &mut self,
        source: &mut S,
        timeline: &Timeline,
        rate: u32,
        data: &mut [f32],
        channels: usize,
        frames: usize,
    ) {
        let fade = fade_frames(rate);
        let step = 1.0 / fade as f32;
        let mut offset = 0;
        while offset < frames {
            let Some(now) = self.now else {
                return;
            };
            let fading = self.next.is_some();
            if fading && self.envelope <= 0.0 {
                match self.next.take().flatten() {
                    Some(target) => self.start(target),
                    None => self.silence(),
                }
                continue;
            }
            let mut n = (frames - offset).min(CHUNK);
            if fading {
                n = n.min((self.envelope / step).ceil().max(1.0) as usize);
            }
            let window = Window {
                offset,
                count: n,
                stride: frames,
                channels,
            };
            let mut read = Reading {
                source: &mut *source,
                timeline,
                rate,
            };
            match now {
                Target::Loop(start, end) => {
                    n = n.min((end - self.position) as usize);
                    let window = Window { count: n, ..window };
                    self.render_loop(&mut read, data, window, (start, end), fade);
                }
                Target::Tape(target) => self.render_tape(&mut read, data, window, target),
            }
            let goal = if fading { 0.0 } else { 1.0 };
            for frame in offset..offset + n {
                self.envelope = if goal > self.envelope {
                    (self.envelope + step).min(goal)
                } else {
                    (self.envelope - step).max(goal)
                };
                for channel in 0..channels {
                    data[channel * frames + frame] *= self.envelope;
                }
            }
            offset += n;
        }
    }

    fn render_loop<S: Source>(
        &mut self,
        read: &mut Reading<'_, S>,
        data: &mut [f32],
        at: Window,
        (start, end): (i64, i64),
        fade: usize,
    ) {
        let cx = RenderContext {
            timeline: read.timeline,
            sample_rate: read.rate,
            start: self.position,
            tail: false,
            stopping: false,
            mode: crate::source::Mode::Scrub,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: None,
        };
        let mut out = Output::new(data, at.stride, at.offset, at.count, at.channels);
        read.source.render(&cx, &mut out);
        let length = end - start;
        let edge = (fade as i64).min(length / 2).max(1);
        for k in 0..at.count {
            let pos = self.position + k as i64 - start;
            let gain = ((pos + 1).min(length - pos).min(edge) as f32 / edge as f32).min(1.0);
            for channel in 0..at.channels {
                data[channel * at.stride + at.offset + k] *= gain;
            }
        }
        self.position += at.count as i64;
        if self.position >= end {
            self.position = start;
        }
    }

    fn render_tape<S: Source>(
        &mut self,
        read: &mut Reading<'_, S>,
        data: &mut [f32],
        at: Window,
        target: f64,
    ) {
        let rate = f64::from(read.rate);
        let follow = 1.0 / (FOLLOW_SECONDS * rate);
        let inertia = 1.0 / (INERTIA_SECONDS * rate);
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        for head in &mut self.heads[..at.count] {
            let wanted = ((target - self.head) * follow).clamp(-MAX_SPEED, MAX_SPEED);
            self.speed += (wanted - self.speed) * inertia;
            *head = self.head;
            low = low.min(self.head);
            high = high.max(self.head);
            self.head += self.speed;
        }
        let from = low.floor() as i64;
        let span = ((high.floor() as i64 - from + 2) as usize).min(SPAN);
        let channels = at.channels.min(MAX_CHANNELS);
        self.scratch[..channels * SPAN].fill(0.0);
        let cx = RenderContext {
            timeline: read.timeline,
            sample_rate: read.rate,
            start: from,
            tail: false,
            stopping: false,
            mode: crate::source::Mode::Scrub,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: None,
        };
        let mut out = Output::new(&mut self.scratch, SPAN, 0, span, channels);
        read.source.render(&cx, &mut out);
        let loudness = (self.speed.abs() / STILL_SPEED).min(1.0) as f32;
        for (k, &head) in self.heads[..at.count].iter().enumerate() {
            let x = head - from as f64;
            let i = (x.floor() as usize).min(span.saturating_sub(2));
            let frac = (x - i as f64) as f32;
            for channel in 0..channels {
                let samples = &self.scratch[channel * SPAN..channel * SPAN + span];
                let a = samples[i];
                let b = samples[(i + 1).min(span - 1)];
                data[channel * at.stride + at.offset + k] += (a + (b - a) * frac) * loudness;
            }
        }
    }
}

struct Reading<'a, S> {
    source: &'a mut S,
    timeline: &'a Timeline,
    rate: u32,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    offset: usize,
    count: usize,
    stride: usize,
    channels: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ramp;

    impl Source for Ramp {
        fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
            for channel in 0..out.channels() {
                for (k, sample) in out.channel(channel).iter_mut().enumerate() {
                    *sample += (cx.start + k as i64) as f32;
                }
            }
        }
    }

    const RATE: u32 = 48_000;

    fn run(scrub: &mut Scrubber, frames: usize) -> Vec<f32> {
        let mut data = vec![0.0; frames];
        scrub.render(&mut Ramp, &Timeline::default(), RATE, &mut data, 1, frames);
        data
    }

    #[test]
    fn a_loop_plays_its_step_over_and_over_at_normal_speed() {
        let mut scrub = Scrubber::default();
        scrub.scrub(
            10_000,
            14_800,
            ScrubStyle::Loop {
                length: Ticks::ZERO,
            },
            RATE,
        );
        let out = run(&mut scrub, 3 * 4_800);
        assert_eq!(out[1_000], 11_000.0);
        assert_eq!(out[4_800 + 1_000], 11_000.0, "the second pass");
        assert!(out.iter().all(|s| (0.0..14_800.0).contains(s)));
        assert!(out[4_800] < 10_000.0 / 50.0);
        assert!(out[4_799] < 14_800.0 / 50.0);
    }

    #[test]
    fn short_steps_are_lengthened_and_changes_crossfade() {
        let mut scrub = Scrubber::default();
        scrub.scrub(
            0,
            10,
            ScrubStyle::Loop {
                length: Ticks::ZERO,
            },
            RATE,
        );
        assert_eq!(scrub.now, Some(Target::Loop(0, 2_400)), "50 ms at least");
        run(&mut scrub, 1_000);
        scrub.scrub(
            48_000,
            50_400,
            ScrubStyle::Loop {
                length: Ticks::ZERO,
            },
            RATE,
        );
        let out = run(&mut scrub, 1_000);
        assert!(out[50] < 2_400.0);
        assert_eq!(scrub.now, Some(Target::Loop(48_000, 50_400)));
        assert!(out[999] > 48_000.0 * 0.99);
    }

    #[test]
    fn the_tape_follows_the_hand_and_holding_still_is_silent() {
        let mut scrub = Scrubber::default();
        scrub.scrub(0, 0, ScrubStyle::Tape, RATE);
        let mut read = Vec::new();
        for frame in 1..=60 {
            scrub.scrub(frame * 800, 0, ScrubStyle::Tape, RATE);
            read.extend(run(&mut scrub, 800));
        }
        let late = &read[40_000..40_800];
        let rise = (late[799] - late[0]) / 799.0;
        assert!((0.8..1.2).contains(&rise), "speed {rise}");
        assert!(late[799] > 40_800.0 - 4_800.0, "keeps up: {}", late[799]);
        let still = run(&mut scrub, 48_000);
        assert!(still[47_999].abs() < 1e-3, "{}", still[47_999]);
        scrub.end();
        run(&mut scrub, 1_000);
        assert!(!scrub.active());
    }

    #[test]
    fn backwards_is_the_audio_reversed_and_speed_is_capped() {
        let mut scrub = Scrubber::default();
        scrub.scrub(480_000, 0, ScrubStyle::Tape, RATE);
        run(&mut scrub, 100);
        scrub.scrub(0, 0, ScrubStyle::Tape, RATE);
        let out = run(&mut scrub, 4_800);
        let fall = (out[4_000] - out[4_799]) / 799.0;
        assert!(fall > 0.0, "reads backwards");
        assert!(fall <= MAX_SPEED as f32 + 0.01, "capped: {fall}");
    }
}
