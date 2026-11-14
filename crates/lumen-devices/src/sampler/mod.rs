//! Shutter, the built-in sampler (M8 slice B5, ADR-XXXX).
//!
//! One instrument with three faces (the owner's SP A): a **One shot** region
//! played at its root key and transposed away from it, **Zones** of key and
//! velocity ranges, and **Pads**, one per sound of the track's drum map. What
//! it plays is a [`Sound`], built on the interface side and handed over as
//! immutable data; the engine never decodes, never copies a file, and never
//! frees one — the interface's library owns every buffer (see
//! [`Sampler::load_sound`]).
//!
//! Real-time rules: no allocation, no lock and no blocking after construction.
//! A voice holds its own handle to the sample it started with, so a new sound
//! never disturbs a note already sounding, and the per-block parameter curves
//! are filled into arrays made once by the constructor.
//!
//! Everything that is not straight arithmetic is worked out in `f64` and
//! rounded to `f32` once — the pitch ratio, the filter's coefficients — so a
//! render is the same on every platform (M8 slice B1's lesson).

mod sound;
mod table;
mod voice;

pub use sound::{Face, Pad, Region, Sample, Sound, Zone};
pub use table::{
    ATTACK, CUTOFF, DECAY, FILTER, FINE, LEVEL, PITCH, RELEASE, RESONANCE, SUSTAIN, TABLE, VELOCITY,
};

use table::PARAMS;
use voice::{Curves, Envelope, FrameValues, Fx, Pairs, Stage, Voice, db_to_gain};

use std::sync::Arc;

use lumen_core::midi::{Event, Kind};
use lumen_core::pan::PanLaw;

use crate::ParamBlock;
use crate::instrument::Instrument;
use crate::params::{self, Glide};

/// The device's stable name in `PluginRef::Builtin`, as a project saves it.
/// Never the product name, and never changed.
pub const NAME: &str = "sampler";

/// Which instrument a track keeps its voices across: a sampler stays the same
/// sampler while a note sounds, whatever it plays.
pub const KIND: u32 = 0x5348_5554; // "SHUT"

/// The most notes that sound at once. Past it the oldest is taken.
pub const MAX_VOICES: usize = 64;

/// The auxiliary output pairs a pad can be routed to (§6.4's "a specific
/// instrument output", the owner's RT). A pad whose `output` is 0 plays on the
/// track's own output.
pub const PAIRS: usize = 8;

/// How long a choked pad fades by default, in seconds (the owner's CO A).
pub const DEFAULT_CHOKE: f32 = 0.005;

/// Frames between two readings of what a voice's pitch and filter are: about
/// two thirds of a millisecond at 48 kHz. A voice counts its own frames, so
/// the readings fall on the same frames at any block size.
const KNOT: u64 = 32;

/// The highest cutoff, which is also the top of the parameter: at it the
/// filter is out of the path, so an unfiltered sample is the file's own frames
/// to the bit.
const FILTER_TOP_HZ: f32 = 20_000.0;

/// What the interface reads of the sampler: its stable name and its table.
/// An instrument has no latency in the chain (it makes the sound, it does not
/// delay another) and no tail of its own (its voices report when they sound,
/// through [`Instrument::active`]), so both are zero whatever its values say.
pub static DESCRIPTION: crate::Description = crate::Description {
    name: NAME,
    table: TABLE,
    latency: |_| 0,
    tail: |_| 0,
};

/// Shutter.
pub struct Sampler {
    /// What it plays. Replaced whole when the project's sound changes; the
    /// notes already sounding keep theirs.
    sound: Arc<Sound>,
    voices: [Voice; MAX_VOICES],
    /// Where each parameter was at the end of the last block.
    glides: [Glide; PARAMS],
    curves: Box<Curves>,
    pairs: Box<Pairs>,
    /// How many frames `curves` holds, and the rate they were filled at.
    filled: usize,
    rate: u32,
    /// The order the next voice takes.
    order: u64,
    /// Frames written to the auxiliary outputs last block.
    frames: usize,
}

impl Sampler {
    /// Shutter playing `sound` at its defaults: unity level, no pitch, no
    /// filter, a 50 ms release. Allocates its curves and output buffers: call
    /// it on the interface side.
    pub fn new(sound: Arc<Sound>) -> Self {
        let at = |id: u32| {
            let info = params::find(TABLE, id).expect("the table names its own ids");
            info.normalised(info.default)
        };
        let mut glides = [Glide::new(0.0); PARAMS];
        for (index, glide) in glides.iter_mut().enumerate() {
            let id = TABLE[index].id;
            *glide = Glide::new(at(id));
        }
        Self {
            sound,
            voices: [Voice::IDLE; MAX_VOICES],
            glides,
            curves: Curves::new(),
            pairs: Pairs::new(),
            filled: 0,
            rate: 48_000,
            order: 0,
            frames: 0,
        }
    }

