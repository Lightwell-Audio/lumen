//! One voice of the synth: oscillators, filter, envelope and the glide
//! between presets. Nothing allocates.

use std::f32::consts::PI;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Wave {
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Osc {
    pub(super) wave: Wave,
    pub(super) level: f32,
    pub(super) cents: f32,
    /// −1 to 1.
    pub(super) pan: f32,
}

/// A sound.
#[derive(Debug, Clone, Copy)]
pub(super) struct Params {
    pub(super) osc: [Osc; 2],
    /// Hz.
    pub(super) cutoff: f32,
    /// Filter Q.
    pub(super) resonance: f32,
    /// Octaves of cutoff a full-velocity note gains over a half-velocity one.
    pub(super) vel_cutoff: f32,
    pub(super) attack: f32,
    pub(super) decay: f32,
    pub(super) sustain: f32,
    pub(super) release: f32,
    pub(super) gain: f32,
}

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
    /// The sound now: while gliding, between where it began and `goal`.
    params: Params,
    /// Where a glide is heading.
    goal: Params,
    /// Frames of the glide left (0: not gliding).
    glide: u32,
    /// The waveforms before a change, cross-fading out as `wave_mix` goes
    /// from 0 to 1.
    wave_old: [Wave; 2],
    wave_mix: f32,
    /// Velocity, 0 to 1, and the note's pitch in Hz before detune.
    vel: f32,
    hz: f32,
    amp: f32,
    /// Octaves of cutoff from velocity.
    cutoff_shift: f32,
    freq: [f32; 2],
    phase: [f32; 2],
    env: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    pub(super) steal: f32,
    /// The note waiting for this voice to fade out: (channel, key, velocity).
    pub(super) pending: Option<(u8, u8, u8)>,
    /// The filter's state: `[ic1eq, ic2eq]` for the left side, then the right.
    ic: [[f32; 2]; 2],
    /// Each oscillator's left and right gain (equal-power pan), worked out
    /// once when the note starts.
    pans: [[f32; 2]; 2],
}

pub(super) const IDLE_PARAMS: Params = Params {
    osc: [Osc {
        wave: Wave::Saw,
        level: 0.0,
        cents: 0.0,
        pan: 0.0,
    }; 2],
    cutoff: 1_000.0,
    resonance: 0.7,
    vel_cutoff: 0.0,
    attack: 0.0,
    decay: 0.0,
    sustain: 0.0,
    release: 0.0,
    gain: 0.0,
};

pub(super) const IDLE: Voice = Voice {
    stage: Stage::Off,
    channel: 0,
    key: 0,
    down: false,
    pedal: false,
    age: 0,
    params: IDLE_PARAMS,
    goal: IDLE_PARAMS,
    glide: 0,
    wave_old: [Wave::Saw; 2],
    wave_mix: 1.0,
    vel: 0.0,
    hz: 0.0,
    amp: 0.0,
    cutoff_shift: 0.0,
    freq: [0.0; 2],
    phase: [0.0; 2],
    env: 0.0,
    attack_step: 1.0,
    decay_coef: 1.0,
    release_coef: 1.0,
    steal: 1.0,
    pending: None,
    ic: [[0.0; 2]; 2],
    pans: [[0.0; 2]; 2],
};

/// What one span of rendering shares between voices.
pub(super) struct Span {
    pub(super) rate: f32,
    /// Pitch bend as a frequency ratio.
    pub(super) bend: f32,
    /// Filter opening from the wheel or pressure, in octaves.
    pub(super) open: f32,
    /// The sound new notes get (a stolen voice's replacement).
    pub(super) sound: Params,
}

/// Left and right gains for a pan from -1 to 1, at constant power.
pub(super) fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (pan + 1.0) * 0.25 * PI;
    [angle.cos(), angle.sin()]
}

pub(super) fn coef(seconds: f32, rate: f32) -> f32 {
    1.0 - (-4.6 / (seconds.max(1e-4) * rate)).exp()
}

