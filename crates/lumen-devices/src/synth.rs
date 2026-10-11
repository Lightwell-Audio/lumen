//! The built-in polyphonic synth (M5 slice 1, ADR-0045): two band-limited
//! oscillators per voice, a 12 dB low-pass filter, an ADSR envelope, 16
//! voices with the oldest stolen. Five presets; the first, **Poly**, is the
//! default sound (the owner's S1: two saws detuned ±7 cents, low-pass at
//! 2.4 kHz with a little velocity to cutoff, A 5 ms, D 300 ms, S 70 %,
//! R 250 ms).
//!
//! Events apply at their exact frame: a block is rendered in spans between
//! them. A note's sound is fixed when it starts, so changing the preset
//! (or a program change) leaves notes already sounding as they were.
//!
//! The synth listens on every channel. Pitch bend is ±2 semitones; CC 64 is
//! the sustain pedal; CC 1 and channel pressure open the filter a little
//! (up to an octave and a half); CC 120 silences, CC 123 releases, CC 121
//! resets the controllers; program changes 0 to 4 choose a preset. Bank
//! selects are ignored.

use std::f32::consts::PI;

use lumen_core::midi::{Event, Kind, Msg, cc};

use crate::Instrument;

/// The synth's instrument number in a project (see [`Instrument`]).
pub const KIND: u32 = 1;
const VOICES: usize = 16;
/// How far the pitch bend wheel goes, in semitones.
const BEND_RANGE: f32 = 2.0;
/// How far the modulation wheel opens the filter, in octaves.
const MOD_OCTAVES: f32 = 1.5;
/// A stolen voice fades out over this long.
const STEAL_SECONDS: f32 = 0.002;
/// Below this an envelope has ended (-80 dB).
const SILENT: f32 = 1e-4;

