//! One voice of Flare: its oscillators, its noise, its filter and its two
//! envelopes, at the sample the events fall on. Nothing allocates.
//!
//! A voice keeps two kinds of setting. What shapes it — the waveforms, the
//! octaves and detune, the pans, the envelopes' times, the LFO's target —
//! is worked out from the per-frame curves every [`DERIVE_FRAMES`] frames, on
//! a grid counted from the start of the stream, so a frame is always computed
//! the same way whatever block it falls in. What modulates it — the cutoff,
//! the resonance, the filter envelope's amount, the key and velocity follow,
//! the levels, the noise and the LFO's depth — is read on **every** frame, so
//! a lane on the cutoff plays on its own samples (M8 slice B4).

use std::f64::consts::{PI, TAU};

use super::det::{exp, exp2, sin_cos, tan};
use super::params::{self, COUNT};
use super::*;

/// Frames between two derivations of a voice's settings: a sixtieth of a
/// millisecond at 48 kHz, so a lane on a pan or a waveform still reads as a
/// smooth change, and the same frame is always computed the same way.
pub(super) const DERIVE_FRAMES: u64 = 32;

/// Frames between two derivations while a value glides: a tenth of a
/// millisecond at 48 kHz, so a change reaches the sound as a slope.
pub(super) const GLIDE_FRAMES: u64 = 8;

/// The waveforms, in [`params::SHAPES`]' order.
pub(super) const SINE: u8 = 0;
pub(super) const TRIANGLE: u8 = 1;
pub(super) const SAW: u8 = 2;
pub(super) const SQUARE: u8 = 3;

/// The filter shapes, in [`params::FILTERS`]' order.
pub(super) const LOW_12: u8 = 0;
pub(super) const LOW_24: u8 = 1;
pub(super) const HIGH_12: u8 = 2;
pub(super) const BAND_12: u8 = 3;

/// What the LFO moves, in [`params::TARGETS`]' order.
pub(super) const LFO_OFF: u8 = 0;
pub(super) const LFO_PITCH: u8 = 1;
pub(super) const LFO_CUTOFF: u8 = 2;
pub(super) const LFO_AMP: u8 = 3;

/// How far the LFO's full depth moves the cutoff, in octaves.
const LFO_OCTAVES: f32 = 4.0;
/// The Q of a 24 dB filter's second stage: the resonance is the first
/// stage's, as a cascade of two resonators would double it.
const CASCADE_Q: f32 = 0.707;

#[derive(Debug, Clone, Copy)]
pub(super) struct Osc {
    pub(super) wave: u8,
    /// Octave and detune together, in cents.
    pub(super) cents: f32,
    /// −1 to 1.
    pub(super) pan: f32,
}

/// One envelope's times and sustain, in seconds and 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Env {
    pub(super) attack: f32,
    pub(super) decay: f32,
    pub(super) sustain: f32,
    pub(super) release: f32,
}

/// A sound: what a voice works out on its [`DERIVE_FRAMES`] grid. What
/// modulates the note — the levels, the cutoff, the resonance, velocity, the
/// LFO's depth — is read per frame instead, from the same curves.
#[derive(Debug, Clone, Copy)]
pub(super) struct Settings {
    pub(super) osc: [Osc; 2],
    pub(super) filter: u8,
    pub(super) amp: Env,
    pub(super) fenv: Env,
    /// What the LFO moves, in [`params::TARGETS`]' order.
    pub(super) lfo: u8,
    /// How many notes sound at once (1 to 16): the synth's, because it
    /// starts notes.
    pub(super) voices: usize,
    /// Portamento, in seconds.
    pub(super) glide: f32,
}

impl Settings {
    /// Silent, and nothing of it matters: what a slot holds before it is filled.
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

    /// The settings of `values` (normalised, by id).
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

    /// The settings the per-frame `curves` hold at `at` frames into the block.
    pub(super) fn at(curves: &Curves<'_>, at: usize) -> Self {
        let mut values = [0.0; COUNT];
        for (slot, info) in values.iter_mut().zip(params::TABLE) {
            *slot = curves.at(info.id, at);
        }
        Self::from_values(&values)
    }
}

/// How far a note has got with its envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
    /// Fading out to make way for `pending`.
    Steal,
}

/// One envelope's level, the stage it is in, and the coefficients that move
/// it. The amp envelope and the filter envelope each run their own stages:
/// a filter envelope with a short decay settles while the amplitude is still
/// decaying (the note is not released by the filter's own fall).
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

    /// The coefficients for `env` at `rate`.
    fn retune(&mut self, env: &Env, rate: f32) {
        self.attack_step = 1.0 / (env.attack.max(0.001) * rate);
        self.decay_coef = coef(env.decay, rate);
        self.release_coef = coef(env.release, rate);
    }

    /// Starts again from nothing.
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

    /// One frame of the envelope, returning its level.
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
            // A note held while its sound changes moves to the new sustain
            // level at the decay's own pace, so a preset change or a lane
            // never steps the level (the old path set it straight from the
            // gliding value; this one arrives over the decay).
            Stage::Sustain => {
                self.value += (env.sustain - self.value) * self.decay_coef;
            }
            Stage::Off | Stage::Steal => {}
        }
        self.value
    }
}

