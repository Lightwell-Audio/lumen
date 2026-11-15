use crate::shm::{Layout, Wake};
use crate::wire::WireError;

pub const MAGIC: u32 = 0x3148_504c;
pub const VERSION: u16 = 9;

pub const MAX_FRAMES: usize = 8192;
pub const MAX_CHANNELS: usize = 64;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_QUEUES: usize = 1024;
pub const MAX_POINTS: usize = 16_384;
pub const MAX_PARAMETERS: usize = 16_384;
pub const MAX_STATE: usize = 16 << 20;
pub const MAX_TEXT: usize = 1024;
pub const MAX_CLASSES: usize = 256;
pub const MAX_UNITS: usize = 4096;
pub const MAX_EDITS: usize = 4096;
pub const MAX_PROGRAM_LISTS: usize = 64;
pub const MAX_PROGRAMS: usize = 4096;
pub const MAX_PROGRAMS_ALL: usize = 16_384;
pub const MAX_BUSES: usize = 32;
pub const MAX_BASE: usize = 64;
pub const MAX_MESSAGE: usize = MAX_STATE + (MAX_FRAMES * MAX_CHANNELS * 4) + (1 << 20);

pub const MAX_KEYS: usize = 64;

pub const RESTART_RECONFIGURE: u32 = 1 | (1 << 1) | (1 << 3);

pub const RESTART_TITLES: u32 = 1 << 4;