    /// Shutter with nothing to play.
    pub fn silent() -> Self {
        Self::new(Arc::new(Sound::default()))
    }

    /// What it plays now.
    pub fn sound(&self) -> &Arc<Sound> {
        &self.sound
    }

    /// How many notes sound now.
    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.playing()).count()
    }

    /// Takes `sound` to play, returning the one it played before: the caller
    /// frees that on the interface side, so the audio thread never frees a
    /// sound or a sample (M8 slice B5). Notes already sounding keep the
    /// sample they started with. Real-time safe: one handle swap, nothing
    /// dropped.
    pub fn load_sound(&mut self, sound: Arc<Sound>) -> Option<Arc<Sound>> {
        if Arc::ptr_eq(&self.sound, &sound) {
            return None;
        }
        Some(std::mem::replace(&mut self.sound, sound))
    }

    /// Fills the block's curves from what the track's values and lanes say,
    /// frame by frame. Called before the block's [`Instrument::render`];
    /// regions, zones and pads are the sound's own, not parameters.
    pub fn automate(&mut self, block: &ParamBlock) {
        let frames = block.frames().min(crate::MOST_FRAMES);
        self.fill(frames, block);
    }

    /// Fills the curves for `frames` frames from `block`.
    fn fill(&mut self, frames: usize, block: &ParamBlock) {
        let rate = self.rate;
        for (index, info) in TABLE.iter().enumerate() {
            let curve = self.curves.slot(index);
            params::values(
                &mut curve[..frames],
                &mut self.glides[index],
                block,
                info.id,
                rate,
            );
        }
        self.filled = frames;
    }

    /// Values at `frame` of the block, in the parameters' own units.
    fn at(&self, frame: usize, index: usize) -> f32 {
        let curve = match index {
            0 => &self.curves.level,
            1 => &self.curves.pitch,
            2 => &self.curves.fine,
            3 => &self.curves.attack,
            4 => &self.curves.decay,
            5 => &self.curves.sustain,
            6 => &self.curves.release,
            7 => &self.curves.filter,
            8 => &self.curves.cutoff,
            9 => &self.curves.resonance,
            _ => &self.curves.velocity,
        };
        // The table's own row, not a search: this runs for every voice of
        // every frame.
        TABLE[index].value(curve[frame.min(self.filled.saturating_sub(1))])
    }

    /// The envelope's four times at `frame`.
    fn times(&self, frame: usize) -> [f32; 4] {
        [
            self.at(frame, 3),
            self.at(frame, 4),
            self.at(frame, 5) / 100.0,
            self.at(frame, 6),
        ]
    }

    /// What every voice of a frame reads: the pitch and fine offset, the
    /// filter's settings and the velocity's share. Worked out once a frame,
    /// not once a voice.
    fn frame_values(&self, frame: usize) -> FrameValues {
        FrameValues {
            pitch: self.at(frame, 1),
            fine: self.at(frame, 2),
            mode: self.at(frame, 7).round().clamp(0.0, 2.0) as u8,
            cutoff: self.at(frame, 8),
            resonance: self.at(frame, 9) / 100.0,
            velocity: (self.at(frame, PARAMS - 1) / 100.0).clamp(0.0, 1.0),
        }
    }

    /// Strikes a note at `frame` of the block.
    fn start(&mut self, frame: usize, key: u8, velocity: u8) {
        if velocity == 0 {
            self.release(key, frame);
            return;
        }
        let values = self.frame_values(frame);
        let Some(source) = self.sound.voice_for(key, velocity) else {
            return;
        };
        // What the note plays, copied out so the chokes below can change the
        // voices (the sound is only read).
        let region = source.region;
        let (sample, start, length) = (
            Arc::clone(&region.sample),
            region.start.min(region.sample.frames()),
            region.length(),
        );
        let (reversed, looping, root) = (region.reversed, region.looping, region.root);
        let (pan, tune, output) = (source.pan, source.tune, source.output);
        let velocity_gain = 1.0 - values.velocity + values.velocity * (f32::from(velocity) / 127.0);
        let (choke, choke_fade, gain) =
            (source.choke, source.choke_fade, source.gain * velocity_gain);
        // A pad that is in a choke group ends its group's other notes at this
        // very frame, each over its own fade (the owner's CO A: the engine
        // sends the note-off too, and this makes it click-free either way); a
        // pad struck again takes its own earlier voice with it the same way.
        let mut choked = [0_u8; 128];
        let mut count = 0;
        if let Some(group) = choke {
            for note in self.sound.choked_by(group) {
                if count < choked.len() {
                    choked[count] = note;
                    count += 1;
                }
            }
        }
        for &note in &choked[..count] {
            self.choke_note(note);
        }
        self.choke_note(key);
        let sides = if pan == 0.0 {
            [1.0, 1.0]
        } else {
            PanLaw::Balance.gains(pan)
        };
        self.order += 1;
        let index = self.free_voice();
        self.voices[index] = Voice {
            sample: Some(sample),
            start,
            length,
            reversed,
            looping,
            phase: 0.0,
            step: 1.0,
            key,
            root,
            tune,
            gain,

            sides,
            output: output.min(PAIRS as u32) as u8,
            choke,
            choke_fade,
            env: Envelope::START,
            age: 0,
            semitones: 0.0,
            release_seconds: 0.0,
            filter: [[0.0; 2]; 2],
            cut: Fx::OFF,
            order: self.order,
        };
        self.tune_voice(index, &values);
    }

    /// Ends the sounding voices of `note` at once, over their own choke fade
    /// (or the device's release when they have none).
    fn choke_note(&mut self, note: u8) {
        let times = self.times(0);
        for voice in &mut self.voices {
            if !voice.playing() || voice.key != note {
                continue;
            }
            voice.env.release();
            voice.release_seconds = voice.choke_fade.unwrap_or(times[3]);
        }
    }

    /// Releases every voice of `key` at `frame`: a note-off. A pad in a choke
    /// group fades over its choke fade; a pad outside one plays its sample to
    /// the end, as a drum sampler does; a melodic voice takes the release.
    fn release(&mut self, key: u8, _frame: usize) {
        for voice in &mut self.voices {
            if !voice.playing() || voice.key != key {
                continue;
            }
            match voice.choke_fade {
                // A pad in a choke group: the choke's own fade.
                Some(fade) if voice.choke.is_some() => {
                    voice.env.release();
                    voice.release_seconds = fade;
                }
                // A pad outside one, and a melodic voice: nothing to do for
                // the pad (it rings out), the release for the voice.
                Some(_) => {}
                None => {
                    voice.env.release();
                    voice.release_seconds = 0.0;
                }
            }
        }
    }

    /// Reads a voice's pitch and filter at its `age`, and sets its step.
    fn tune_voice(&mut self, index: usize, values: &FrameValues) {
        let rate = self.rate;
        let voice = &mut self.voices[index];
        let Some(sample) = voice.sample.as_ref() else {
            return;
        };
        let semitones = f64::from(i16::from(voice.key) - i16::from(voice.root))
            + f64::from(voice.tune)
            + f64::from(values.pitch)
            + f64::from(values.fine) / 100.0;
        voice.semitones = semitones;
        voice.step = (semitones / 12.0).exp2() * f64::from(sample.rate()) / f64::from(rate.max(1));
        voice.cut = Fx::new(values.mode, values.cutoff, values.resonance, rate);
    }

    /// A voice to strike: one that is done, else the oldest.
    fn free_voice(&self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.playing()) {
            return i;
        }
        let mut oldest = 0;
        for (i, voice) in self.voices.iter().enumerate() {
            if voice.order < self.voices[oldest].order {
                oldest = i;
            }
        }
        oldest
    }

    /// Mixes one voice's frame into the block.
    #[inline]
    fn mix(
        &mut self,
        index: usize,
        frame: usize,
        (times, values): ([f32; 4], &FrameValues),
        left: &mut [f32],
        right: &mut [f32],
    ) {
        if !self.voices[index].playing() {
            return;
        }
        // The pitch and the filter are read at the voice's own knots, so a
        // block of any size plays the same frames.
        if self.voices[index].age.is_multiple_of(KNOT) {
            self.tune_voice(index, values);
        }
        let mut times = times;
        if self.voices[index].release_seconds > 0.0 {
            times[3] = self.voices[index].release_seconds;
        }
        let rate = self.rate;
        let voice = &mut self.voices[index];
        voice.env.step(times, rate);
        if voice.env.done() {
            return;
        }
        let env = voice.env.level;
        let l = voice.read(0, voice.phase);
        let r = voice.read(1, voice.phase);
        let l = voice.cut.run(&mut voice.filter[0], l);
        let r = voice.cut.run(&mut voice.filter[1], r);
        let (out_l, out_r) = (l as f32 * env * voice.gain, r as f32 * env * voice.gain);
        let (side_l, side_r) = (out_l * voice.sides[0], out_r * voice.sides[1]);
        let pair = voice.output;
        if !voice.advance(voice.step) {
            voice.env.stage = Stage::Done;
        }
        voice.age += 1;
        if pair == 0 {
            left[frame] += side_l;
            right[frame] += side_r;
        } else {
            let pair = usize::from(pair) - 1;
            if pair < PAIRS {
                self.pairs.left[pair][frame] += side_l;
                self.pairs.right[pair][frame] += side_r;
            }
        }
    }

    /// The level parameter's gain at `frame`.
    fn level_gain(&self, frame: usize) -> f32 {
        db_to_gain(self.at(frame, 0))
    }
}

