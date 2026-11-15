use std::sync::Arc;

use crate::sampler::Sample;
use crate::sampler::table::FLOOR_DB;
use crate::sampler::{FILTER_TOP_HZ, PAIRS};

pub(super) fn db_to_gain(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        (10.0_f64).powf(f64::from(db) / 20.0) as f32
    }
}

fn flush(value: f64) -> f64 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Envelope {
    pub(super) stage: Stage,

    pub(super) level: f32,

    pub(super) release_from: f32,
}

impl Envelope {
    pub(super) const START: Envelope = Envelope {
        stage: Stage::Attack,
        level: 0.0,
        release_from: 0.0,
    };

    pub(super) fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    pub(super) fn release(&mut self) {
        if self.stage != Stage::Done {
            self.release_from = self.level;
            self.stage = Stage::Release;
        }
    }

    #[inline]
    pub(super) fn step(&mut self, times: [f32; 4], rate: u32) {
        let frames = |seconds: f32| (f64::from(seconds) * f64::from(rate)).max(1.0);
        match self.stage {
            Stage::Attack => {
                if times[0] <= 0.0 {
                    self.level = 1.0;
                } else {
                    self.level += (1.0 / frames(times[0])) as f32;
                }
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let target = times[2].clamp(0.0, 1.0);
                if times[1] <= 0.0 {
                    self.level = target;
                } else {
                    self.level -= ((1.0 - f64::from(target)) / frames(times[1])) as f32;
                }
                if self.level <= target {
                    self.level = target;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = times[2].clamp(0.0, 1.0);
            }
            Stage::Release => {
                if times[3] <= 0.0 {
                    self.level = 0.0;
                } else {
                    self.level -= (f64::from(self.release_from) / frames(times[3])) as f32;
                }
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {
                self.level = 0.0;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Fx {
    pub(super) mode: u8,
    pub(super) a: [f64; 3],
    pub(super) k: f64,
}

impl Fx {
    pub(super) const OFF: Fx = Fx {
        mode: 0,
        a: [0.0; 3],
        k: 0.0,
    };

    pub(super) fn new(mode: u8, hz: f32, resonance: f32, rate: u32) -> Self {
        if mode == 0 || rate == 0 || hz >= FILTER_TOP_HZ {
            return Self::OFF;
        }
        let nyquist = f64::from(rate) * 0.5;
        let cutoff = f64::from(hz).clamp(10.0, nyquist * 0.98);

        let g = (std::f64::consts::PI * cutoff / f64::from(rate)).tan();
        let k = 2.0 - 1.9 * f64::from(resonance.clamp(0.0, 1.0));
        let a = 1.0 / (1.0 + g * (g + k));
        Self {
            mode,
            a: [a, g * a, g * g * a],
            k,
        }
    }

    #[inline]
    pub(super) fn run(&self, state: &mut [f64; 2], x: f64) -> f64 {
        if self.mode == 0 {
            return x;
        }
        let [a1, a2, a3] = self.a;
        let v3 = x - state[1];
        let v1 = a1 * state[0] + a2 * v3;
        let v2 = state[1] + a2 * state[0] + a3 * v3;
        state[0] = flush(2.0 * v1 - state[0]);
        state[1] = flush(2.0 * v2 - state[1]);
        if self.mode == 2 {
            x - self.k * v1 - v2
        } else {
            v2
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FrameValues {
    pub(super) pitch: f32,
    pub(super) fine: f32,
    pub(super) mode: u8,
    pub(super) cutoff: f32,
    pub(super) resonance: f32,

    pub(super) velocity: f32,
}

pub(super) struct Voice {
    pub(super) sample: Option<Arc<Sample>>,

    pub(super) start: usize,
    pub(super) length: usize,
    pub(super) reversed: bool,
    pub(super) looping: bool,

    pub(super) phase: f64,

    pub(super) step: f64,

    pub(super) key: u8,
    pub(super) root: u8,

    pub(super) tune: f32,

    pub(super) gain: f32,

    pub(super) sides: [f32; 2],

    pub(super) output: u8,

    pub(super) choke: Option<u8>,
    pub(super) choke_fade: Option<f32>,
    pub(super) env: Envelope,

    pub(super) age: u64,
    pub(super) semitones: f64,

    pub(super) release_seconds: f32,

    pub(super) filter: [[f64; 2]; 2],
    pub(super) cut: Fx,

    pub(super) order: u64,
}

impl Voice {
    pub(super) const IDLE: Voice = Voice {
        sample: None,
        start: 0,
        length: 0,
        reversed: false,
        looping: false,
        phase: 0.0,
        step: 1.0,
        key: 0,
        root: 60,
        tune: 0.0,
        gain: 0.0,

        sides: [0.0; 2],
        output: 0,
        choke: None,
        choke_fade: None,
        env: Envelope {
            stage: Stage::Done,
            level: 0.0,
            release_from: 0.0,
        },
        age: 0,
        semitones: 0.0,
        release_seconds: 0.0,
        filter: [[0.0; 2]; 2],
        cut: Fx::OFF,
        order: 0,
    };

    pub(super) fn playing(&self) -> bool {
        self.sample.is_some() && !self.env.done()
    }

    #[inline]
    pub(super) fn index(&self, at: f64) -> f64 {
        if self.reversed {
            self.start as f64 + (self.length as f64 - 1.0) - at
        } else {
            self.start as f64 + at
        }
    }

    #[inline]
    pub(super) fn read(&self, channel: usize, at: f64) -> f64 {
        let sample = self
            .sample
            .as_ref()
            .expect("a playing voice has its sample");
        let index = self.index(at);
        let base = index.floor();
        let into = index - base;
        if into <= 0.0 {
            return f64::from(sample.at(channel, base as isize));
        }

        let at = |offset: isize| f64::from(sample.at(channel, base as isize + offset));
        let (p0, p1, p2, p3) = (at(-1), at(0), at(1), at(2));
        let (f, f2, f3) = (into, into * into, into * into * into);
        0.5 * ((2.0 * p1)
            + (-p0 + p2) * f
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f3)
    }

    #[inline]
    pub(super) fn advance(&mut self, frames: f64) -> bool {
        self.phase += frames;
        if self.phase < self.length as f64 {
            return true;
        }
        if self.looping && self.length > 0 {
            let length = self.length as f64;
            self.phase -= length * (self.phase / length).floor();
            return true;
        }
        false
    }
}

pub(super) struct Curves {
    pub(super) level: [f32; crate::MOST_FRAMES],
    pub(super) pitch: [f32; crate::MOST_FRAMES],
    pub(super) fine: [f32; crate::MOST_FRAMES],
    pub(super) attack: [f32; crate::MOST_FRAMES],
    pub(super) decay: [f32; crate::MOST_FRAMES],
    pub(super) sustain: [f32; crate::MOST_FRAMES],
    pub(super) release: [f32; crate::MOST_FRAMES],
    pub(super) filter: [f32; crate::MOST_FRAMES],
    pub(super) cutoff: [f32; crate::MOST_FRAMES],
    pub(super) resonance: [f32; crate::MOST_FRAMES],
    pub(super) velocity: [f32; crate::MOST_FRAMES],
}

impl Curves {
    pub(super) fn new() -> Box<Self> {
        Box::new(Self {
            level: [0.0; crate::MOST_FRAMES],
            pitch: [0.0; crate::MOST_FRAMES],
            fine: [0.0; crate::MOST_FRAMES],
            attack: [0.0; crate::MOST_FRAMES],
            decay: [0.0; crate::MOST_FRAMES],
            sustain: [0.0; crate::MOST_FRAMES],
            release: [0.0; crate::MOST_FRAMES],
            filter: [0.0; crate::MOST_FRAMES],
            cutoff: [0.0; crate::MOST_FRAMES],
            resonance: [0.0; crate::MOST_FRAMES],
            velocity: [0.0; crate::MOST_FRAMES],
        })
    }

    pub(super) fn slot(&mut self, index: usize) -> &mut [f32] {
        let all = [
            &mut self.level,
            &mut self.pitch,
            &mut self.fine,
            &mut self.attack,
            &mut self.decay,
            &mut self.sustain,
            &mut self.release,
            &mut self.filter,
            &mut self.cutoff,
            &mut self.resonance,
            &mut self.velocity,
        ];
        all[index]
    }
}

pub(super) struct Pairs {
    pub(super) left: [[f32; crate::MOST_FRAMES]; PAIRS],
    pub(super) right: [[f32; crate::MOST_FRAMES]; PAIRS],
}

impl Pairs {
    pub(super) fn new() -> Box<Self> {
        Box::new(Self {
            left: [[0.0; crate::MOST_FRAMES]; PAIRS],
            right: [[0.0; crate::MOST_FRAMES]; PAIRS],
        })
    }

    pub(super) fn clear(&mut self, frames: usize) {
        for pair in 0..PAIRS {
            self.left[pair][..frames].fill(0.0);
            self.right[pair][..frames].fill(0.0);
        }
    }
}