/// How many settings a block can hold: one for each [`GLIDE_FRAMES`] it
/// covers, plus both ends (a still block fills every fourth).
pub(super) const SLOT_COUNT: usize = MOST_FRAMES / GLIDE_FRAMES as usize + 1;

/// The settings a block's derivation slots hold, and the LFO's rate at each:
/// worked out once for every voice rather than once per voice (review M5).
pub(super) struct Slots {
    /// The block offset of the first slot: the first frame whose absolute
    /// count is a multiple of the step.
    first: usize,
    /// Frames between two slots: [`DERIVE_FRAMES`], or [`GLIDE_FRAMES`] while
    /// a value is arriving.
    step: usize,
    len: usize,
    sounds: [Settings; SLOT_COUNT],
    lfo: [f32; SLOT_COUNT],
}

impl Slots {
    /// An empty list: a block whose slots have not been filled.
    pub(super) const EMPTY: Slots = Slots {
        first: 0,
        step: DERIVE_FRAMES as usize,
        len: 0,
        sounds: [Settings::IDLE; SLOT_COUNT],
        lfo: [0.0; SLOT_COUNT],
    };

    /// Works out every slot of the piece that begins at absolute frame `start`
    /// and covers `frames`, from the piece's curves.
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

    /// The settings of the frame `at` frames into the piece, and the LFO's
    /// rate there, when it is a derivation slot: every [`DERIVE_FRAMES`] or
    /// [`GLIDE_FRAMES`] frames is one, and both are steps of this list.
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

/// The LFO's rate at a slot: its own in Hz when it is free, or its division at
/// the tempo when it follows it.
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

/// The per-frame normalised value of every parameter for one block: what the
/// lanes play and what the project holds, straight between points and exact
/// on a point's own frame.
pub(super) struct Curves<'a> {
    data: &'a [[f32; MOST_FRAMES]; COUNT],
    /// The block's first frame, counted from the start of the stream.
    pub(super) start: u64,
    /// The frames the block holds.
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

    /// The value of parameter `id` at `at` frames into the block.
    #[inline(always)]
    pub(super) fn at(&self, id: u32, at: usize) -> f32 {
        let at = at.min(self.frames.saturating_sub(1));
        self.data[id as usize - 1][at]
    }
}

#[derive(Clone, Copy)]
pub(super) struct Voice {
    pub(super) stage: Stage,
    /// The MIDI channel and key the note came on: a note is both.
    pub(super) channel: u8,
    pub(super) key: u8,
    /// The key is down.
    pub(super) down: bool,
    /// The key came up while the pedal was down: released with the pedal.
    pub(super) pedal: bool,
    pub(super) age: u64,
    /// The derived settings now.
    sound: Settings,
    /// Velocity, 0 to 1, and the note's pitch in Hz before detune.
    vel: f32,
    hz: f32,
    /// The octaves this note glides from (portamento), and the frames of it
    /// left and in all.
    glide_oct: f32,
    glide: u32,
    glide_frames: u32,
    /// Each oscillator's pitch in Hz, with its octave and detune.
    freq: [f32; 2],
    phase: [f32; 2],
    /// The LFO's phase, 0 to 1.
    lfo_phase: f32,
    /// The LFO's rate in Hz: its own, or its division at the tempo.
    lfo_hz: f32,
    /// Where the noise generator stands.
    noise_state: u32,
    amp_env: EnvState,
    filter_env: EnvState,
    /// The filter's two stages, `[ic1eq, ic2eq]` for each side.
    ic: [[f32; 2]; 2],
    ic2: [[f32; 2]; 2],
    /// The coefficients the filter holds, and what they were worked out for.
    coefs: Option<(u32, u32, Coefficients, Coefficients)>,
    /// The filter's shape, the one it is cross-fading from (its own state,
    /// independent of the new one's), and how far that has got.
    filter_old: u8,
    filter_mix: f32,
    ic_old: [[f32; 2]; 2],
    ic2_old: [[f32; 2]; 2],
    /// The cutoff the corner and its modulation last worked out to, and the
    /// two they came from: a still sound pays for the exponential once.
    corner: (u32, u32, f32),
    pub(super) steal: f32,
    /// The note waiting for this voice to fade out.
    pub(super) pending: Option<super::Waiting>,
    /// Each oscillator's left and right gain (equal-power pan), worked out
    /// when the settings are derived.
    pans: [[f32; 2]; 2],
    /// The waveform each oscillator plays now, and the one it is cross-fading
    /// from with how far that has got (1: none). A change of shape is a slope,
    /// never a step.
    shapes: [u8; 2],
    shape_old: [u8; 2],
    shape_mix: [f32; 2],
}

