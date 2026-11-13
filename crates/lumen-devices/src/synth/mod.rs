//! Flare, the built-in polyphonic synth (M5 slice 1, ADR-0045; its controls
//! in M8 slice B4): two band-limited oscillators and a noise generator per
//! voice, a state-variable filter with its own envelope, an amp envelope, an
//! LFO, glide and 1 to 16 voices, all read from the same parameter table a
//! project saves ([`params`]).
//!
//! Events apply at their exact frame: a block is rendered in spans between
//! them. A value the project holds reaches the sound without cutting what
//! sounds: [`Instrument::configure_values`] points every parameter at its new
//! value and the voices glide there over [`GLIDE_SECONDS`], and a lane's
//! points are played on their own frames ([`Instrument::automate`]), so
//! automating the cutoff is sample-exact.
//!
//! The words a value reads as, the five presets as parameter sets, and the
//! table's ids live in [`params`]. The DSP is played in `f64` arithmetic
//! worked out by hand ([`det`]), never through an `f32` `sin`, `cos`, `tan`,
//! `exp` or `powf`, so a golden render is the same on every platform. Nothing
//! allocates after the constructor.
//!
//! The synth listens on every channel. A note is its channel and key: the
//! same key on two channels is two notes, and a note-off, a retrigger or a
//! steal ends only the note on its own channel (the piano roll previews on
//! channel 15, apart from a clip's channel 0). Pitch bend is Â±2 semitones;
//! CC 64 is the sustain pedal; CC 1 and channel pressure open the filter a
//! little (up to an octave and a half); CC 120 silences, CC 123 releases, CC
//! 121 resets the controllers; all of these act on every channel. Program
//! changes 0 to 4 load one of the five presets into the values the synth
//! plays. Bank selects are ignored.

use lumen_core::midi::{Event, Kind, Msg, cc};

use crate::Instrument;
use crate::hosted::{MOST_FRAMES, ParamBlock};
use crate::params::Glide;

mod det;
pub mod params;
mod voice;

use voice::*;

/// The synth's instrument number in a project (see [`Instrument`]).
pub const KIND: u32 = 1;
/// How many voices a project may ask for.
pub const MOST_VOICES: usize = 16;
/// How far the pitch bend wheel goes, in semitones.
const BEND_RANGE: f32 = 2.0;
/// How far the modulation wheel opens the filter, in octaves.
const MOD_OCTAVES: f32 = 1.5;
/// A value the project holds reaches sounding notes by gliding over this
/// long: long enough that no parameter steps audibly, short enough to feel at
/// once. The same for a preset.
pub const GLIDE_SECONDS: f32 = 0.025;
/// A stolen voice fades out over this long.
const STEAL_SECONDS: f32 = 0.002;
/// Below this an envelope has ended (âˆ’80 dB).
const SILENT: f32 = 1e-4;
/// The tempo a synth with no transport plays a synced LFO at.
const DEFAULT_BPM: f32 = 120.0;

/// The five sounds Flare is shipped with: each is a set of parameter values
/// ([`params::preset_values`]).
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

    /// This preset as the values a project holds for it, by parameter id,
    /// normalised.
    pub fn values(self) -> Vec<(u32, f32)> {
        params::preset_values(self)
    }
}

/// A note waiting for a voice to fade out.
#[derive(Debug, Clone, Copy)]
pub(super) struct Waiting {
    pub(super) channel: u8,
    pub(super) key: u8,
    pub(super) velocity: u8,
    /// How far it glides to its pitch, and how long that takes.
    pub(super) glide_oct: f32,
    pub(super) glide: f32,
}

/// Flare.
pub struct Synth {
    voices: [Voice; MOST_VOICES],
    /// The values every parameter holds (normalised), as the project has
    /// them.
    targets: [f32; params::COUNT],
    /// Where each parameter is now: it glides to its target when the values
    /// move.
    glides: [Glide; params::COUNT],
    /// Each parameter's value for each frame of the block being rendered.
    curves: Box<[[f32; MOST_FRAMES]; params::COUNT]>,
    /// The lanes the track's automation plays this block.
    events: ParamBlock,
    /// The same lanes for the piece of it being played.
    chunk: ParamBlock,
    /// The sound a new note starts on.
    sound: Settings,
    /// The preset last loaded, for the record (`None` once edited).
    preset: Option<Preset>,
    rate: f32,
    /// The tempo a synced LFO follows.
    tempo: f32,
    /// How many notes sound at once.
    voice_limit: usize,
    /// Portamento, in seconds.
    glide: f32,
    /// The key the last note started on, for the next note's glide.
    last_key: Option<u8>,
    /// The pedal is down.
    sustain: bool,
    /// Pitch bend as a frequency ratio.
    bend: f32,
    /// The wheel and pressure, 0 to 1.
    wheel: f32,
    pressure: f32,
    clock: u64,
    /// Frames rendered since the stream began: the voices' derivation grid.
    frame: u64,
    /// The frame a value glide the project's values started ends at: while one
    /// runs, a voice works its settings out every [`voice::GLIDE_FRAMES`]
    /// frames rather than every [`voice::DERIVE_FRAMES`], so a change is a
    /// slope and never a step.
    glide_until: u64,
    /// Notes a full synth could not start (every voice was already fading):
    /// counted, never lost quietly (KNOWN_ISSUES, answered in B4).
    dropped: u32,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new(Preset::default())
    }
}

