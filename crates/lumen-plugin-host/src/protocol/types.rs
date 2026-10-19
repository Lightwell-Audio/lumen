//! The messages and the values they carry, and the limits both sides keep.

use crate::shm::{Layout, Wake};
use crate::wire::WireError;

/// First four bytes of every frame.
pub const MAGIC: u32 = 0x3148_504c; // "LPH1"
/// Bumped when a message changes shape or a limit both sides keep changes. A
/// host of another version is refused (8: a block may carry 64 channels, 32
/// stereo buses, where it was 16).
pub const VERSION: u16 = 8;

pub const MAX_FRAMES: usize = 8192;
/// The channels a block carries each way in all: the main bus and every
/// auxiliary one that is on, 32 stereo buses at most (`MAX_BUSES`).
pub const MAX_CHANNELS: usize = 64;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_QUEUES: usize = 1024;
pub const MAX_POINTS: usize = 16_384;
pub const MAX_PARAMETERS: usize = 16_384;
pub const MAX_STATE: usize = 16 << 20;
pub const MAX_TEXT: usize = 1024;
pub const MAX_CLASSES: usize = 256;
/// The most units and the most edits one answer carries.
pub const MAX_UNITS: usize = 4096;
pub const MAX_EDITS: usize = 4096;
/// The most program lists, the most programs in one and the most in all that
/// one answer carries.
pub const MAX_PROGRAM_LISTS: usize = 64;
pub const MAX_PROGRAMS: usize = 4096;
pub const MAX_PROGRAMS_ALL: usize = 16_384;
/// The most audio buses a plugin may declare each way.
pub const MAX_BUSES: usize = 32;
/// The longest base name of a shared-memory link.
pub const MAX_BASE: usize = 64;
/// The largest frame payload: a full block of audio both ways, with room.
pub const MAX_MESSAGE: usize = MAX_STATE + (MAX_FRAMES * MAX_CHANNELS * 4) + (1 << 20);

/// The most keys Lumen may ask a host to take from its plugin's window.
pub const MAX_KEYS: usize = 64;

/// The restart flags (`IComponentHandler::restartComponent`) that need the
/// plugin set up again: reload the component, its buses changed, its latency
/// changed. The others (parameter values or titles, controller assignments,
/// note expression, key switches) are news for the controller's readers, and a
/// plugin sends them whenever it changes a program or a patch.
pub const RESTART_RECONFIGURE: u32 = 1 | (1 << 1) | (1 << 3);

/// The same 16 bytes as a VST3 class id.
pub type ClassId = [u8; 16];

/// Which auxiliary buses to turn on: bit `i` turns on the plugin's bus `i` of
/// that direction (bit 0, the main bus, is ignored). Every other auxiliary
/// bus is turned off. A block carries their channels after the main bus's, in
/// bus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aux {
    pub inputs: u32,
    pub outputs: u32,
}

/// An audio bus a plugin declares, as the plugin describes it (M6 slice 6b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusInfo {
    pub name: String,
    /// Channels it has by default (a mono or stereo bus is 1 or 2).
    pub channels: u8,
    /// The plugin's main bus (the first of its direction) rather than an
    /// auxiliary one: a sidechain input, an extra output.
    pub main: bool,
    /// The plugin has it on unless told otherwise.
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
    /// A controller (CC) change; `value` is 0 to 1. A plugin hears it through
    /// its own controller mapping.
    Controller {
        channel: i16,
        number: i16,
        value: f32,
    },
    /// Pitch bend, 0 to 1 with 0.5 at the centre.
    PitchBend { channel: i16, value: f32 },
    /// Channel pressure (aftertouch), 0 to 1.
    Pressure { channel: i16, value: f32 },
    /// A program change: program number 0 to 127. A plugin hears it through
    /// its program-change parameter.
    Program { channel: i16, program: i16 },
}

/// The wire tags of event kinds, shared with the shared-memory region.
pub const TAG_NOTE_ON: u8 = 0;
pub const TAG_NOTE_OFF: u8 = 1;
pub const TAG_CONTROLLER: u8 = 2;
pub const TAG_PITCH_BEND: u8 = 3;
pub const TAG_PRESSURE: u8 = 4;
pub const TAG_PROGRAM: u8 = 5;

impl EventKind {
    /// `(tag, channel, number, value)`: the four fields every event is
    /// carried as. `number` is the pitch of a note or a controller's number;
    /// `value` is a velocity or a 0 to 1 value.
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