impl Voice {
    pub(super) fn start(
        &mut self,
        channel: u8,
        key: u8,
        velocity: u8,
        params: &Params,
        rate: f32,
        age: u64,
    ) {
        *self = Voice {
            stage: Stage::Attack,
            channel,
            key,
            down: true,
            pedal: false,
            age,
            params: *params,
            goal: *params,
            glide: 0,
            wave_old: [params.osc[0].wave, params.osc[1].wave],
            wave_mix: 1.0,
            vel: f32::from(velocity) / 127.0,
            hz: 440.0 * ((f32::from(key) - 69.0) / 12.0).exp2(),
            amp: 0.0,
            cutoff_shift: 0.0,
            freq: [0.0; 2],
            phase: [0.0, 0.25],
            env: 0.0,
            attack_step: 1.0,
            decay_coef: 1.0,
            release_coef: 1.0,
            steal: 1.0,
            pending: None,
            ic: [[0.0; 2]; 2],
            pans: [[0.0; 2]; 2],
        };
        self.refresh(rate);
    }

    /// Works the values derived from the sound out again.
    pub(super) fn refresh(&mut self, rate: f32) {
        let p = &self.params;
        self.amp = self.vel * p.gain;
        self.cutoff_shift = p.vel_cutoff * (self.vel - 0.5) * 2.0;
        self.freq = [
            self.hz * (p.osc[0].cents / 1_200.0).exp2(),
            self.hz * (p.osc[1].cents / 1_200.0).exp2(),
        ];
        self.attack_step = 1.0 / (p.attack.max(0.001) * rate);
        self.decay_coef = coef(p.decay, rate);
        self.release_coef = coef(p.release, rate);
        self.pans = [pan_gains(p.osc[0].pan), pan_gains(p.osc[1].pan)];
    }

    /// Begins gliding to another sound, from wherever the voice is now.
    pub(super) fn retarget(&mut self, sound: &Params, rate: f32) {
        if self.wave_mix >= 1.0 {
            self.wave_old = [self.params.osc[0].wave, self.params.osc[1].wave];
        }
        for n in 0..2 {
            if sound.osc[n].wave != self.params.osc[n].wave {
                self.params.osc[n].wave = sound.osc[n].wave;
                self.wave_mix = 0.0;
            }
        }
        self.goal = *sound;
        self.glide = ((GLIDE_SECONDS * rate) as u32).max(1);
    }

    /// Moves `frames` frames along the glide.
    pub(super) fn advance(&mut self, frames: u32, rate: f32) {
        let frac = if frames >= self.glide {
            1.0
        } else {
            frames as f32 / self.glide as f32
        };
        let mix = |a: &mut f32, b: f32| *a += (b - *a) * frac;
        let (p, g) = (&mut self.params, &self.goal);
        for n in 0..2 {
            mix(&mut p.osc[n].level, g.osc[n].level);
            mix(&mut p.osc[n].cents, g.osc[n].cents);
            mix(&mut p.osc[n].pan, g.osc[n].pan);
        }
        mix(&mut p.cutoff, g.cutoff);
        mix(&mut p.resonance, g.resonance);
        mix(&mut p.vel_cutoff, g.vel_cutoff);
        mix(&mut p.attack, g.attack);
        mix(&mut p.decay, g.decay);
        mix(&mut p.sustain, g.sustain);
        mix(&mut p.release, g.release);
        mix(&mut p.gain, g.gain);
        mix(&mut self.wave_mix, 1.0);
        self.glide = self.glide.saturating_sub(frames);
        if self.glide == 0 {
            self.params = self.goal;
            self.wave_mix = 1.0;
        }
        if self.stage == Stage::Sustain {
            // A held note moves to the new sustain level.
            self.env = self.params.sustain;
            if self.glide == 0 && self.env <= SILENT {
                self.stage = Stage::Off;
            }
        }
        self.refresh(rate);
    }

    pub(super) fn release(&mut self) {
        if matches!(self.stage, Stage::Attack | Stage::Decay | Stage::Sustain) {
            self.stage = Stage::Release;
        }
        self.down = false;
        self.pedal = false;
    }

    /// Adds `left.len()` frames of this voice.
    pub(super) fn render(&mut self, span: &Span, left: &mut [f32], right: &mut [f32]) {
        if self.glide == 0 {
            return self.run(span, left, right);
        }
        let mut at = 0;
        while at < left.len() && self.stage != Stage::Off {
            let end = (at + GLIDE_CHUNK).min(left.len());
            if self.glide > 0 {
                self.advance((end - at) as u32, span.rate);
            }
            if self.stage == Stage::Off {
                break;
            }
            self.run(span, &mut left[at..end], &mut right[at..end]);
            at = end;
        }
    }

