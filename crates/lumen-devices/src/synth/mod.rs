use lumen_core::midi::{Event, Kind, Msg, cc};

use crate::Instrument;
use crate::hosted::{MOST_FRAMES, ParamBlock};
use crate::params::Glide;

mod det;
pub mod params;
mod voice;

use voice::*;

pub const KIND: u32 = 1;
pub const MOST_VOICES: usize = 16;
const BEND_RANGE: f32 = 2.0;
const MOD_OCTAVES: f32 = 1.5;
pub const GLIDE_SECONDS: f32 = 0.025;
const STEAL_SECONDS: f32 = 0.002;
const SILENT: f32 = 1e-4;
const DEFAULT_BPM: f32 = 120.0;

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

    pub fn name(self) -> &'static str {
        match self {
            Preset::Poly => "Poly",
            Preset::Keys => "Keys",
            Preset::Bass => "Bass",
            Preset::Pluck => "Pluck",
            Preset::Pad => "Pad",
        }
    }

    pub fn number(self) -> u32 {
        self as u32
    }

    pub fn from_number(number: u32) -> Option<Self> {
        Self::ALL.get(number as usize).copied()
    }

    pub fn values(self) -> Vec<(u32, f32)> {
        params::preset_values(self)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Waiting {
    pub(super) channel: u8,
    pub(super) key: u8,
    pub(super) velocity: u8,
    pub(super) glide_oct: f32,
    pub(super) glide: f32,
}

pub struct Synth {
    voices: [Voice; MOST_VOICES],
    targets: [f32; params::COUNT],
    glides: [Glide; params::COUNT],
    curves: Box<[[f32; MOST_FRAMES]; params::COUNT]>,
    slots: Slots,
    events: ParamBlock,
    chunk: ParamBlock,
    sound: Settings,
    preset: Option<Preset>,
    rate: f32,
    tempo: f32,
    voice_limit: usize,
    glide: f32,
    last_key: Option<u8>,
    sustain: bool,
    bend: f32,
    wheel: f32,
    pressure: f32,
    clock: u64,
    frame: u64,
    glide_until: u64,
    dropped: u32,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new(Preset::default())
    }
}

impl Synth {
    pub fn new(preset: Preset) -> Self {
        let mut values = [0.0; params::COUNT];
        params::preset_values_into(&mut values, preset);
        let mut synth = Self::with_array(&values);
        synth.preset = Some(preset);
        synth
    }

    pub fn with_values(values: &[(u32, f32)]) -> Self {
        let mut targets = params::defaults();
        for &(id, value) in values {
            if let Some(slot) = targets.get_mut((id as usize).wrapping_sub(1))
                && params::find(id).is_some()
            {
                *slot = value.clamp(0.0, 1.0);
            }
        }
        Self::with_array(&targets)
    }

    fn with_array(targets: &[f32; params::COUNT]) -> Self {
        let targets = *targets;
        let sound = Settings::from_values(&targets);
        Self {
            voices: [IDLE; MOST_VOICES],
            targets,
            glides: targets.map(Glide::new),
            curves: Box::new([[0.0; MOST_FRAMES]; params::COUNT]),
            slots: Slots::EMPTY,
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

    pub fn preset(&self) -> Option<Preset> {
        self.preset
    }

    pub fn values(&self) -> Vec<(u32, f32)> {
        params::TABLE
            .iter()
            .map(|info| (info.id, self.targets[info.id as usize - 1]))
            .collect()
    }

    pub fn voices(&self) -> usize {
        self.voices
            .iter()
            .take(self.voice_limit)
            .filter(|v| v.stage != Stage::Off)
            .count()
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn tempo(&self) -> f32 {
        self.tempo
    }

    pub fn played_cutoff(&self, frames: usize) -> Vec<f32> {
        (0..frames.min(MOST_FRAMES))
            .map(|at| params::cutoff_hz(self.curves[(params::FILTER_CUTOFF - 1) as usize][at]))
            .collect()
    }

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
                        v.pending = None;
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
                self.bend =
                    det::exp2(f64::from(value) / 8192.0 * f64::from(BEND_RANGE) / 12.0) as f32;
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

    fn point_at(&mut self, values: &[(u32, f32)]) {
        let mut targets = self.targets;
        for &(id, value) in values {
            if let Some(slot) = targets.get_mut((id as usize).wrapping_sub(1))
                && params::find(id).is_some()
            {
                *slot = value.clamp(0.0, 1.0);
            }
        }
        self.pointed(targets);
    }

    fn point_at_all(&mut self, values: &[f32; params::COUNT]) {
        self.pointed(*values);
    }

    fn pointed(&mut self, targets: [f32; params::COUNT]) {
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
            self.silence();
            self.rate = rate as f32;
        }
        let frames = left.len().min(right.len());
        if frames == 0 {
            for e in events {
                self.apply(e.msg);
            }
            return;
        }
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
            let step = if self.glide_until > start {
                voice::GLIDE_FRAMES as usize
            } else {
                voice::DERIVE_FRAMES as usize
            };
            self.slots.fill(
                &Curves::new(&self.curves, start, n),
                start,
                n,
                self.tempo,
                step,
            );
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
                let changing = self.glide_until > start + done as u64;
                let span = Span {
                    rate: self.rate,
                    bend: self.bend,
                    open: self.wheel.max(self.pressure) * MOD_OCTAVES,
                    sound: self.sound,
                    changing,
                    derive: if changing {
                        voice::GLIDE_FRAMES
                    } else {
                        voice::DERIVE_FRAMES
                    },
                    curves: &Curves::new(&self.curves, start, n),
                    slots: &self.slots,
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
        for e in &events[next..] {
            self.apply(e.msg);
        }
    }

    fn preview(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        if rate as f32 != self.rate {
            return;
        }
        let kept = self.voices;
        self.render(rate, &[], left, right);
        self.voices = kept;
    }

    fn active(&self) -> bool {
        self.voices.iter().any(|v| v.stage != Stage::Off)
    }

    fn configure(&mut self, preset: u32) {
        if let Some(preset) = Preset::from_number(preset) {
            let mut values = [0.0; params::COUNT];
            params::preset_values_into(&mut values, preset);
            self.point_at_all(&values);
            self.preset = Some(preset);
        }
    }

    fn configure_values(&mut self, values: &[(u32, f32)]) {
        if values.is_empty() {
            return;
        }
        self.point_at(values);
        self.preset = None;
    }

    fn set_tempo(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            self.tempo = bpm;
        }
    }

    fn automate(&mut self, params: &ParamBlock) {
        self.events.copy_from(params);
    }

    fn silence(&mut self) {
        self.voices = [IDLE; MOST_VOICES];
        self.sustain = false;
    }
}