impl Synth {
    /// Flare on `preset`. Allocates; call off the audio thread.
    pub fn new(preset: Preset) -> Self {
        let mut synth = Self::with_values(&preset.values());
        synth.preset = Some(preset);
        synth
    }

    /// Flare on the values `values` gives its parameters (normalised, by id),
    /// and the table's defaults for any it does not name. Allocates; call off
    /// the audio thread.
    pub fn with_values(values: &[(u32, f32)]) -> Self {
        let mut targets = params::defaults();
        for &(id, value) in values {
            if let Some(slot) = targets.get_mut((id as usize).wrapping_sub(1))
                && params::find(id).is_some()
            {
                *slot = value.clamp(0.0, 1.0);
            }
        }
        let sound = Settings::from_values(&targets);
        Self {
            voices: [IDLE; MOST_VOICES],
            targets,
            glides: targets.map(Glide::new),
            curves: Box::new([[0.0; MOST_FRAMES]; params::COUNT]),
            events: ParamBlock::new(),
            chunk: ParamBlock::new(),
            voice_limit: sound.voices.clamp(1, MOST_VOICES),
            glide: sound.glide,
            sound,
            preset: None,
            rate: 48_000.0,
            tempo: DEFAULT_BPM,
            last_key: None,
            sustain: false,
            bend: 1.0,
            wheel: 0.0,
            pressure: 0.0,
            clock: 0,
            frame: 0,
            glide_until: 0,
            dropped: 0,
        }
    }

    /// The preset the sound was last loaded from, when nothing has been
    /// dialled since.
    pub fn preset(&self) -> Option<Preset> {
        self.preset
    }

    /// The values the parameters hold now (normalised, by id).
    pub fn values(&self) -> Vec<(u32, f32)> {
        params::TABLE
            .iter()
            .map(|info| (info.id, self.targets[info.id as usize - 1]))
            .collect()
    }

    /// Voices sounding now (tests).
    pub fn voices(&self) -> usize {
        self.voices
            .iter()
            .take(self.voice_limit)
            .filter(|v| v.stage != Stage::Off)
            .count()
    }

    /// Notes that could not start because every voice was already fading out
    /// (tests and the status line).
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// The tempo a synced LFO follows, as the transport has it.
    pub fn tempo(&self) -> f32 {
        self.tempo
    }

    /// The cutoff each frame of the last block played, in Hz (tests: that a
    /// lane on the cutoff is sample-exact).
    pub fn played_cutoff(&self, frames: usize) -> Vec<f32> {
        (0..frames.min(MOST_FRAMES))
            .map(|at| params::cutoff_hz(self.curves[(params::FILTER_CUTOFF - 1) as usize][at]))
            .collect()
    }