    /// The kind with these fields, if the tag is known and the value is a
    /// finite number (a plugin never sees NaN).
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
    /// Sample offset into the block.
    pub offset: u32,
    pub kind: EventKind,
}

/// The value changes of one parameter inside a block.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamQueue {
    pub id: u32,
    /// `(sample offset, normalized value)`, in time order.
    pub points: Vec<(u32, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub id: ClassId,
    pub name: String,
    pub category: String,
    pub vendor: String,
    pub subcategories: String,
}

/// Where a plugin's editor window is: its outer top-left corner and the size
/// of its content, in physical pixels at `dpi`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

/// The largest content size an editor may be asked to open at.
pub const MAX_EDITOR_SIDE: u32 = 16_384;

/// What the host knows of a plugin's editor window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorStatus {
    /// The window exists. False after the user closes it.
    pub open: bool,
    /// The window is shown: false while it is hidden (still open).
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
    /// The plugin's own flags (`ParameterInfo::flags`); see the `FLAG_*`
    /// constants for the ones Lumen reads.
    pub flags: u32,
    /// The unit (group) the parameter belongs to; 0 is the root.
    pub unit: i32,
    /// The normalized value now.
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

/// The bits of `ParameterInfo::flags` Lumen reads (the VST3 values).
pub const FLAG_CAN_AUTOMATE: u32 = 1;
pub const FLAG_READ_ONLY: u32 = 1 << 1;
pub const FLAG_HIDDEN: u32 = 1 << 4;
pub const FLAG_PROGRAM_CHANGE: u32 = 1 << 15;
pub const FLAG_BYPASS: u32 = 1 << 16;

/// A group of parameters (VST3 `IUnitInfo`): a name and a parent.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitInfo {
    pub id: i32,
    /// The unit this one sits in; -1 for none.
    pub parent: i32,
    pub name: String,
}

/// A list of programs (patches) the plugin offers (VST3 `IUnitInfo`'s program
/// lists), with the parameter that selects one.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramList {
    pub id: i32,
    pub name: String,
    /// The unit the list belongs to.
    pub unit: i32,
    /// The program-change parameter of that unit: setting it to
    /// `index / (count - 1)` selects program `index`. `None` when the plugin
    /// lists programs but offers no way to select one.
    pub parameter: Option<u32>,
    /// The programs' names, in order.
    pub programs: Vec<String>,
}

/// What happened to a parameter, as the plugin's controller told the host
/// or as the processor reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EditKind {
    /// A gesture began (a control was grabbed).
    Begin = 0,
    /// A new value during a gesture, or alone.
    Perform = 1,
    /// The gesture ended.
    End = 2,
    /// The processor reported a value of its own (a meter, a changed program).
    Output = 3,
    /// The plugin said that something inside it changed that no parameter
    /// shows (a patch, a preset): `setDirty`, or a restart that says the
    /// values changed. The id and the value mean nothing.
    Dirty = 4,
}

impl EditKind {
    pub(super) fn from_u8(v: u8) -> Result<Self, WireError> {
        Ok(match v {
            0 => Self::Begin,
            1 => Self::Perform,
            2 => Self::End,
            3 => Self::Output,
            4 => Self::Dirty,
            _ => return Err(WireError::Invalid("edit kind")),
        })
    }
}

/// What a `TakeEdits` call brings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Taken {
    pub list: Vec<Edit>,
    /// Edits dropped for want of room.
    pub dropped: u32,
    /// Controller, bend and pressure events no parameter took.
    pub ignored: u32,
}

/// One parameter edit, in the order the plugin made it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edit {
    pub id: u32,
    /// Normalized; meaningless for a `Begin` or an `End`.
    pub value: f64,
    pub kind: EditKind,
}

