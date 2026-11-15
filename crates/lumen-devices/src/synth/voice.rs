use std::f64::consts::{PI, TAU};

use super::det::{exp, exp2, sin_cos, tan};
use super::params::{self, COUNT};
use super::*;

pub(super) const DERIVE_FRAMES: u64 = 32;

pub(super) const GLIDE_FRAMES: u64 = 8;

pub(super) const SINE: u8 = 0;
pub(super) const TRIANGLE: u8 = 1;
pub(super) const SAW: u8 = 2;
pub(super) const SQUARE: u8 = 3;

pub(super) const LOW_12: u8 = 0;
pub(super) const LOW_24: u8 = 1;
pub(super) const HIGH_12: u8 = 2;
pub(super) const BAND_12: u8 = 3;

pub(super) const LFO_OFF: u8 = 0;
pub(super) const LFO_PITCH: u8 = 1;
pub(super) const LFO_CUTOFF: u8 = 2;
pub(super) const LFO_AMP: u8 = 3;

const LFO_OCTAVES: f32 = 4.0;
const CASCADE_Q: f32 = 0.707;

#[derive(Debug, Clone, Copy)]
pub(super) struct Osc {
    pub(super) wave: u8,
    pub(super) cents: f32,
    pub(super) pan: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Env {
    pub(super) attack: f32,
    pub(super) decay: f32,
    pub(super) sustain: f32,
    pub(super) release: f32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Settings {
    pub(super) osc: [Osc; 2],
    pub(super) filter: u8,
    pub(super) amp: Env,
    pub(super) fenv: Env,
    pub(super) lfo: u8,
    pub(super) voices: usize,
    pub(super) glide: f32,
}

impl Settings {
    pub(super) const IDLE: Settings = Settings {
        osc: [
            Osc {
                wave: SAW,
                cents: 0.0,
                pan: 0.0,
            },
            Osc {
                wave: SAW,
                cents: 0.0,
                pan: 0.0,
            },
        ],
        filter: LOW_12,
        amp: Env {
            attack: 0.0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
        },
        fenv: Env {
            attack: 0.0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
        },
        lfo: LFO_OFF,
        voices: 1,
        glide: 0.0,
    };

    pub(super) fn from_values(values: &[f32; COUNT]) -> Self {
        let value = |id: u32| params::dsp_value(id, values[id as usize - 1]);
        let choice = |id: u32| {
            params::snapped(id, values[id as usize - 1])
                .round()
                .max(0.0) as u8
        };
        let osc = |shape: u32, octave: u32, detune: u32, pan: u32| Osc {
            wave: choice(shape),
            cents: value(octave) * 1_200.0 + value(detune),
            pan: value(pan) / 100.0,
        };
        Self {
            osc: [
                osc(
                    params::OSC1_SHAPE,
                    params::OSC1_OCTAVE,
                    params::OSC1_DETUNE,
                    params::OSC1_PAN,
                ),
                osc(
                    params::OSC2_SHAPE,
                    params::OSC2_OCTAVE,
                    params::OSC2_DETUNE,
                    params::OSC2_PAN,
                ),
            ],
            filter: choice(params::FILTER_TYPE),
            amp: Env {
                attack: value(params::AMP_ATTACK),
                decay: value(params::AMP_DECAY),
                sustain: value(params::AMP_SUSTAIN) / 100.0,
                release: value(params::AMP_RELEASE),
            },
            fenv: Env {
                attack: value(params::FILTER_ATTACK),
                decay: value(params::FILTER_DECAY),
                sustain: value(params::FILTER_SUSTAIN) / 100.0,
                release: value(params::FILTER_RELEASE),
            },
            lfo: choice(params::LFO_TARGET),
            voices: (value(params::VOICES).round() as usize).clamp(1, MOST_VOICES),
            glide: value(params::GLIDE),
        }
    }

    pub(super) fn at(curves: &Curves<'_>, at: usize) -> Self {
        let mut values = [0.0; COUNT];
        for (slot, info) in values.iter_mut().zip(params::TABLE) {
            *slot = curves.at(info.id, at);
        }
        Self::from_values(&values)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
    Steal,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EnvState {
    pub(super) stage: Stage,
    value: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
}

impl EnvState {
    pub(super) const IDLE: Self = Self {
        stage: Stage::Off,
        value: 0.0,
        attack_step: 1.0,
        decay_coef: 1.0,
        release_coef: 1.0,
    };

    fn retune(&mut self, env: &Env, rate: f32) {
        self.attack_step = 1.0 / (env.attack.max(0.001) * rate);
        self.decay_coef = coef(env.decay, rate);
        self.release_coef = coef(env.release, rate);
    }

    fn restart(&mut self, env: &Env, rate: f32) {
        self.stage = Stage::Attack;
        self.value = 0.0;
        self.retune(env, rate);
    }

    fn release(&mut self) {
        if matches!(self.stage, Stage::Attack | Stage::Decay | Stage::Sustain) {
            self.stage = Stage::Release;
        }
    }

    #[inline(always)]
    fn step(&mut self, env: &Env) -> f32 {
        match self.stage {
            Stage::Attack => {
                self.value += self.attack_step;
                if self.value >= 1.0 {
                    self.value = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.value += (env.sustain - self.value) * self.decay_coef;
                if (self.value - env.sustain).abs() < SILENT {
                    self.value = env.sustain;
                    self.stage = if env.sustain <= SILENT {
                        Stage::Off
                    } else {
                        Stage::Sustain
                    };
                }
            }
            Stage::Release => {
                self.value -= self.value * self.release_coef;
                if self.value < SILENT {
                    self.value = 0.0;
                    self.stage = Stage::Off;
                }
            }
            Stage::Sustain => {
                self.value += (env.sustain - self.value) * self.decay_coef;
            }
            Stage::Off | Stage::Steal => {}
        }
        self.value
    }
}

pub(super) const SLOT_COUNT: usize = MOST_FRAMES / GLIDE_FRAMES as usize + 1;

pub(super) struct Slots {
    first: usize,
    step: usize,
    len: usize,
    sounds: [Settings; SLOT_COUNT],
    lfo: [f32; SLOT_COUNT],
}

impl Slots {
    pub(super) const EMPTY: Slots = Slots {
        first: 0,
        step: DERIVE_FRAMES as usize,
        len: 0,
        sounds: [Settings::IDLE; SLOT_COUNT],
        lfo: [0.0; SLOT_COUNT],
    };

    pub(super) fn fill(
        &mut self,
        curves: &Curves<'_>,
        start: u64,
        frames: usize,
        tempo: f32,
        step: usize,
    ) {
        self.step = step;
        self.first = ((step - start as usize % step) % step).min(frames.saturating_sub(1));
        self.len = 0;
        let mut at = self.first;
        while at < frames && self.len < SLOT_COUNT {
            let sound = Settings::at(curves, at);
            self.sounds[self.len] = sound;
            self.lfo[self.len] = lfo_rate(curves, at, tempo);
            self.len += 1;
            at += step;
        }
    }

    #[inline(always)]
    pub(super) fn at(&self, at: usize) -> Option<(&Settings, f32)> {
        let off = at.checked_sub(self.first)?;
        if !off.is_multiple_of(self.step) {
            return None;
        }
        let index = off / self.step;
        (index < self.len).then(|| (&self.sounds[index], self.lfo[index]))
    }
}

fn lfo_rate(curves: &Curves<'_>, at: usize, tempo: f32) -> f32 {
    let sync = params::snapped(params::LFO_SYNC, curves.at(params::LFO_SYNC, at)) >= 0.5;
    if sync {
        let division =
            params::snapped(params::LFO_DIVISION, curves.at(params::LFO_DIVISION, at)) as usize;
        let beats = params::DIVISIONS.get(division).map_or(1.0, |&(_, b)| b);
        tempo / 60.0 / beats
    } else {
        params::dsp_value(params::LFO_RATE, curves.at(params::LFO_RATE, at))
    }
}

pub(super) struct Curves<'a> {
    data: &'a [[f32; MOST_FRAMES]; COUNT],
    pub(super) start: u64,
    frames: usize,
}

impl<'a> Curves<'a> {
    pub(super) fn new(data: &'a [[f32; MOST_FRAMES]; COUNT], start: u64, frames: usize) -> Self {
        Self {
            data,
            start,
            frames,
        }
    }

    #[inline(always)]
    pub(super) fn at(&self, id: u32, at: usize) -> f32 {
        let at = at.min(self.frames.saturating_sub(1));
        self.data[id as usize - 1][at]
    }
}

#[derive(Clone, Copy)]
pub(super) struct Voice {
    pub(super) stage: Stage,
    pub(super) channel: u8,
    pub(super) key: u8,
    pub(super) down: bool,
    pub(super) pedal: bool,
    pub(super) age: u64,
    sound: Settings,
    vel: f32,
    hz: f32,
    glide_oct: f32,
    glide: u32,
    glide_frames: u32,
    freq: [f32; 2],
    phase: [f32; 2],
    lfo_phase: f32,
    lfo_hz: f32,
    noise_state: u32,
    amp_env: EnvState,
    filter_env: EnvState,
    ic: [[f32; 2]; 2],
    ic2: [[f32; 2]; 2],
    coefs: Option<(u32, u32, Coefficients, Coefficients)>,
    filter_old: u8,
    filter_mix: f32,
    ic_old: [[f32; 2]; 2],
    ic2_old: [[f32; 2]; 2],
    corner: (u32, u32, f32),
    pub(super) steal: f32,
    pub(super) pending: Option<super::Waiting>,
    pans: [[f32; 2]; 2],
    shapes: [u8; 2],
    shape_old: [u8; 2],
    shape_mix: [f32; 2],
}

#[derive(Clone, Copy)]
struct Coefficients {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Coefficients {
    fn new(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { k, a1, a2, a3 }
    }
}

pub(super) const IDLE: Voice = Voice {
    stage: Stage::Off,
    channel: 0,
    key: 0,
    down: false,
    pedal: false,
    age: 0,
    sound: Settings {
        osc: [
            Osc {
                wave: SAW,
                cents: 0.0,
                pan: 0.0,
            },
            Osc {
                wave: SAW,
                cents: 0.0,
                pan: 0.0,
            },
        ],
        filter: LOW_12,
        amp: Env {
            attack: 0.0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
        },
        fenv: Env {
            attack: 0.0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
        },
        lfo: LFO_OFF,
        voices: 1,
        glide: 0.0,
    },
    vel: 0.0,
    hz: 0.0,
    glide_oct: 0.0,
    glide: 0,
    glide_frames: 1,
    freq: [0.0; 2],
    phase: [0.0; 2],
    lfo_phase: 0.0,
    lfo_hz: 0.0,
    noise_state: 0,
    amp_env: EnvState::IDLE,
    filter_env: EnvState::IDLE,
    ic: [[0.0; 2]; 2],
    ic2: [[0.0; 2]; 2],
    coefs: None,
    corner: (0, 0, 1_000.0),
    filter_old: LOW_12,
    filter_mix: 1.0,
    ic_old: [[0.0; 2]; 2],
    ic2_old: [[0.0; 2]; 2],
    steal: 1.0,
    pending: None,
    pans: [[0.0; 2]; 2],
    shapes: [SAW; 2],
    shape_old: [SAW; 2],
    shape_mix: [1.0; 2],
};

pub(super) struct Span<'a> {
    pub(super) rate: f32,
    pub(super) bend: f32,
    pub(super) open: f32,
    pub(super) sound: Settings,
    pub(super) derive: u64,
    pub(super) changing: bool,
    pub(super) curves: &'a Curves<'a>,
    pub(super) slots: &'a Slots,
}

pub(super) fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (f64::from(pan.clamp(-1.0, 1.0)) + 1.0) * 0.25 * PI;
    let (sin, cos) = sin_cos(angle);
    [cos as f32, sin as f32]
}

pub(super) fn coef(seconds: f32, rate: f32) -> f32 {
    (1.0 - exp(-4.6 / (f64::from(seconds.max(1e-4)) * f64::from(rate)))) as f32
}

impl Voice {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start(
        &mut self,
        channel: u8,
        key: u8,
        velocity: u8,
        params: &Settings,
        rate: f32,
        age: u64,
        glide_oct: f32,
        glide: f32,
    ) {
        let hz = 440.0 * exp2((f64::from(key) - 69.0) / 12.0) as f32;
        let frames = if glide > 0.0 {
            ((glide * rate) as u32).max(1)
        } else {
            0
        };
        *self = Voice {
            stage: Stage::Attack,
            channel,
            key,
            down: true,
            pedal: false,
            age,
            sound: *params,
            vel: f32::from(velocity) / 127.0,
            hz,
            glide_oct,
            glide: frames,
            glide_frames: frames.max(1),
            freq: [0.0; 2],
            phase: [0.0, 0.25],
            lfo_phase: 0.0,
            lfo_hz: 0.0,
            noise_state: 0x9E37_79B9
                ^ (age as u32).wrapping_mul(0x85EB_CA6B)
                ^ u32::from(key).wrapping_mul(0xC2B2_AE35),
            amp_env: EnvState::IDLE,
            filter_env: EnvState::IDLE,
            ic: [[0.0; 2]; 2],
            ic2: [[0.0; 2]; 2],
            coefs: None,
            corner: (0, 0, 1_000.0),
            filter_old: params.filter,
            filter_mix: 1.0,
            ic_old: [[0.0; 2]; 2],
            ic2_old: [[0.0; 2]; 2],
            steal: 1.0,
            pending: None,
            pans: [[0.0; 2]; 2],
            shapes: [params.osc[0].wave, params.osc[1].wave],
            shape_old: [params.osc[0].wave, params.osc[1].wave],
            shape_mix: [1.0; 2],
        };
        self.amp_env.restart(&params.amp, rate);
        self.filter_env.restart(&params.fenv, rate);
        self.refresh(rate, None);
    }

    pub(super) fn refresh(&mut self, rate: f32, arriving: Option<&Settings>) {
        let p = self.sound;
        for n in 0..2 {
            let wanted = match arriving {
                Some(target) if target.osc[n].wave != self.shapes[n] => target.osc[n].wave,
                _ => p.osc[n].wave,
            };
            if wanted != self.shapes[n] {
                self.shape_old[n] = self.shapes[n];
                self.shapes[n] = wanted;
                self.shape_mix[n] = 0.0;
            }
        }
        let wanted_filter = match arriving {
            Some(target) if target.filter != self.sound.filter => target.filter,
            _ => p.filter,
        };
        if wanted_filter != self.sound.filter {
            self.filter_old = self.sound.filter;
            self.sound.filter = wanted_filter;
            self.ic_old = self.ic;
            self.ic2_old = self.ic2;
            self.filter_mix = 0.0;
        }
        self.freq = [
            (f64::from(self.hz) * exp2(f64::from(p.osc[0].cents) / 1_200.0)) as f32,
            (f64::from(self.hz) * exp2(f64::from(p.osc[1].cents) / 1_200.0)) as f32,
        ];
        self.pans = [pan_gains(p.osc[0].pan), pan_gains(p.osc[1].pan)];
        self.amp_env.retune(&p.amp, rate);
        self.filter_env.retune(&p.fenv, rate);
    }

    pub(super) fn release(&mut self) {
        if matches!(self.stage, Stage::Attack | Stage::Decay | Stage::Sustain) {
            self.stage = Stage::Release;
            self.amp_env.release();
            self.filter_env.release();
        }
        self.down = false;
        self.pedal = false;
    }

    pub(super) fn render(
        &mut self,
        span: &Span<'_>,
        at: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) {
        if self.stage == Stage::Off {
            return;
        }
        self.run(span, at, left, right);
    }

    #[inline(always)]
    fn run(&mut self, span: &Span<'_>, at: usize, left: &mut [f32], right: &mut [f32]) {
        let inv = 1.0 / span.rate;
        let rate = span.rate;
        let steal_step = 1.0 / (STEAL_SECONDS * rate);
        let fade_step = 1.0 / (GLIDE_SECONDS * rate);
        let full = left.len();
        for i in 0..full {
            let frame_at = at + i;
            if (span.curves.start + frame_at as u64).is_multiple_of(span.derive)
                && let Some((sound, lfo_hz)) = span.slots.at(frame_at)
            {
                self.sound = *sound;
                self.lfo_hz = lfo_hz;
                self.refresh(rate, span.changing.then_some(&span.sound));
            }
            match self.stage {
                Stage::Off => return,
                Stage::Attack | Stage::Decay | Stage::Sustain | Stage::Release => {
                    self.amp_env.step(&self.sound.amp);
                    self.stage = self.amp_env.stage;
                    if self.stage == Stage::Off {
                        return;
                    }
                }
                Stage::Steal => {
                    self.steal -= steal_step;
                    if self.steal <= 0.0 {
                        match self.pending.take() {
                            Some(waiting) => {
                                let age = self.age;
                                let sound = span.sound;
                                self.start(
                                    waiting.channel,
                                    waiting.key,
                                    waiting.velocity,
                                    &sound,
                                    rate,
                                    age,
                                    waiting.glide_oct,
                                    waiting.glide,
                                );
                            }
                            None => {
                                self.stage = Stage::Off;
                                return;
                            }
                        }
                    }
                }
            }
            let level = self.amp_env.value;
            let filter_env = self.filter_env.step(&self.sound.fenv);
            let (lfo, depth) = if self.sound.lfo == LFO_OFF {
                (0.0, 0.0)
            } else {
                self.lfo_phase += self.lfo_hz * inv;
                if self.lfo_phase >= 1.0 {
                    self.lfo_phase -= self.lfo_phase.floor();
                }
                let (sin, _) = sin_cos(f64::from(self.lfo_phase) * TAU);
                let depth = params::percent(span.curves.at(params::LFO_DEPTH, frame_at)) / 100.0;
                (sin as f32, depth)
            };
            let mut bend = span.bend;
            if self.sound.lfo == LFO_PITCH && depth > 0.0 {
                bend *= exp2(f64::from(depth * lfo)) as f32;
            }
            let mut glide_mul = 1.0;
            if self.glide > 0 {
                let done = 1.0 - self.glide as f32 / self.glide_frames as f32;
                glide_mul = exp2(f64::from(self.glide_oct) * f64::from(1.0 - done)) as f32;
                self.glide -= 1;
            }
            let base = params::cutoff_hz(span.curves.at(params::FILTER_CUTOFF, frame_at));
            let mut octaves = params::signed_octaves(span.curves.at(params::FILTER_ENV, frame_at))
                * filter_env
                + params::key_follow(span.curves.at(params::FILTER_KEY, frame_at))
                    * (f32::from(self.key) - 60.0)
                    / 12.0
                + params::vel_octaves(span.curves.at(params::FILTER_VEL, frame_at))
                    * (self.vel - 0.5)
                    * 2.0
                + span.open;
            if self.sound.lfo == LFO_CUTOFF && depth > 0.0 {
                octaves += depth * LFO_OCTAVES * lfo;
            }
            let (base_bits, octave_bits) = (base.to_bits(), octaves.to_bits());
            let cutoff = if self.corner.0 == base_bits && self.corner.1 == octave_bits {
                self.corner.2
            } else {
                let cutoff = (base * exp2(f64::from(octaves)) as f32).clamp(20.0, rate * 0.45);
                self.corner = (base_bits, octave_bits, cutoff);
                cutoff
            };
            let q = params::resonance(span.curves.at(params::FILTER_RESO, frame_at));
            let k = 1.0 / q.max(0.3);
            let (first, second) = match self.coefs {
                Some((cutoff_bits, k_bits, first, second))
                    if cutoff_bits == cutoff.to_bits() && k_bits == k.to_bits() =>
                {
                    (first, second)
                }
                _ => {
                    let g = tan(f64::from(std::f32::consts::PI * cutoff * inv)) as f32;
                    let first = Coefficients::new(g, k);
                    let second = Coefficients::new(g, 1.0 / CASCADE_Q);
                    self.coefs = Some((cutoff.to_bits(), k.to_bits(), first, second));
                    (first, second)
                }
            };
            let mut sides = [0.0f32; 2];
            for n in 0..2 {
                let voiced = params::percent(span.curves.at(
                    if n == 0 {
                        params::OSC1_LEVEL
                    } else {
                        params::OSC2_LEVEL
                    },
                    frame_at,
                )) / 100.0;
                let dt = self.freq[n] * bend * glide_mul * inv;
                let t = self.phase[n];
                let mix = self.shape_mix[n];
                let sample = if mix >= 1.0 {
                    wave_at(self.shapes[n], t, dt)
                } else {
                    self.shape_mix[n] = (mix + fade_step).min(1.0);
                    wave_at(self.shape_old[n], t, dt) * (1.0 - mix)
                        + wave_at(self.shapes[n], t, dt) * mix
                } * voiced;
                let next = t + dt;
                self.phase[n] = next - next.floor();
                sides[0] += sample * self.pans[n][0];
                sides[1] += sample * self.pans[n][1];
            }
            let noise = params::percent(span.curves.at(params::NOISE, frame_at)) / 100.0;
            if noise > 0.0 {
                let white = self.noise() * noise;
                sides[0] += white;
                sides[1] += white;
            }
            let filter = self.sound.filter;
            let mix = self.filter_mix;
            let fading = mix < 1.0 && self.filter_old != filter;
            if fading {
                self.filter_mix = (mix + fade_step).min(1.0);
            }
            for (side, x) in sides.iter_mut().enumerate() {
                let input = *x;
                let out = shaped(
                    filter,
                    first,
                    second,
                    &mut self.ic[side],
                    &mut self.ic2[side],
                    input,
                );
                *x = if fading {
                    let before = shaped(
                        self.filter_old,
                        first,
                        second,
                        &mut self.ic_old[side],
                        &mut self.ic2_old[side],
                        input,
                    );
                    before * (1.0 - mix) + out * mix
                } else {
                    out
                };
            }
            let velocity = params::percent(span.curves.at(params::VELOCITY, frame_at)) / 100.0;
            let vel_scale = 1.0 - velocity + velocity * self.vel;
            let out_level = params::percent(span.curves.at(params::LEVEL, frame_at)) / 100.0;
            let tremolo = if self.sound.lfo == LFO_AMP && depth > 0.0 {
                1.0 - depth * 0.5 * (1.0 - lfo)
            } else {
                1.0
            };
            let fade = if self.stage == Stage::Steal {
                self.steal
            } else {
                1.0
            };
            let gain = level * vel_scale * out_level * tremolo * fade;
            left[i] += sides[0] * gain;
            right[i] += sides[1] * gain;
        }
    }

    #[inline(always)]
    fn noise(&mut self) -> f32 {
        let mut x = self.noise_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.noise_state = x;
        (x >> 8) as f32 / 8_388_607.5 - 1.0
    }
}

#[inline(always)]
fn shaped(
    kind: u8,
    first: Coefficients,
    second: Coefficients,
    ic: &mut [f32; 2],
    ic2: &mut [f32; 2],
    x: f32,
) -> f32 {
    let (band, low) = svf(first, ic, x);
    match kind {
        LOW_24 => svf(second, ic2, low).1,
        HIGH_12 => x - first.k * band - low,
        BAND_12 => band,
        _ => low,
    }
}

#[inline(always)]
fn svf(c: Coefficients, ic: &mut [f32; 2], x: f32) -> (f32, f32) {
    let v3 = x - ic[1];
    let v1 = c.a1 * ic[0] + c.a2 * v3;
    let v2 = ic[1] + c.a2 * ic[0] + c.a3 * v3;
    ic[0] = 2.0 * v1 - ic[0];
    ic[1] = 2.0 * v2 - ic[1];
    (v1, v2)
}

#[inline]
pub(super) fn wave_at(wave: u8, t: f32, dt: f32) -> f32 {
    match wave {
        SQUARE => {
            let u = if t < 0.5 { t + 0.5 } else { t - 0.5 };
            (if t < 0.5 { 1.0 } else { -1.0 }) + blep(t, dt) - blep(u, dt)
        }
        TRIANGLE => 4.0 * (t - 0.5).abs() - 1.0,
        SINE => {
            let (sin, _) = sin_cos(f64::from(t) * TAU);
            sin as f32
        }
        _ => 2.0 * t - 1.0 - blep(t, dt),
    }
}

#[inline]
pub(super) fn blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}
