mod capture;
mod clock;
mod hold;
mod index;
mod kit;
mod live;
mod player;
mod recording;
mod tracks;

pub use capture::{Captured, Heard, WRAP, record_position};
pub use clock::CLOCK_TICKS;
pub use kit::{DrumKit, KitKey};
pub use live::{AUDITION, INPUT_SLOTS, KEYBOARD, Listen, LiveIn, LiveInput, SOURCES};
pub use player::{MidiHandle, MidiPlayer, midi_player};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordTiming {
    pub start: Ticks,
    pub latency_samples: i64,
    pub looped: Option<(Ticks, Ticks)>,
    pub punch: Option<(Ticks, Ticks)>,
}

use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_core::time::Ticks;
use lumen_devices::{Instrument, ParamBlock};

use crate::mixer::ParamLane;

use hold::TrackState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNote {
    pub start: Ticks,
    pub length: Ticks,
    pub pitch: u8,
    pub velocity: u8,

    pub channel: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiControl {
    pub at: Ticks,
    pub msg: Msg,
}

pub fn note_list(mut notes: Vec<MidiNote>) -> Arc<[MidiNote]> {
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes.into()
}

pub fn control_list(mut controls: Vec<MidiControl>) -> Arc<[MidiControl]> {
    controls.sort_by_key(|c| c.at);
    controls.into()
}

#[derive(Debug, Clone)]
pub struct MidiClip {
    pub id: u64,
    pub start: Ticks,

    pub length: Ticks,

    pub period: Option<Ticks>,
    pub notes: Arc<[MidiNote]>,
    pub controls: Arc<[MidiControl]>,

    pub seams: Arc<[Ticks]>,
}

impl MidiClip {
    fn same_as(&self, other: &MidiClip) -> bool {
        self.id == other.id
            && self.start == other.start
            && self.length == other.length
            && self.period == other.period
            && (Arc::ptr_eq(&self.notes, &other.notes) || self.notes == other.notes)
            && (Arc::ptr_eq(&self.controls, &other.controls) || self.controls == other.controls)
            && self.seams == other.seams
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Instrument,

    Port { port: u16, channel: u8 },
}

pub struct InstrumentSpec {
    pub kind: u32,

    pub preset: u32,

    pub values: Vec<(u32, f32)>,
    pub instrument: Box<dyn Instrument>,
}

pub struct MidiTrack {
    pub id: u64,

    pub clips: Vec<MidiClip>,
    pub instrument: Option<InstrumentSpec>,
    pub target: Target,

    pub outputs: Vec<(u32, u64)>,

    pub listen: Listen,

    pub record: Listen,

    pub record_window: (Ticks, Ticks),

    pub record_timing: Option<RecordTiming>,

    pub automation: Vec<ParamLane>,

    pub values: Vec<(u32, f32)>,

    pub sound: Option<Arc<lumen_devices::sampler::Sound>>,

    params: Option<Box<ParamBlock>>,
    state: Box<TrackState>,

    index: Vec<index::ClipIndex>,

    dirty: bool,

    begun: bool,

    leaving: Option<InstrumentSpec>,

    pub(super) kit: Option<Box<kit::DrumKit>>,
}

impl MidiTrack {
    pub fn new(
        id: u64,
        target: Target,
        instrument: Option<InstrumentSpec>,
        mut clips: Vec<MidiClip>,
    ) -> Self {
        clips.sort_by_key(|c| c.start);
        let index = clips.iter().map(index::ClipIndex::new).collect();
        Self {
            id,
            clips,
            index,
            instrument,
            target,
            outputs: Vec::new(),
            listen: Listen::OFF,
            record: Listen::OFF,
            record_window: hold::ALWAYS,
            record_timing: None,
            automation: Vec::new(),
            values: Vec::new(),
            sound: None,
            params: None,
            state: Box::new(TrackState::new()),
            dirty: true,
            begun: false,
            leaving: None,
            kit: None,
        }
    }

    pub fn playing_drums(mut self, kit: Option<Box<kit::DrumKit>>) -> Self {
        self.kit = kit;
        self
    }

    pub fn with_outputs(mut self, outputs: Vec<(u32, u64)>) -> Self {
        self.outputs = outputs;
        self
    }

    pub fn listening(mut self, listen: Listen) -> Self {
        self.listen = listen;
        self
    }

    pub fn recording(mut self, record: Listen) -> Self {
        self.record = record;
        self
    }

    pub fn recording_within(mut self, window: Option<(Ticks, Ticks)>) -> Self {
        self.record_window = window.unwrap_or(hold::ALWAYS);
        self
    }

    pub fn recording_timing(mut self, timing: Option<RecordTiming>) -> Self {
        self.record_timing = timing;
        self
    }

    pub fn with_automation(mut self, lanes: Vec<ParamLane>) -> Self {
        self.set_automation(lanes);
        self
    }

    pub fn set_automation(&mut self, lanes: Vec<ParamLane>) {
        self.params =
            (!lanes.is_empty() || !self.values.is_empty()).then(|| Box::new(ParamBlock::new()));
        self.automation = lanes;
    }

    pub fn with_values(mut self, values: Vec<(u32, f32)>) -> Self {
        self.set_values(values);
        self
    }

    pub fn set_values(&mut self, values: Vec<(u32, f32)>) {
        self.params = (!values.is_empty() || !self.automation.is_empty())
            .then(|| Box::new(ParamBlock::new()));
        self.values = values;
    }

    pub fn playing_sound(mut self, sound: Option<Arc<lumen_devices::sampler::Sound>>) -> Self {
        self.sound = sound;
        self
    }

    fn channel(&self) -> u8 {
        match self.target {
            Target::Instrument => 0,
            Target::Port { channel, .. } => channel & 15,
        }
    }

    fn same_as(&self, other: &MidiTrack) -> bool {
        self.target == other.target
            && self.outputs == other.outputs
            && self.instrument.as_ref().map(|i| (i.kind, i.preset))
                == other.instrument.as_ref().map(|i| (i.kind, i.preset))
            && self.kit == other.kit
            && self.clips.len() == other.clips.len()
            && self
                .clips
                .iter()
                .zip(&other.clips)
                .all(|(a, b)| a.same_as(b))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiOut {
    At { frame: u64, port: u16, msg: Msg },

    Flush,
}