impl Instrument for Sampler {
    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len()).min(crate::MOST_FRAMES);
        if rate != 0 {
            self.rate = rate;
        }
        if frames == 0 {
            // Messages only: a note-on starts a voice that plays on the next
            // block, at the block's first frame. Nothing of a pair is handed
            // out until a block is rendered again.
            self.frames = 0;
            for event in events {
                match event.msg.kind() {
                    Kind::NoteOn { key, velocity } if velocity > 0 => self.start(0, key, velocity),
                    Kind::NoteOff { key, velocity: _ } => self.release(key, 0),
                    _ => {}
                }
            }
            return;
        }
        if self.filled != frames {
            // No block was given for this call (a release played out on the
            // interface side of a jump): hold what every parameter is.
            let none = ParamBlock::none();
            self.fill(frames, &none);
        }
        left[..frames].fill(0.0);
        right[..frames].fill(0.0);
        self.pairs.clear(frames);
        self.frames = frames;
        // Events are sorted by offset and apply at their exact frame.
        let mut next = 0;
        for frame in 0..frames {
            while next < events.len() && events[next].offset as usize == frame {
                match events[next].msg.kind() {
                    Kind::NoteOn { key, velocity } if velocity > 0 => {
                        self.start(frame, key, velocity)
                    }
                    Kind::NoteOff { key, velocity: _ } => self.release(key, frame),
                    _ => {}
                }
                next += 1;
            }
            let level = self.level_gain(frame);
            let values = self.frame_values(frame);
            let times = self.times(frame);
            for index in 0..MAX_VOICES {
                if self.voices[index].playing() {
                    self.mix(index, frame, (times, &values), left, right);
                }
            }
            let (l, r) = (left[frame], right[frame]);
            left[frame] = l * level;
            right[frame] = r * level;
            for pair in 0..PAIRS {
                self.pairs.left[pair][frame] *= level;
                self.pairs.right[pair][frame] *= level;
            }
        }
        // Messages at or past the end apply after the last frame.
        while next < events.len() {
            match events[next].msg.kind() {
                Kind::NoteOn { key, velocity } if velocity > 0 => {
                    self.start(frames - 1, key, velocity)
                }
                Kind::NoteOff { key, velocity: _ } => self.release(key, frames - 1),
                _ => {}
            }
            next += 1;
        }
    }

    fn active(&self) -> bool {
        self.voices.iter().any(Voice::playing)
    }

    fn silence(&mut self) {
        for voice in &mut self.voices {
            voice.env.stage = Stage::Done;
            voice.sample = None;
        }
    }

    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        let frames = left.len().min(right.len()).min(self.frames);
        if pair == 0 || pair > PAIRS {
            left.fill(0.0);
            right.fill(0.0);
            return false;
        }
        left[..frames].copy_from_slice(&self.pairs.left[pair - 1][..frames]);
        right[..frames].copy_from_slice(&self.pairs.right[pair - 1][..frames]);
        // Past the block last rendered there is nothing of this pair.
        left[frames..].fill(0.0);
        right[frames..].fill(0.0);
        true
    }

    fn load_sound(&mut self, sound: Arc<Sound>) -> Option<Arc<Sound>> {
        Sampler::load_sound(self, sound)
    }

    fn automate(&mut self, params: &ParamBlock) {
        Sampler::automate(self, params);
    }
}