/// The five sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    #[default]
    Poly,
    Keys,
    Bass,
    Pluck,
    Pad,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Poly,
        Preset::Keys,
        Preset::Bass,
        Preset::Pluck,
        Preset::Pad,
    ];

    /// The name saved in projects: never changes.
    pub fn id(self) -> &'static str {
        match self {
            Preset::Poly => "poly",
            Preset::Keys => "keys",
            Preset::Bass => "bass",
            Preset::Pluck => "pluck",
            Preset::Pad => "pad",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// What menus show.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Poly => "Poly",
            Preset::Keys => "Keys",
            Preset::Bass => "Bass",
            Preset::Pluck => "Pluck",
            Preset::Pad => "Pad",
        }
    }

    /// The number [`Instrument::configure`] takes.
    pub fn number(self) -> u32 {
        self as u32
    }

    pub fn from_number(number: u32) -> Option<Self> {
        Self::ALL.get(number as usize).copied()
    }

    fn params(self) -> Params {
        let saw = |level, cents, pan| Osc {
            wave: Wave::Saw,
            level,
            cents,
            pan,
        };
        match self {
            Preset::Poly => Params {
                osc: [saw(1.0, -7.0, -0.35), saw(1.0, 7.0, 0.35)],
                cutoff: 2_400.0,
                resonance: 0.8,
                vel_cutoff: 0.6,
                attack: 0.005,
                decay: 0.3,
                sustain: 0.7,
                release: 0.25,
                gain: 0.16,
            },
            Preset::Keys => Params {
                osc: [
                    Osc {
                        wave: Wave::Triangle,
                        level: 1.0,
                        cents: -4.0,
                        pan: -0.2,
                    },
                    Osc {
                        wave: Wave::Triangle,
                        level: 0.6,
                        cents: 1_200.0 + 3.0,
                        pan: 0.2,
                    },
                ],
                cutoff: 5_000.0,
                resonance: 0.7,
                vel_cutoff: 1.0,
                attack: 0.002,
                decay: 0.7,
                sustain: 0.35,
                release: 0.2,
                gain: 0.24,
            },
            Preset::Bass => Params {
                osc: [
                    Osc {
                        wave: Wave::Saw,
                        level: 1.0,
                        cents: 0.0,
                        pan: 0.0,
                    },
                    Osc {
                        wave: Wave::Square,
                        level: 0.7,
                        cents: -1_200.0,
                        pan: 0.0,
                    },
                ],
                cutoff: 700.0,
                resonance: 1.6,
                vel_cutoff: 1.2,
                attack: 0.002,
                decay: 0.2,
                sustain: 0.8,
                release: 0.1,
                gain: 0.22,
            },
            Preset::Pluck => Params {
                osc: [saw(1.0, -5.0, -0.3), saw(0.8, 5.0, 0.3)],
                cutoff: 3_500.0,
                resonance: 1.0,
                vel_cutoff: 1.0,
                attack: 0.001,
                decay: 0.25,
                sustain: 0.0,
                release: 0.15,
                gain: 0.2,
            },
            Preset::Pad => Params {
                osc: [saw(1.0, -12.0, -0.5), saw(1.0, 12.0, 0.5)],
                cutoff: 1_600.0,
                resonance: 0.7,
                vel_cutoff: 0.3,
                attack: 0.4,
                decay: 0.5,
                sustain: 0.8,
                release: 0.9,
                gain: 0.14,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Wave {
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
struct Osc {
    wave: Wave,
    level: f32,
    cents: f32,
    /// −1 to 1.
    pan: f32,
}

/// A sound, fixed when a note starts.
#[derive(Debug, Clone, Copy)]
struct Params {
    osc: [Osc; 2],
    /// Hz.
    cutoff: f32,
    /// Filter Q.
    resonance: f32,
    /// Octaves of cutoff a full-velocity note gains over a half-velocity one.
    vel_cutoff: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    gain: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
    /// Fading out to make way for `pending`.
    Steal,
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    key: u8,
    /// The key is down.
    down: bool,
    /// The key came up while the pedal was down: released with the pedal.
    pedal: bool,
    age: u64,
    params: Params,
    amp: f32,
    /// Octaves of cutoff from velocity.
    cutoff_shift: f32,
    freq: [f32; 2],
    phase: [f32; 2],
    env: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    steal: f32,
    pending: Option<(u8, u8)>,
    /// The filter's state: `[ic1eq, ic2eq]` for the left side, then the right.
    ic: [[f32; 2]; 2],
    /// Each oscillator's left and right gain (equal-power pan), worked out
    /// once when the note starts.
    pans: [[f32; 2]; 2],
}

const IDLE: Voice = Voice {
    stage: Stage::Off,
    key: 0,
    down: false,
    pedal: false,
    age: 0,
    params: Params {
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
    },
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
struct Span {
    rate: f32,
    /// Pitch bend as a frequency ratio.
    bend: f32,
    /// Filter opening from the wheel or pressure, in octaves.
    open: f32,
    /// The sound new notes get (a stolen voice's replacement).
    sound: Params,
}

/// Left and right gains for a pan from -1 to 1, at constant power.
fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (pan + 1.0) * 0.25 * PI;
    [angle.cos(), angle.sin()]
}

fn coef(seconds: f32, rate: f32) -> f32 {
    1.0 - (-4.6 / (seconds.max(1e-4) * rate)).exp()
}

impl Voice {
    fn start(&mut self, key: u8, velocity: u8, params: &Params, rate: f32, age: u64) {
        let v = f32::from(velocity) / 127.0;
        let hz = 440.0 * ((f32::from(key) - 69.0) / 12.0).exp2();
        *self = Voice {
            stage: Stage::Attack,
            key,
            down: true,
            pedal: false,
            age,
            params: *params,
            amp: v * params.gain,
            cutoff_shift: params.vel_cutoff * (v - 0.5) * 2.0,
            freq: [
                hz * (params.osc[0].cents / 1_200.0).exp2(),
                hz * (params.osc[1].cents / 1_200.0).exp2(),
            ],
            phase: [0.0, 0.25],
            env: 0.0,
            attack_step: 1.0 / (params.attack.max(0.001) * rate),
            decay_coef: coef(params.decay, rate),
            release_coef: coef(params.release, rate),
            steal: 1.0,
            pending: None,
            ic: [[0.0; 2]; 2],
            pans: [pan_gains(params.osc[0].pan), pan_gains(params.osc[1].pan)],
        };
    }

    fn release(&mut self) {
        if matches!(self.stage, Stage::Attack | Stage::Decay | Stage::Sustain) {
            self.stage = Stage::Release;
        }
        self.down = false;
        self.pedal = false;
    }

    /// Adds `left.len()` frames of this voice.
    fn render(&mut self, span: &Span, left: &mut [f32], right: &mut [f32]) {
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
                            Some((key, velocity)) => {
                                let age = self.age;
                                self.start(key, velocity, &span.sound, span.rate, age);
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
                let sample = match osc.wave {
                    Wave::Saw => 2.0 * t - 1.0 - blep(t, dt),
                    Wave::Square => {
                        let u = if t < 0.5 { t + 0.5 } else { t - 0.5 };
                        (if t < 0.5 { 1.0 } else { -1.0 }) + blep(t, dt) - blep(u, dt)
                    }
                    Wave::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
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

/// The polyBLEP correction for a step at phase 0 of an oscillator whose
/// phase advances `dt` per sample.
#[inline]
fn blep(t: f32, dt: f32) -> f32 {
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

/// The synth.
pub struct Synth {
    voices: [Voice; VOICES],
    sound: Params,
    preset: Preset,
    rate: f32,
    /// The pedal is down.
    sustain: bool,
    /// Pitch bend as a frequency ratio.
    bend: f32,
    /// The wheel and pressure, 0 to 1.
    wheel: f32,
    pressure: f32,
    clock: u64,
}

impl Synth {
    /// Allocates; call off the audio thread.
    pub fn new(preset: Preset) -> Self {
        Self {
            voices: [IDLE; VOICES],
            sound: preset.params(),
            preset,
            rate: 48_000.0,
            sustain: false,
            bend: 1.0,
            wheel: 0.0,
            pressure: 0.0,
            clock: 0,
        }
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// Voices sounding now (tests).
    pub fn voices(&self) -> usize {
        self.voices.iter().filter(|v| v.stage != Stage::Off).count()
    }

    fn apply(&mut self, msg: Msg) {
        match msg.kind() {
            Kind::NoteOn { key, velocity } => self.note_on(key, velocity),
            Kind::NoteOff { key, .. } => self.note_off(key),
            Kind::Controller { number, value } => match number {
                cc::SUSTAIN => self.pedal(value >= 64),
                cc::MODULATION => self.wheel = f32::from(value) / 127.0,
                cc::ALL_SOUND_OFF => self.silence(),
                cc::ALL_NOTES_OFF => {
                    for v in &mut self.voices {
                        v.release();
                    }
                }
                121 => {
                    self.wheel = 0.0;
                    self.pressure = 0.0;
                    self.bend = 1.0;
                    self.pedal(false);
                }
                _ => {}
            },
            Kind::PitchBend { value } => {
                self.bend = (f32::from(value) / 8192.0 * BEND_RANGE / 12.0).exp2();
            }
            Kind::Pressure { value } => self.pressure = f32::from(value) / 127.0,
            Kind::Program { program } => {
                if let Some(preset) = Preset::from_number(u32::from(program)) {
                    self.configure(preset.number());
                }
            }
            Kind::KeyPressure { .. } | Kind::Other => {}
        }
    }

    fn pedal(&mut self, down: bool) {
        self.sustain = down;
        if !down {
            for v in &mut self.voices {
                if v.pedal {
                    v.release();
                }
            }
        }
    }

    fn note_on(&mut self, key: u8, velocity: u8) {
        self.clock += 1;
        // The same key again lets the first go.
        for v in &mut self.voices {
            if v.key == key && v.stage != Stage::Off && (v.down || v.pedal) {
                v.release();
            }
        }
        if let Some(v) = self.voices.iter_mut().find(|v| v.stage == Stage::Off) {
            v.start(key, velocity, &self.sound, self.rate, self.clock);
            return;
        }
        // Full: the oldest voice fades out, then the new note takes it.
        if let Some(v) = self
            .voices
            .iter_mut()
            .filter(|v| v.stage != Stage::Steal)
            .min_by_key(|v| v.age)
        {
            v.stage = Stage::Steal;
            v.steal = 1.0;
            v.pending = Some((key, velocity));
            v.age = self.clock;
            v.down = false;
            v.pedal = false;
        }
    }

    fn note_off(&mut self, key: u8) {
        for v in &mut self.voices {
            if v.stage == Stage::Steal && v.pending.is_some_and(|(k, _)| k == key) {
                v.pending = None;
            } else if v.key == key && v.down && v.stage != Stage::Off {
                if self.sustain {
                    v.down = false;
                    v.pedal = true;
                } else {
                    v.release();
                }
            }
        }
    }
}

impl Instrument for Synth {
    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
        if rate != 0 && rate as f32 != self.rate {
            // The device changed its rate: voices begin again.
            self.silence();
            self.rate = rate as f32;
        }
        let frames = left.len().min(right.len());
        let mut done = 0;
        let mut next = 0;
        loop {
            while next < events.len() && events[next].offset as usize <= done {
                self.apply(events[next].msg);
                next += 1;
            }
            if done >= frames {
                break;
            }
            let end = events
                .get(next)
                .map_or(frames, |e| (e.offset as usize).min(frames));
            let span = Span {
                rate: self.rate,
                bend: self.bend,
                open: self.wheel.max(self.pressure) * MOD_OCTAVES,
                sound: self.sound,
            };
            for voice in &mut self.voices {
                if voice.stage != Stage::Off {
                    voice.render(&span, &mut left[done..end], &mut right[done..end]);
                }
            }
            done = end;
        }
        // Events past the end of the slices (a release with no frames).
        for e in &events[next..] {
            self.apply(e.msg);
        }
    }

    fn preview(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        if rate as f32 != self.rate {
            return;
        }
        // Voices are plain data: run a copy, keep the real ones.
        let kept = self.voices;
        self.render(rate, &[], left, right);
        self.voices = kept;
    }

    fn active(&self) -> bool {
        self.voices.iter().any(|v| v.stage != Stage::Off)
    }

    fn configure(&mut self, preset: u32) {
        if let Some(preset) = Preset::from_number(preset) {
            self.preset = preset;
            self.sound = preset.params();
        }
    }

    fn silence(&mut self) {
        self.voices = [IDLE; VOICES];
        self.sustain = false;
    }
}