    /// Adds the frames of this voice with its values as they are.
    pub(super) fn run(&mut self, span: &Span, left: &mut [f32], right: &mut [f32]) {
        // Two copies of the loop, so that a voice not gliding pays nothing.
        if self.wave_mix < 1.0 {
            self.run_with::<true>(span, left, right);
        } else {
            self.run_with::<false>(span, left, right);
        }
    }

    #[inline(always)]
    pub(super) fn run_with<const FADING: bool>(
        &mut self,
        span: &Span,
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let mix = self.wave_mix;
        let inv = 1.0 / span.rate;
        let incs = [
            self.freq[0] * span.bend * inv,
            self.freq[1] * span.bend * inv,
        ];
        let cutoff = (self.params.cutoff * (self.cutoff_shift + span.open).exp2())
            .clamp(20.0, span.rate * 0.45);
        let g = (PI * cutoff * inv).tan();
        let k = 1.0 / self.params.resonance.max(0.3);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let steal_step = 1.0 / (STEAL_SECONDS * span.rate);
        for i in 0..left.len() {
            match self.stage {
                Stage::Off => return,
                Stage::Attack => {
                    self.env += self.attack_step;
                    if self.env >= 1.0 {
                        self.env = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => {
                    let target = self.params.sustain;
                    self.env += (target - self.env) * self.decay_coef;
                    if (self.env - target).abs() < SILENT {
                        self.env = target;
                        self.stage = if target <= SILENT {
                            Stage::Off
                        } else {
                            Stage::Sustain
                        };
                    }
                }
                Stage::Sustain => {}
                Stage::Release => {
                    self.env -= self.env * self.release_coef;
                    if self.env < SILENT {
                        self.stage = Stage::Off;
                    }
                }
                Stage::Steal => {
                    self.steal -= steal_step;
                    if self.steal <= 0.0 {
                        match self.pending.take() {
                            Some((channel, key, velocity)) => {
                                let age = self.age;
                                self.start(channel, key, velocity, &span.sound, span.rate, age);
                            }
                            None => self.stage = Stage::Off,
                        }
                        if self.stage == Stage::Off {
                            return;
                        }
                        // The new note's first frame is this one.
                        self.env = 0.0;
                    }
                }
            }
            let mut sides = [0.0f32; 2];
            for (n, osc) in self.params.osc.iter().enumerate() {
                let dt = incs[n];
                let t = self.phase[n];
                let sample = if FADING {
                    wave_at(self.wave_old[n], t, dt) * (1.0 - mix) + wave_at(osc.wave, t, dt) * mix
                } else {
                    wave_at(osc.wave, t, dt)
                } * osc.level;
                let next = t + dt;
                self.phase[n] = next - next.floor();
                // Equal-power pan of each oscillator.
                sides[0] += sample * self.pans[n][0];
                sides[1] += sample * self.pans[n][1];
            }
            // A state variable filter (low-pass) on each side.
            for (x, ic) in sides.iter_mut().zip(self.ic.iter_mut()) {
                let v3 = *x - ic[1];
                let v1 = a1 * ic[0] + a2 * v3;
                let v2 = ic[1] + a2 * ic[0] + a3 * v3;
                ic[0] = 2.0 * v1 - ic[0];
                ic[1] = 2.0 * v2 - ic[1];
                *x = v2;
            }
            let gain = self.env
                * self.amp
                * if self.stage == Stage::Steal {
                    self.steal
                } else {
                    1.0
                };
            left[i] += sides[0] * gain;
            right[i] += sides[1] * gain;
        }
    }
}

#[inline]
pub(super) fn wave_at(wave: Wave, t: f32, dt: f32) -> f32 {
    match wave {
        Wave::Saw => 2.0 * t - 1.0 - blep(t, dt),
        Wave::Square => {
            let u = if t < 0.5 { t + 0.5 } else { t - 0.5 };
            (if t < 0.5 { 1.0 } else { -1.0 }) + blep(t, dt) - blep(u, dt)
        }
        Wave::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
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
