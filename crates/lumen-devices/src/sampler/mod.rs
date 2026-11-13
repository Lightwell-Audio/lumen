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

pub use sound::{Face, Pad, Region, Sample, Sound, Zone};

use std::sync::Arc;

use lumen_core::midi::{Event, Kind};
use lumen_core::pan::PanLaw;

use crate::ParamBlock;
use crate::instrument::Instrument;
use crate::params::{self, Glide, ParamInfo, Taper};

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

/// Level, in decibels.
pub const LEVEL: u32 = 1;
/// Pitch, in semitones.
pub const PITCH: u32 = 2;
/// Fine pitch, in hundredths of a semitone.
pub const FINE: u32 = 3;
/// Attack, in seconds.
pub const ATTACK: u32 = 4;
/// Decay, in seconds.
pub const DECAY: u32 = 5;
/// Sustain, as a share of the note's own level.
pub const SUSTAIN: u32 = 6;
/// Release, in seconds.
pub const RELEASE: u32 = 7;
/// The filter: off, low pass or high pass.
pub const FILTER: u32 = 8;
/// The filter's cutoff, in Hz.
pub const CUTOFF: u32 = 9;
/// The filter's resonance, as a share.
pub const RESONANCE: u32 = 10;
/// How much a note's velocity sets its level.
pub const VELOCITY: u32 = 11;

/// How many parameters the table holds.
const PARAMS: usize = 11;

/// The lowest level: under it, silence.
const FLOOR_DB: f32 = -60.0;

/// What the columns of the table are for, so a reader sees the shape of a row.
#[allow(clippy::too_many_arguments)]
const fn param(
    id: u32,
    name: &'static str,
    unit: &'static str,
    low: f32,
    high: f32,
    taper: Taper,
    default: f32,
    steps: i32,
    choices: &'static [&'static str],
    text: fn(f32) -> String,
    parse: fn(&str) -> Option<f32>,
) -> ParamInfo {
    ParamInfo {
        id,
        name,
        unit,
        low,
        high,
        taper,
        default,
        steps,
        choices,
        text,
        parse,
    }
}