/// One state-variable stage's coefficients.
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

/// What one span of rendering shares between voices.
pub(super) struct Span<'a> {
    pub(super) rate: f32,
    /// Pitch bend as a frequency ratio.
    pub(super) bend: f32,
    /// Filter opening from the wheel or pressure, in octaves.
    pub(super) open: f32,
    /// The sound new notes get (a stolen voice's replacement).
    pub(super) sound: Settings,
    /// Frames between two derivations of the settings.
    pub(super) derive: u64,
    /// A value glide is running: a sound the project changed is arriving, and
    /// its waveforms are cross-faded from the moment it began (not from the
    /// moment the choice snaps over).
    pub(super) changing: bool,
    /// The per-frame values of every parameter.
    pub(super) curves: &'a Curves<'a>,
    /// The settings and the LFO's rate at each derivation slot of the piece.
    pub(super) slots: &'a Slots,
}

/// Left and right gains for a pan from -1 to 1, at constant power.
pub(super) fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (f64::from(pan.clamp(-1.0, 1.0)) + 1.0) * 0.25 * PI;
    let (sin, cos) = sin_cos(angle);
    [cos as f32, sin as f32]
}

/// How fast an exponential envelope stage moves, for a time constant of
/// `4.6`: the level falls to −80 dB in `seconds`.
pub(super) fn coef(seconds: f32, rate: f32) -> f32 {
    (1.0 - exp(-4.6 / (f64::from(seconds.max(1e-4)) * f64::from(rate)))) as f32
}

impl Voice {
    /// Starts a note: `glide_oct` is how many octaves above it the last note
    /// was (portamento), and `glide` how long it takes to arrive.
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

    /// Works the values derived from the settings out again (pitch, pans and
    /// the envelopes' coefficients).
    pub(super) fn refresh(&mut self, rate: f32, arriving: Option<&Settings>) {
        // A copy: the filter's shape is written below, and the rest is read.
        let p = self.sound;
        for n in 0..2 {
            // While a change arrives, its waveform takes over at once and the
            // one playing fades out under it, as the old path cross-faded: the
            // shape does not wait for the choice to snap over halfway.
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
        // A changed filter shape cross-fades too, from a filter of its own so
        // its state is its own: the response slides from the old shape to the
        // new one over the glide, and no sample jumps (review M3).
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

    /// Adds the frames of this voice from `at` frames into the block.
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
        // A changed waveform cross-fades over the time a value glides.
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
            // The amp envelope decides where the note is.
            match self.stage {
                Stage::Off => return,
                // Both envelopes run their own stages; the note follows the
                // amp envelope, which is what "sounding" means.
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
            // The LFO: a sine from the note's own start.
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
            // The pitch: the note, its glide, the bend, and the LFO.
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
            // The cutoff: the setting, the filter envelope, the key, velocity
            // and the wheel — read every frame, so a lane on it plays on its
            // own samples.
            let base = params::cutoff_hz(span.curves.at(params::FILTER_CUTOFF, frame_at));
            let mut octaves = params::signed_octaves(span.curves.at(params::FILTER_ENV, frame_at))
                * filter_env
                + params::key_follow(span.curves.at(params::FILTER_KEY, frame_at))
                    * (f32::from(self.key) - 60.0)
                    / 12.0
                + params::vel_octaves(span.curves.at(params::FILTER_VEL, frame_at))
                    * (self.vel - 0.5)
                    // A full-velocity note gains `vel_oct` octaves over a
                    // half-velocity one.
                    * 2.0
                + span.open;
            if self.sound.lfo == LFO_CUTOFF && depth > 0.0 {
                octaves += depth * LFO_OCTAVES * lfo;
            }
            // The corner, only when the setting or its modulation moved: a
            // held note with nothing on its filter costs a comparison.
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
            // The oscillators and the noise.
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
            // The filter, on each side: the new shape's, and while a changed
            // shape slides in, the old one's from a filter of its own state
            // (review M3).
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
            // The level: velocity, the output level, the tremolo and a steal.
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

    /// The next white sample, from a 32-bit xorshift: the same sequence on
    /// every platform.
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

/// One frame of a shape's output through a filter stage.
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

/// One state-variable stage's outputs: the band-pass and the low-pass.
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

/// The polyBLEP correction for a step at phase 0 of an oscillator whose
/// phase advances `dt` per sample.
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
