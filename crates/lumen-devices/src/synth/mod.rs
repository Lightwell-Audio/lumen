//! The built-in polyphonic synth (M5 slice 1, ADR-0045): two band-limited
//! oscillators per voice, a 12 dB low-pass filter, an ADSR envelope, 16
//! voices with the oldest stolen. Five presets; the first, **Poly**, is the
//! default sound (the owner's S1: two saws detuned ±7 cents, low-pass at
//! 2.4 kHz with a little velocity to cutoff, A 5 ms, D 300 ms, S 70 %,
//! R 250 ms).
//!
//! Events apply at their exact frame: a block is rendered in spans between
//! them. Choosing another preset (or a program change) reaches the notes
//! already sounding: each voice glides to the new sound over
//! [`GLIDE_SECONDS`] without retriggering (the owner's choice, after
//! ADR-0045). Levels, filter, detune, pan, gain and the sustain level move
//! in a straight line; a changed waveform cross-fades. A voice in attack,
//! decay or sustain stays in its stage and heads for the new targets (a
//! held note moves to the new sustain level; if that is zero, as in Pluck,
//! it ends when the glide does, as a Pluck note would). A voice in release
//! keeps releasing, with the new release time (its level is continuous, so
//! it cannot click, and it always ends). Nothing allocates.
//!
//! The synth listens on every channel. A note is its channel and key: the
//! same key on two channels is two notes, and a note-off, a retrigger or a
//! steal ends only the note on its own channel (the piano roll previews on
//! channel 15, apart from a clip's channel 0). Pitch bend is ±2 semitones;
//! CC 64 is the sustain pedal; CC 1 and channel pressure open the filter a
//! little (up to an octave and a half); CC 120 silences, CC 123 releases, CC
//! 121 resets the controllers; all of these act on every channel. Program
//! changes 0 to 4 choose a preset. Bank selects are ignored.

use lumen_core::midi::{Event, Kind, Msg, cc};

use crate::Instrument;

mod voice;

use voice::*;

/// The synth's instrument number in a project (see [`Instrument`]).
pub const KIND: u32 = 1;
const VOICES: usize = 16;
/// How far the pitch bend wheel goes, in semitones.
const BEND_RANGE: f32 = 2.0;
/// How far the modulation wheel opens the filter, in octaves.
const MOD_OCTAVES: f32 = 1.5;
/// A preset change reaches sounding notes by gliding over this long: long
/// enough that no parameter steps audibly, short enough to feel at once.
pub const GLIDE_SECONDS: f32 = 0.025;
/// While gliding, a voice's derived values (filter, pitch) are worked out
/// again every this many frames.
const GLIDE_CHUNK: usize = 8;
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
            Kind::NoteOn { key, velocity } => self.note_on(msg.channel(), key, velocity),
            Kind::NoteOff { key, .. } => self.note_off(msg.channel(), key),
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

    fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        self.clock += 1;
        // The same key on the same channel again lets the first go.
        for v in &mut self.voices {
            if v.channel == channel && v.key == key && v.stage != Stage::Off && (v.down || v.pedal)
            {
                v.release();
            }
        }
        if let Some(v) = self.voices.iter_mut().find(|v| v.stage == Stage::Off) {
            v.start(channel, key, velocity, &self.sound, self.rate, self.clock);
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
            v.pending = Some((channel, key, velocity));
            v.age = self.clock;
            v.down = false;
            v.pedal = false;
        }
    }

    fn note_off(&mut self, channel: u8, key: u8) {
        for v in &mut self.voices {
            if v.stage == Stage::Steal
                && v.pending.is_some_and(|(c, k, _)| c == channel && k == key)
            {
                v.pending = None;
            } else if v.channel == channel && v.key == key && v.down && v.stage != Stage::Off {
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
            // Sounding notes glide to the new sound.
            for v in &mut self.voices {
                if v.stage != Stage::Off {
                    v.retarget(&self.sound, self.rate);
                }
            }
        }
    }

    fn silence(&mut self) {
        self.voices = [IDLE; VOICES];
        self.sustain = false;
    }
}