/// Shutter's parameters, in the order the card and All parameters show them.
/// Ids are stable and never reused. They hold what every face shares; the
/// regions, zones and pads are the sound's own ([`Sound`]), not parameters.
pub static TABLE: &[ParamInfo] = &[
    param(
        LEVEL,
        "Level",
        "dB",
        FLOOR_DB,
        12.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        gain_text,
        gain_parse,
    ),
    param(
        PITCH,
        "Pitch",
        "st",
        -24.0,
        24.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        semitone_text,
        semitone_parse,
    ),
    param(
        FINE,
        "Fine",
        "ct",
        -100.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        cents_text,
        cents_parse,
    ),
    param(
        ATTACK,
        "Attack",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.0,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        DECAY,
        "Decay",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.0,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        SUSTAIN,
        "Sustain",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        100.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        RELEASE,
        "Release",
        "s",
        0.0,
        10.0,
        Taper::Power(3.0),
        0.05,
        0,
        &[],
        seconds_text,
        seconds_parse,
    ),
    param(
        FILTER,
        "Filter",
        "",
        0.0,
        2.0,
        Taper::Linear,
        0.0,
        2,
        &["Off", "Low pass", "High pass"],
        filter_text,
        filter_parse,
    ),
    param(
        CUTOFF,
        "Cutoff",
        "Hz",
        20.0,
        FILTER_TOP_HZ,
        Taper::Power(3.0),
        FILTER_TOP_HZ,
        0,
        &[],
        cutoff_text,
        cutoff_parse,
    ),
    param(
        RESONANCE,
        "Res",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
    param(
        VELOCITY,
        "Vel>Level",
        "%",
        0.0,
        100.0,
        Taper::Linear,
        0.0,
        0,
        &[],
        percent_text,
        percent_parse,
    ),
];

fn gain_text(db: f32) -> String {
    if db <= FLOOR_DB {
        "\u{2212}\u{221E} dB".to_owned()
    } else if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

fn gain_parse(text: &str) -> Option<f32> {
    number_in(text, &["db"]).filter(|db| *db <= 12.0)
}

fn semitone_text(st: f32) -> String {
    if st.abs() < 0.005 {
        "0 st".to_owned()
    } else {
        format!("{st:+.2} st").replace('-', "\u{2212}")
    }
}

fn semitone_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("st").trim();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn cents_text(ct: f32) -> String {
    if ct.abs() < 0.05 {
        "0 ct".to_owned()
    } else {
        format!("{ct:+.0} ct").replace('-', "\u{2212}")
    }
}

fn cents_parse(text: &str) -> Option<f32> {
    number_in(text, &["ct", "cents"])
}

fn seconds_text(s: f32) -> String {
    if s < 0.0005 {
        "0 ms".to_owned()
    } else if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

fn seconds_parse(text: &str) -> Option<f32> {
    let t = text.trim().to_lowercase();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    if let Some(s) = t.strip_suffix('s') {
        return s.trim().parse::<f32>().ok();
    }
    t.parse::<f32>().ok()
}

fn percent_text(percent: f32) -> String {
    format!("{percent:.0} %")
}

fn percent_parse(text: &str) -> Option<f32> {
    number_in(text, &["%"])
}

fn filter_text(value: f32) -> String {
    match value.round() as i32 {
        1 => "Low pass",
        2 => "High pass",
        _ => "Off",
    }
    .to_owned()
}

fn filter_parse(text: &str) -> Option<f32> {
    match text.trim().to_lowercase().as_str() {
        "off" | "none" | "0" => Some(0.0),
        "low" | "low pass" | "lowpass" | "lp" | "1" => Some(1.0),
        "high" | "high pass" | "highpass" | "hp" | "2" => Some(2.0),
        _ => None,
    }
}

fn cutoff_text(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{:.1} kHz", hz / 1_000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

fn cutoff_parse(text: &str) -> Option<f32> {
    let t = text.trim().replace('\u{2212}', "-").to_lowercase();
    if let Some(k) = t.strip_suffix("khz") {
        return k.trim().parse::<f32>().ok().map(|v| v * 1_000.0);
    }
    number_in(&t, &["hz"])
}

/// A number in `text`, with any of `units` after it and either minus.
fn number_in(text: &str, units: &[&str]) -> Option<f32> {
    let mut t = text.trim().replace('\u{2212}', "-").to_lowercase();
    for unit in units {
        if let Some(rest) = t.strip_suffix(unit) {
            t = rest.trim_end().to_owned();
        }
    }
    t.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// The value of a normalised parameter in its own units.
fn own(id: u32, x: f32) -> f32 {
    params::find(TABLE, id).map_or(0.0, |info| info.value(x))
}

/// A gain in decibels, silent at the floor. Worked out in `f64`.
fn db_to_gain(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        (10.0_f64).powf(f64::from(db) / 20.0) as f32
    }
}

/// A tiny value becomes zero, so a decaying filter never reaches denormals.
fn flush(value: f64) -> f64 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

/// Which part of its envelope a voice is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// The gain of the amp envelope through one voice.
#[derive(Debug, Clone, Copy)]
struct Envelope {
    stage: Stage,
    /// The level now, 0 to 1.
    level: f32,
    /// The level the release started from.
    release_from: f32,
}

impl Envelope {
    const START: Envelope = Envelope {
        stage: Stage::Attack,
        level: 0.0,
        release_from: 0.0,
    };

    fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    fn release(&mut self) {
        if self.stage != Stage::Done {
            self.release_from = self.level;
            self.stage = Stage::Release;
        }
    }

    /// Moves on one frame with `times` (attack, decay, sustain, release in
    /// seconds) at `rate`.
    #[inline]
    fn step(&mut self, times: [f32; 4], rate: u32) {
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

/// The filter's settings at a voice's last knot.
#[derive(Debug, Clone, Copy)]
struct Fx {
    /// 0 is out of the path, 1 low pass, 2 high pass.
    mode: u8,
    a: [f64; 3],
    k: f64,
}

impl Fx {
    /// No filter: the sample passes untouched, to the bit.
    const OFF: Fx = Fx {
        mode: 0,
        a: [0.0; 3],
        k: 0.0,
    };

    /// The coefficients for `mode`, `hz` and `resonance` (0 to 1) at `rate`,
    /// worked out in `f64`.
    fn new(mode: u8, hz: f32, resonance: f32, rate: u32) -> Self {
        if mode == 0 || rate == 0 || hz >= FILTER_TOP_HZ {
            return Self::OFF;
        }
        let nyquist = f64::from(rate) * 0.5;
        let cutoff = f64::from(hz).clamp(10.0, nyquist * 0.98);
        // A state-variable filter's `g` and damping: resonance takes the
        // damping from critically damped towards ringing.
        let g = (std::f64::consts::PI * cutoff / f64::from(rate)).tan();
        let k = 2.0 - 1.9 * f64::from(resonance.clamp(0.0, 1.0));
        let a = 1.0 / (1.0 + g * (g + k));
        Self {
            mode,
            a: [a, g * a, g * g * a],
            k,
        }
    }

    /// One frame through the filter, `state` being the two integrators.
    #[inline]
    fn run(&self, state: &mut [f64; 2], x: f64) -> f64 {
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

/// One sounding note.
struct Voice {
    /// The sample it plays, kept with the voice so a new sound never disturbs
    /// a note already sounding. The interface's library holds every buffer,
    /// so a voice letting go here never frees one on this thread.
    sample: Option<Arc<Sample>>,
    /// The region, in frames of `sample`.
    start: usize,
    length: usize,
    reversed: bool,
    looping: bool,
    /// Where it is in the region, in frames, from 0 to `length`.
    phase: f64,
    /// Frames of the region per frame of output.
    step: f64,
    /// The note it answers, the note it was struck at, and the region's root.
    key: u8,
    note: u8,
    root: u8,
    /// The pad's tune, in semitones.
    tune: f32,
    /// The pad's level, the note's velocity, and the pan's two gains.
    gain: f32,

    sides: [f32; 2],
    /// The output pair: 0 is the track's own.
    output: u8,
    /// The group that chokes it, and how long it fades when it is.
    choke: Option<u8>,
    choke_fade: Option<f32>,
    env: Envelope,
    /// Frames since the note started, and the pitch it last read.
    age: u64,
    semitones: f64,
    /// The release this voice takes, when it is not the device's own (a
    /// choke fade); 0 when the release curve says.
    release_seconds: f32,
    /// The filter's two integrating states, per side.
    filter: [[f64; 2]; 2],
    cut: Fx,
    /// Voices sound in the order they started, so the oldest is taken first.
    order: u64,
}

impl Voice {
    const IDLE: Voice = Voice {
        sample: None,
        start: 0,
        length: 0,
        reversed: false,
        looping: false,
        phase: 0.0,
        step: 1.0,
        key: 0,
        note: 0,
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

    fn playing(&self) -> bool {
        self.sample.is_some() && !self.env.done()
    }

    /// Where in the file the region's frame `at` is: a reversed region runs
    /// backwards through its frames.
    #[inline]
    fn index(&self, at: f64) -> f64 {
        if self.reversed {
            self.start as f64 + (self.length as f64 - 1.0) - at
        } else {
            self.start as f64 + at
        }
    }

    /// The sample's value `at` frames into the region, on `channel`, read
    /// through the region and interpolated between its neighbours.
    #[inline]
    fn read(&self, channel: usize, at: f64) -> f64 {
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
        // Catmull-Rom through the neighbours on each side: smooth when a
        // sample plays away from its root, and the frame itself when it does
        // not (the branch above is the ratio-of-one path).
        let at = |offset: isize| f64::from(sample.at(channel, base as isize + offset));
        let (p0, p1, p2, p3) = (at(-1), at(0), at(1), at(2));
        let (f, f2, f3) = (into, into * into, into * into * into);
        0.5 * ((2.0 * p1)
            + (-p0 + p2) * f
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f3)
    }

    /// Moves `frames` frames into the region, wrapping when it loops; false
    /// when it has run out.
    #[inline]
    fn advance(&mut self, frames: f64) -> bool {
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

/// The parameters' values through one block, normalised: one curve each,
/// filled by [`Sampler::automate`]. Made once by the constructor.
struct Curves {
    level: [f32; crate::MOST_FRAMES],
    pitch: [f32; crate::MOST_FRAMES],
    fine: [f32; crate::MOST_FRAMES],
    attack: [f32; crate::MOST_FRAMES],
    decay: [f32; crate::MOST_FRAMES],
    sustain: [f32; crate::MOST_FRAMES],
    release: [f32; crate::MOST_FRAMES],
    filter: [f32; crate::MOST_FRAMES],
    cutoff: [f32; crate::MOST_FRAMES],
    resonance: [f32; crate::MOST_FRAMES],
    velocity: [f32; crate::MOST_FRAMES],
}

impl Curves {
    fn new() -> Box<Self> {
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

    /// The curve of the parameter at `index` of the table, `frames` long.
    fn slot(&mut self, index: usize) -> &mut [f32] {
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

/// The auxiliary outputs of the block last rendered, per pair and side.
struct Pairs {
    left: [[f32; crate::MOST_FRAMES]; PAIRS],
    right: [[f32; crate::MOST_FRAMES]; PAIRS],
}

impl Pairs {
    fn new() -> Box<Self> {
        Box::new(Self {
            left: [[0.0; crate::MOST_FRAMES]; PAIRS],
            right: [[0.0; crate::MOST_FRAMES]; PAIRS],
        })
    }

    fn clear(&mut self, frames: usize) {
        for pair in 0..PAIRS {
            self.left[pair][..frames].fill(0.0);
            self.right[pair][..frames].fill(0.0);
        }
    }
}

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
        own(
            TABLE[index].id,
            curve[frame.min(self.filled.saturating_sub(1))],
        )
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

    /// The gain a note of `velocity` gets, from the block's Vel>Level.
    fn velocity_gain(&self, frame: usize, velocity: u8) -> f32 {
        let amount = (self.at(frame, PARAMS - 1) / 100.0).clamp(0.0, 1.0);
        1.0 - amount + amount * (f32::from(velocity) / 127.0)
    }

    /// Strikes a note at `frame` of the block.
    fn start(&mut self, frame: usize, key: u8, velocity: u8) {
        if velocity == 0 {
            self.release(key, frame);
            return;
        }
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
        let (choke, choke_fade, gain) = (
            source.choke,
            source.choke_fade,
            source.gain * self.velocity_gain(frame, velocity),
        );
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
            note: key,
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
        self.tune_voice(index, frame);
    }

    /// Ends the sounding voices of `note` at once, over their own choke fade
    /// (or the device's release when they have none).
    fn choke_note(&mut self, note: u8) {
        let times = self.times(0);
        for voice in &mut self.voices {
            if !voice.playing() || voice.note != note {
                continue;
            }
            voice.env.release();
            voice.release_seconds = voice.choke_fade.unwrap_or(times[3]);
        }
    }

    /// Releases every voice of `key` at `frame`: a note-off. A pad in a choke
    /// group fades over its choke fade; a pad outside one plays its sample to
    /// the end, as a drum sampler does; a melodic voice takes the release.
    fn release(&mut self, key: u8, frame: usize) {
        let times = self.times(frame);
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
                    let _ = times;
                }
            }
        }
    }

    /// Reads a voice's pitch and filter at its `age`, and sets its step.
    fn tune_voice(&mut self, index: usize, frame: usize) {
        let pitch = self.at(frame, 1);
        let fine = self.at(frame, 2);
        let mode = self.at(frame, 7).round().clamp(0.0, 2.0) as u8;
        let cutoff = self.at(frame, 8);
        let resonance = self.at(frame, 9) / 100.0;
        let rate = self.rate;
        let voice = &mut self.voices[index];
        let Some(sample) = voice.sample.as_ref() else {
            return;
        };
        let semitones = f64::from(i16::from(voice.key) - i16::from(voice.root))
            + f64::from(voice.tune)
            + f64::from(pitch)
            + f64::from(fine) / 100.0;
        voice.semitones = semitones;
        voice.step = (semitones / 12.0).exp2() * f64::from(sample.rate()) / f64::from(rate.max(1));
        voice.cut = Fx::new(mode, cutoff, resonance, rate);
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
    fn mix(&mut self, index: usize, frame: usize, left: &mut [f32], right: &mut [f32]) {
        if !self.voices[index].playing() {
            return;
        }
        // The pitch and the filter are read at the voice's own knots, so a
        // block of any size plays the same frames.
        if self.voices[index].age.is_multiple_of(KNOT) {
            self.tune_voice(index, frame);
        }
        let mut times = self.times(frame);
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
            // block, at the block's first frame.
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
            for index in 0..MAX_VOICES {
                if self.voices[index].playing() {
                    self.mix(index, frame, left, right);
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
        true
    }

    fn load_sound(&mut self, sound: Arc<Sound>) -> Option<Arc<Sound>> {
        Sampler::load_sound(self, sound)
    }

    fn automate(&mut self, params: &ParamBlock) {
        Sampler::automate(self, params);
    }
}