pub type ClassId = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aux {
    pub inputs: u32,
    pub outputs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusInfo {
    pub name: String,
    pub channels: u8,
    pub main: bool,
    pub default_active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    NoteOn {
        channel: i16,
        pitch: i16,
        velocity: f32,
    },
    NoteOff {
        channel: i16,
        pitch: i16,
        velocity: f32,
    },
    Controller {
        channel: i16,
        number: i16,
        value: f32,
    },
    PitchBend {
        channel: i16,
        value: f32,
    },
    Pressure {
        channel: i16,
        value: f32,
    },
    Program {
        channel: i16,
        program: i16,
    },
}

pub const TAG_NOTE_ON: u8 = 0;
pub const TAG_NOTE_OFF: u8 = 1;
pub const TAG_CONTROLLER: u8 = 2;
pub const TAG_PITCH_BEND: u8 = 3;
pub const TAG_PRESSURE: u8 = 4;
pub const TAG_PROGRAM: u8 = 5;

impl EventKind {
    pub fn parts(&self) -> (u8, i16, i16, f32) {
        match *self {
            EventKind::NoteOn {
                channel,
                pitch,
                velocity,
            } => (TAG_NOTE_ON, channel, pitch, velocity),
            EventKind::NoteOff {
                channel,
                pitch,
                velocity,
            } => (TAG_NOTE_OFF, channel, pitch, velocity),
            EventKind::Controller {
                channel,
                number,
                value,
            } => (TAG_CONTROLLER, channel, number, value),
            EventKind::PitchBend { channel, value } => (TAG_PITCH_BEND, channel, 0, value),
            EventKind::Pressure { channel, value } => (TAG_PRESSURE, channel, 0, value),
            EventKind::Program { channel, program } => (TAG_PROGRAM, channel, program, 0.0),
        }
    }

    pub fn from_parts(tag: u8, channel: i16, number: i16, value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        Some(match tag {
            TAG_NOTE_ON => EventKind::NoteOn {
                channel,
                pitch: number,
                velocity: value,
            },
            TAG_NOTE_OFF => EventKind::NoteOff {
                channel,
                pitch: number,
                velocity: value,
            },
            TAG_CONTROLLER => EventKind::Controller {
                channel,
                number,
                value,
            },
            TAG_PITCH_BEND => EventKind::PitchBend { channel, value },
            TAG_PRESSURE => EventKind::Pressure { channel, value },
            TAG_PROGRAM => EventKind::Program {
                channel,
                program: number,
            },
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub offset: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamQueue {
    pub id: u32,
    pub points: Vec<(u32, f64)>,
}

pub trait ParamQueues {
    fn queue_count(&self) -> usize;
    fn queue(&self, i: usize) -> (u32, &[(u32, f64)]);
}

impl ParamQueues for [ParamQueue] {
    fn queue_count(&self) -> usize {
        self.len()
    }

    fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        (self[i].id, &self[i].points)
    }
}

impl<const N: usize> ParamQueues for [ParamQueue; N] {
    fn queue_count(&self) -> usize {
        N
    }

    fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        (self[i].id, &self[i].points)
    }
}

impl ParamQueues for Vec<ParamQueue> {
    fn queue_count(&self) -> usize {
        self.len()
    }

    fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        (self[i].id, &self[i].points)
    }
}

impl ParamQueues for () {
    fn queue_count(&self) -> usize {
        0
    }

    fn queue(&self, _: usize) -> (u32, &[(u32, f64)]) {
        (0, &[])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub id: ClassId,
    pub name: String,
    pub category: String,
    pub vendor: String,
    pub subcategories: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

pub const MAX_EDITOR_SIDE: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorStatus {
    pub open: bool,
    pub visible: bool,
    pub bounds: Option<EditorBounds>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub title: String,
    pub units: String,
    pub steps: i32,
    pub default: f64,
    pub flags: u32,
    pub unit: i32,
    pub value: f64,
}

impl ParamInfo {
    pub fn can_automate(&self) -> bool {
        self.flags & FLAG_CAN_AUTOMATE != 0
    }
    pub fn read_only(&self) -> bool {
        self.flags & FLAG_READ_ONLY != 0
    }
    pub fn hidden(&self) -> bool {
        self.flags & FLAG_HIDDEN != 0
    }
    pub fn program_change(&self) -> bool {
        self.flags & FLAG_PROGRAM_CHANGE != 0
    }
    pub fn bypass(&self) -> bool {
        self.flags & FLAG_BYPASS != 0
    }
}

pub const FLAG_CAN_AUTOMATE: u32 = 1;
pub const FLAG_READ_ONLY: u32 = 1 << 1;
pub const FLAG_HIDDEN: u32 = 1 << 4;
pub const FLAG_PROGRAM_CHANGE: u32 = 1 << 15;
pub const FLAG_BYPASS: u32 = 1 << 16;

#[derive(Debug, Clone, PartialEq)]
pub struct UnitInfo {
    pub id: i32,
    pub parent: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramList {
    pub id: i32,
    pub name: String,
    pub unit: i32,
    pub parameter: Option<u32>,
    pub programs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EditKind {
    Begin = 0,
    Perform = 1,
    End = 2,
    Output = 3,
    Dirty = 4,
    Retitled = 5,
}

impl EditKind {
    pub(super) fn from_u8(v: u8) -> Result<Self, WireError> {
        Ok(match v {
            0 => Self::Begin,
            1 => Self::Perform,
            2 => Self::End,
            3 => Self::Output,
            4 => Self::Dirty,
            5 => Self::Retitled,
            _ => return Err(WireError::Invalid("edit kind")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Taken {
    pub list: Vec<Edit>,
    pub dropped: u32,
    pub ignored: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edit {
    pub id: u32,
    pub value: f64,
    pub kind: EditKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FailureKind {
    Load = 0,
    Sequence = 1,
    StaleConfiguration = 2,
    Plugin = 3,
    Protocol = 4,
    Transport = 5,
    NoEditor = 6,
    Unresponsive = 7,
}

impl FailureKind {
    pub(super) fn from_u8(v: u8) -> Result<Self, WireError> {
        Ok(match v {
            0 => Self::Load,
            1 => Self::Sequence,
            2 => Self::StaleConfiguration,
            3 => Self::Plugin,
            4 => Self::Protocol,
            5 => Self::Transport,
            6 => Self::NoEditor,
            7 => Self::Unresponsive,
            _ => return Err(WireError::Invalid("failure kind")),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Load {
        path: String,
        class: Option<ClassId>,
    },
    Configure {
        generation: u32,
        sample_rate: f64,
        max_frames: u32,
        inputs: u8,
        outputs: u8,
        aux_inputs: u32,
        aux_outputs: u32,
    },
    Buses,
    Process {
        generation: u32,
        seq: u64,
        frames: u32,
        input: Vec<Vec<f32>>,
        events: Vec<Event>,
        params: Vec<ParamQueue>,
    },
    GetState,
    SetState(Vec<u8>),
    Parameters,
    Units,
    Programs,
    GetControllerState,
    SetControllerState(Vec<u8>),
    ParamText {
        id: u32,
        value: f64,
    },
    ParamValue {
        id: u32,
        text: String,
    },
    SetParam {
        id: u32,
        value: f64,
    },
    OpenEditor {
        bounds: Option<EditorBounds>,
    },
    CloseEditor,
    ShowEditor(bool),
    EditorStatus,
    TakeEdits,
    Attach {
        base: String,
        layout: Layout,
        wake: Wake,
    },
    CaptureKeys(Vec<u8>),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Loaded(ClassInfo),
    Configured {
        generation: u32,
        latency: u32,
        tail: u32,
        inputs: u8,
        outputs: u8,
    },
    Buses {
        inputs: Vec<BusInfo>,
        outputs: Vec<BusInfo>,
    },
    Processed {
        generation: u32,
        seq: u64,
        status: i32,
        latency: u32,
        restart: u32,
        repaired: u32,
        output: Vec<Vec<f32>>,
    },
    State(Vec<u8>),
    Parameters {
        total: u32,
        list: Vec<ParamInfo>,
    },
    Units(Vec<UnitInfo>),
    Programs(Vec<ProgramList>),
    Text(String),
    Value(f64),
    Editor(EditorStatus),
    Edits {
        list: Vec<Edit>,
        dropped: u32,
        ignored: u32,
    },
    Done,
    Attached,
    Failed {
        kind: FailureKind,
        message: String,
    },
    Key {
        code: u8,
        down: bool,
    },
}

impl Response {
    pub fn is_notice(&self) -> bool {
        matches!(self, Response::Key { .. })
    }
}