    /// Fills `self.chunk` with the lanes of `self.events` as they play in the
    /// piece of the block that begins at `base`: the value each holds at that
    /// frame, and its own points inside the piece.
    fn chunk(&mut self, base: usize, frames: usize) {
        self.chunk.start(frames);
        let events = &self.events;
        for i in 0..events.len() {
            let (id, points) = events.queue(i);
            if points.is_empty() || !self.chunk.open(id) {
                continue;
            }
            let at = base as u32;
            self.chunk
                .point(0, f64::from(crate::params::at(points, at)));
            for &(offset, value) in points {
                if offset > at && (offset as usize) < base + frames {
                    self.chunk.point(offset - at, value);
                }
            }
        }
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
        let waiting = Waiting {
            channel,
            key,
            velocity,
            glide_oct: self
                .last_key
                .map_or(0.0, |last| (f32::from(last) - f32::from(key)) / 12.0),
            glide: self.glide,
        };
        self.last_key = Some(key);
        let limit = self.voice_limit.clamp(1, MOST_VOICES);
        if let Some(v) = self.voices[..limit]
            .iter_mut()
            .find(|v| v.stage == Stage::Off)
        {
            v.start(
                waiting.channel,
                waiting.key,
                waiting.velocity,
                &self.sound,
                self.rate,
                self.clock,
                waiting.glide_oct,
                waiting.glide,
            );
            return;
        }
        // Full: the oldest voice fades out, then the new note takes it.
        let oldest = self.voices[..limit]
            .iter()
            .enumerate()
            .filter(|(_, v)| v.stage != Stage::Steal)
            .min_by_key(|(_, v)| v.age)
            .map(|(i, _)| i);
        match oldest {
            Some(i) => {
                let v = &mut self.voices[i];
                v.stage = Stage::Steal;
                v.steal = 1.0;
                v.pending = Some(waiting);
                v.age = self.clock;
                v.down = false;
                v.pedal = false;
            }
            // Every voice is already fading out: the newest note takes the
            // youngest steal's place, and the note it displaces is counted
            // rather than dropped without a trace (KNOWN_ISSUES, M5 review).
            None => {
                if let Some((i, _)) = self.voices[..limit]
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.age)
                {
                    self.voices[i].pending = Some(waiting);
                    self.dropped = self.dropped.saturating_add(1);
                }
            }
        }
    }

    fn note_off(&mut self, channel: u8, key: u8) {
        for v in &mut self.voices {
            if v.stage == Stage::Steal
                && v.pending
                    .is_some_and(|w| w.channel == channel && w.key == key)
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

    /// Points every parameter at `values` (normalised, by id): a knob moved,
    /// or a preset loaded. Sounding notes glide there; new notes start on it.
    fn point_at(&mut self, values: &[(u32, f32)]) {
        let mut targets = self.targets;
        for &(id, value) in values {
            if let Some(slot) = targets.get_mut((id as usize).wrapping_sub(1))
                && params::find(id).is_some()
            {
                *slot = value.clamp(0.0, 1.0);
            }
        }
        let sounding = self.voices.iter().any(|v| v.stage != Stage::Off);
        if sounding {
            self.glide_until = self.frame + (GLIDE_SECONDS * self.rate) as u64;
        }
        self.targets = targets;
        for (glide, &value) in self.glides.iter_mut().zip(targets.iter()) {
            if sounding {
                glide.head_to(value);
            } else {
                glide.settle(value);
            }
        }
        self.sound = Settings::from_values(&targets);
        self.voice_limit = self.sound.voices.clamp(1, MOST_VOICES);
        self.glide = self.sound.glide;
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
        if frames == 0 {
            // A call with no frames only delivers messages.
            for e in events {
                self.apply(e.msg);
            }
            return;
        }
        // A block is played in pieces of at most `MOST_FRAMES`, because the
        // curves a voice reads are that long: the engine's own block is a
        // chunk, and a longer one (a device of 480 frames, a test) is the same
        // samples cut smaller. Each piece's lanes are re-based to it, so the
        // curve played is the one the whole block would have played.
        let mut next = 0;
        let mut base = 0;
        while base < frames {
            let n = (frames - base).min(MOST_FRAMES);
            self.chunk(base, n);
            for info in params::TABLE {
                let slot = (info.id - 1) as usize;
                crate::params::values(
                    &mut self.curves[slot][..n],
                    &mut self.glides[slot],
                    &self.chunk,
                    info.id,
                    rate.max(1),
                );
            }
            let start = self.frame;
            let mut done = 0;
            loop {
                while next < events.len() && events[next].offset as usize <= base + done {
                    self.apply(events[next].msg);
                    next += 1;
                }
                if done >= n {
                    break;
                }
                let end = events
                    .get(next)
                    .map_or(n, |e| (e.offset as usize).saturating_sub(base).min(n));
                let end = end.max(done);
                // Everything a span reads is worked out here, from the frame
                // it begins at: a controller moved by an event reaches the
                // sound on that event's frame, and whether a value is arriving
                // is a question about the frame, never about the cut.
                let changing = self.glide_until > start + done as u64;
                let span = Span {
                    rate: self.rate,
                    bend: self.bend,
                    open: self.wheel.max(self.pressure) * MOD_OCTAVES,
                    sound: self.sound,
                    tempo: self.tempo,
                    changing,
                    derive: if changing {
                        voice::GLIDE_FRAMES
                    } else {
                        voice::DERIVE_FRAMES
                    },
                    curves: &Curves::new(&self.curves, start, n),
                };
                for voice in &mut self.voices {
                    if voice.stage != Stage::Off {
                        voice.render(
                            &span,
                            done,
                            &mut left[base + done..base + end],
                            &mut right[base + done..base + end],
                        );
                    }
                }
                done = end;
            }
            self.frame += n as u64;
            base += n;
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
            self.point_at(&preset.values());
            self.preset = Some(preset);
        }
    }

    fn configure_values(&mut self, values: &[(u32, f32)]) {
        if values.is_empty() {
            return;
        }
        self.point_at(values);
        // The preset is only a name: once values are set by hand, the sound
        // is the values.
        let same_as_preset = Preset::ALL.into_iter().any(|preset| {
            let wanted = preset.values();
            wanted
                .iter()
                .all(|&(id, value)| (self.targets[id as usize - 1] - value).abs() < 1e-6)
        });
        if !same_as_preset {
            self.preset = None;
        }
    }

    fn set_tempo(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            self.tempo = bpm;
        }
    }

    fn automate(&mut self, params: &ParamBlock) {
        // Copied, never borrowed: the engine's block lives for its own block
        // and this is read while the next one renders.
        self.events.copy_from(params);
    }

    fn silence(&mut self) {
        self.voices = [IDLE; MOST_VOICES];
        self.sustain = false;
    }
}