/// How a request failed, so the caller can choose what to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FailureKind {
    /// The module could not be loaded or has no usable class.
    Load = 0,
    /// The request does not fit the host's state (not loaded, not configured).
    Sequence = 1,
    /// The block belongs to an older configuration.
    StaleConfiguration = 2,
    /// The plugin refused or failed the call.
    Plugin = 3,
    /// The request was not understood.
    Protocol = 4,
    /// The shared-memory audio path could not be opened.
    Transport = 5,
    /// The plugin has no editor, or this platform cannot show one.
    NoEditor = 6,
    /// The host's main thread did not answer in time (a plugin's window code
    /// that stopped answering). The request may still be running.
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
    /// Load a module and make one of its audio classes (`None`: the first).
    Load {
        path: String,
        class: Option<ClassId>,
    },
    /// Set the processing format. Starts a new generation.
    Configure {
        generation: u32,
        sample_rate: f64,
        max_frames: u32,
        /// Channels of the main input and output bus; 0 for none.
        inputs: u8,
        outputs: u8,
        /// The auxiliary buses to turn on (see [`Aux`]); every other one is
        /// turned off. Their channels follow the main bus's in the block.
        aux_inputs: u32,
        aux_outputs: u32,
    },
    /// List the audio buses the plugin declares.
    Buses,
    Process {
        generation: u32,
        seq: u64,
        frames: u32,
        /// One slice per input channel, `frames` long.
        input: Vec<Vec<f32>>,
        events: Vec<Event>,
        params: Vec<ParamQueue>,
    },
    GetState,
    SetState(Vec<u8>),
    Parameters,
    /// The groups parameters belong to.
    Units,
    /// The programs the plugin offers.
    Programs,
    /// The edit controller's own state (apart from the component's).
    GetControllerState,
    /// Restore the edit controller's own state.
    SetControllerState(Vec<u8>),
    /// The plugin's own text for a normalized value ("2.4 kHz").
    ParamText {
        id: u32,
        value: f64,
    },
    /// The normalized value the plugin reads from text the user typed
    /// ("2.4 kHz", "-6").
    ParamValue {
        id: u32,
        text: String,
    },
    /// Set a parameter from Lumen: the controller is told, and the processor
    /// hears it in the next block.
    SetParam {
        id: u32,
        value: f64,
    },
    /// Open the plugin's editor window where it was (or at the system's
    /// place when `None`). Answered with `Editor`.
    OpenEditor {
        bounds: Option<EditorBounds>,
    },
    /// Close the editor window, if open. Answered with `Editor`.
    CloseEditor,
    /// Hide the editor window (it stays open: its view is still there and so
    /// is its place) or show it again, without taking the keyboard. Answered
    /// with `Editor`.
    ShowEditor(bool),
    /// Ask whether the editor window is open and where it is.
    EditorStatus,
    /// Take the edits made since the last call (a plugin's own window, a
    /// processor's output parameters), oldest first.
    TakeEdits,
    /// Open the shared-memory audio path Lumen made under this base name.
    /// A second attach replaces the first.
    Attach {
        base: String,
        layout: Layout,
        wake: Wake,
    },
    /// Take these keys (Windows virtual-key codes) from the plugin's window
    /// when pressed without Ctrl, Alt, Shift or the Windows key: the plugin
    /// does not get them and Lumen hears them as [`Response::Key`]. Empty
    /// gives every key back to the plugin.
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
        /// The channels a block carries each way: the main bus's and every
        /// turned-on auxiliary bus's.
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
        /// The plugin's `process` result (0 is success).
        status: i32,
        latency: u32,
        /// Restart flags the plugin asked for during the block.
        restart: u32,
        /// Samples that were not finite and have been replaced by silence.
        repaired: u32,
        output: Vec<Vec<f32>>,
    },
    State(Vec<u8>),
    Parameters {
        /// The plugin's own count, which may exceed the list's cap.
        total: u32,
        list: Vec<ParamInfo>,
    },
    Units(Vec<UnitInfo>),
    Programs(Vec<ProgramList>),
    Text(String),
    /// A normalized value read from the user's text.
    Value(f64),
    /// The editor window as it is now.
    Editor(EditorStatus),
    Edits {
        list: Vec<Edit>,
        /// Edits dropped since the last call because the list was full.
        dropped: u32,
        /// Controller, bend and pressure events no parameter took, since the
        /// last call (the plugin maps none to a parameter).
        ignored: u32,
    },
    Done,
    /// The audio path is open and the host's block loop is running.
    Attached,
    Failed {
        kind: FailureKind,
        message: String,
    },
    /// Not an answer: a key Lumen asked for went down or up in the plugin's
    /// window (repeats are not sent). A key that goes up after the window
    /// lost the keyboard is sent too, so nothing stays held.
    Key {
        code: u8,
        down: bool,
    },
}

impl Response {
    /// Whether this is news sent on the host's own account rather than the
    /// answer to a request.
    pub fn is_notice(&self) -> bool {
        matches!(self, Response::Key { .. })
    }
}
