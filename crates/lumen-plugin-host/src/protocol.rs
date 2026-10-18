//! The messages between Lumen and a plugin-host process, and their framing.
//!
//! Every request gets exactly one response, in order. The one message that is
//! not an answer is [`Response::Key`]: a key pressed in the plugin's window
//! that Lumen asked for ([`Request::CaptureKeys`]), sent when it happens and
//! set apart by the reader. A `Process` request names the
//! configuration generation and a block sequence number, and its response
//! echoes both, so a response that arrives after its deadline (a late
//! plugin) is recognised as stale and dropped rather than written into a
//! block that has moved on. Everything read is bounded (see the limits).

use std::io::{self, Read, Write};

use crate::shm::{Layout, Waiting, Wake};
use crate::wire::{Reader, WireError, Writer};

/// First four bytes of every frame.
pub const MAGIC: u32 = 0x3148_504c; // "LPH1"
/// Bumped when a message changes shape. A host of another version is refused.
pub const VERSION: u16 = 6;

pub const MAX_FRAMES: usize = 8192;
pub const MAX_CHANNELS: usize = 16;
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
    fn from_u8(v: u8) -> Result<Self, WireError> {
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
    fn from_u8(v: u8) -> Result<Self, WireError> {
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
    },
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

fn write_bounds(w: &mut Writer, bounds: Option<&EditorBounds>) {
    match bounds {
        Some(b) => {
            w.bool(true);
            w.i32(b.x);
            w.i32(b.y);
            w.u32(b.width);
            w.u32(b.height);
            w.u32(b.dpi);
        }
        None => w.bool(false),
    }
}

fn read_bounds(r: &mut Reader<'_>) -> Result<Option<EditorBounds>, WireError> {
    if !r.bool()? {
        return Ok(None);
    }
    let bounds = EditorBounds {
        x: r.i32()?,
        y: r.i32()?,
        width: r.u32()?,
        height: r.u32()?,
        dpi: r.u32()?,
    };
    // A size a window could never have is a message that lies.
    if bounds.width > MAX_EDITOR_SIDE || bounds.height > MAX_EDITOR_SIDE || bounds.dpi > 2_000 {
        return Err(WireError::Invalid("editor bounds"));
    }
    Ok(Some(bounds))
}

const REQ_LOAD: u16 = 1;
const REQ_CONFIGURE: u16 = 2;
const REQ_PROCESS: u16 = 3;
const REQ_GET_STATE: u16 = 4;
const REQ_SET_STATE: u16 = 5;
const REQ_PARAMETERS: u16 = 6;
const REQ_QUIT: u16 = 7;
const REQ_ATTACH: u16 = 8;
const REQ_UNITS: u16 = 9;
const REQ_PARAM_TEXT: u16 = 10;
const REQ_SET_PARAM: u16 = 11;
const REQ_TAKE_EDITS: u16 = 12;
const REQ_PARAM_VALUE: u16 = 13;
const REQ_OPEN_EDITOR: u16 = 14;
const REQ_CLOSE_EDITOR: u16 = 15;
const REQ_EDITOR_STATUS: u16 = 16;
const REQ_CAPTURE_KEYS: u16 = 17;
const REQ_SHOW_EDITOR: u16 = 18;
const REQ_PROGRAMS: u16 = 19;
const REQ_GET_CONTROLLER_STATE: u16 = 20;
const REQ_SET_CONTROLLER_STATE: u16 = 21;

const RES_LOADED: u16 = 101;
const RES_CONFIGURED: u16 = 102;
const RES_PROCESSED: u16 = 103;
const RES_STATE: u16 = 104;
const RES_PARAMETERS: u16 = 105;
const RES_DONE: u16 = 106;
const RES_FAILED: u16 = 107;
const RES_ATTACHED: u16 = 108;
const RES_UNITS: u16 = 109;
const RES_TEXT: u16 = 110;
const RES_EDITS: u16 = 111;
const RES_VALUE: u16 = 112;
const RES_EDITOR: u16 = 113;
const RES_KEY: u16 = 114;
const RES_PROGRAMS: u16 = 115;

fn put_audio(w: &mut Writer, channels: &[Vec<f32>]) {
    w.u32(channels.len() as u32);
    for channel in channels {
        w.u32(channel.len() as u32);
        for &s in channel {
            w.f32(s);
        }
    }
}

fn get_audio(r: &mut Reader<'_>, what: &'static str) -> Result<Vec<Vec<f32>>, WireError> {
    let channels = r.count(what, MAX_CHANNELS, 4)?;
    let mut out = Vec::with_capacity(channels);
    for _ in 0..channels {
        let n = r.count("frames", MAX_FRAMES, 4)?;
        let mut channel = Vec::with_capacity(n);
        for _ in 0..n {
            channel.push(r.f32()?);
        }
        out.push(channel);
    }
    // A block is rectangular: every channel is the same length.
    if out.windows(2).any(|w| w[0].len() != w[1].len()) {
        return Err(WireError::Invalid("channel lengths"));
    }
    Ok(out)
}

fn put_class(w: &mut Writer, c: &ClassInfo) {
    w.raw(&c.id);
    w.str(&c.name);
    w.str(&c.category);
    w.str(&c.vendor);
    w.str(&c.subcategories);
}

fn get_class(r: &mut Reader<'_>) -> Result<ClassInfo, WireError> {
    Ok(ClassInfo {
        id: r.raw()?,
        name: r.str("name", MAX_TEXT)?,
        category: r.str("category", MAX_TEXT)?,
        vendor: r.str("vendor", MAX_TEXT)?,
        subcategories: r.str("subcategories", MAX_TEXT)?,
    })
}

impl Request {
    fn encode(&self) -> (u16, Vec<u8>) {
        let mut w = Writer::new();
        let kind = match self {
            Request::Load { path, class } => {
                w.str(path);
                match class {
                    Some(id) => {
                        w.bool(true);
                        w.raw(id);
                    }
                    None => w.bool(false),
                }
                REQ_LOAD
            }
            Request::Configure {
                generation,
                sample_rate,
                max_frames,
                inputs,
                outputs,
            } => {
                w.u32(*generation);
                w.f64(*sample_rate);
                w.u32(*max_frames);
                w.u8(*inputs);
                w.u8(*outputs);
                REQ_CONFIGURE
            }
            Request::Process {
                generation,
                seq,
                frames,
                input,
                events,
                params,
            } => {
                w.u32(*generation);
                w.u64(*seq);
                w.u32(*frames);
                put_audio(&mut w, input);
                w.u32(events.len() as u32);
                for e in events {
                    w.u32(e.offset);
                    let (tag, channel, pitch, velocity) = e.kind.parts();
                    w.u8(tag);
                    w.u16(channel as u16);
                    w.u16(pitch as u16);
                    w.f32(velocity);
                }
                w.u32(params.len() as u32);
                for q in params {
                    w.u32(q.id);
                    w.u32(q.points.len() as u32);
                    for &(offset, value) in &q.points {
                        w.u32(offset);
                        w.f64(value);
                    }
                }
                REQ_PROCESS
            }
            Request::GetState => REQ_GET_STATE,
            Request::SetState(bytes) => {
                w.bytes(bytes);
                REQ_SET_STATE
            }
            Request::Parameters => REQ_PARAMETERS,
            Request::Units => REQ_UNITS,
            Request::Programs => REQ_PROGRAMS,
            Request::GetControllerState => REQ_GET_CONTROLLER_STATE,
            Request::SetControllerState(bytes) => {
                w.bytes(bytes);
                REQ_SET_CONTROLLER_STATE
            }
            Request::ParamText { id, value } => {
                w.u32(*id);
                w.f64(*value);
                REQ_PARAM_TEXT
            }
            Request::ParamValue { id, text } => {
                w.u32(*id);
                w.str(text);
                REQ_PARAM_VALUE
            }
            Request::SetParam { id, value } => {
                w.u32(*id);
                w.f64(*value);
                REQ_SET_PARAM
            }
            Request::OpenEditor { bounds } => {
                write_bounds(&mut w, bounds.as_ref());
                REQ_OPEN_EDITOR
            }
            Request::CloseEditor => REQ_CLOSE_EDITOR,
            Request::ShowEditor(visible) => {
                w.u8(u8::from(*visible));
                REQ_SHOW_EDITOR
            }
            Request::EditorStatus => REQ_EDITOR_STATUS,
            Request::TakeEdits => REQ_TAKE_EDITS,
            Request::Attach { base, layout, wake } => {
                w.str(base);
                w.u32(layout.max_frames);
                w.u8(layout.inputs);
                w.u8(layout.outputs);
                w.u32(layout.max_events);
                w.u32(layout.max_queues);
                w.u32(layout.max_points);
                for waiting in [wake.host, wake.lumen] {
                    let (tag, micros) = waiting.to_parts();
                    w.u8(tag);
                    w.u32(micros);
                }
                REQ_ATTACH
            }
            Request::CaptureKeys(keys) => {
                w.bytes(keys);
                REQ_CAPTURE_KEYS
            }
            Request::Quit => REQ_QUIT,
        };
        (kind, w.into_bytes())
    }

    fn decode(kind: u16, payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        let request = match kind {
            REQ_LOAD => Request::Load {
                path: r.str("path", MAX_TEXT)?,
                class: if r.bool()? { Some(r.raw()?) } else { None },
            },
            REQ_CONFIGURE => {
                let request = Request::Configure {
                    generation: r.u32()?,
                    sample_rate: r.f64()?,
                    max_frames: r.u32()?,
                    inputs: r.u8()?,
                    outputs: r.u8()?,
                };
                if let Request::Configure {
                    sample_rate,
                    max_frames,
                    inputs,
                    outputs,
                    ..
                } = request
                    && (!(sample_rate.is_finite() && (8_000.0..=768_000.0).contains(&sample_rate))
                        || max_frames == 0
                        || max_frames as usize > MAX_FRAMES
                        || inputs as usize > MAX_CHANNELS
                        || outputs as usize > MAX_CHANNELS)
                {
                    return Err(WireError::Invalid("format"));
                }
                request
            }
            REQ_PROCESS => {
                let generation = r.u32()?;
                let seq = r.u64()?;
                let frames = r.u32()?;
                if frames == 0 || frames as usize > MAX_FRAMES {
                    return Err(WireError::Invalid("frame count"));
                }
                let input = get_audio(&mut r, "input channels")?;
                if input.iter().any(|c| c.len() != frames as usize) {
                    return Err(WireError::Invalid("input length"));
                }
                let n = r.count("events", MAX_EVENTS, 13)?;
                let mut events = Vec::with_capacity(n);
                for _ in 0..n {
                    let offset = r.u32()?;
                    if offset >= frames {
                        return Err(WireError::Invalid("event offset"));
                    }
                    let tag = r.u8()?;
                    let channel = r.u16()? as i16;
                    let pitch = r.u16()? as i16;
                    let velocity = r.f32()?;
                    let kind = EventKind::from_parts(tag, channel, pitch, velocity)
                        .ok_or(WireError::Invalid("event kind"))?;
                    events.push(Event { offset, kind });
                }
                let n = r.count("parameter queues", MAX_QUEUES, 8)?;
                let mut params = Vec::with_capacity(n);
                let mut points_total = 0;
                for _ in 0..n {
                    let id = r.u32()?;
                    let m = r.count("points", MAX_POINTS, 12)?;
                    points_total += m;
                    if points_total > MAX_POINTS {
                        return Err(WireError::TooLarge {
                            what: "points in a block",
                            max: MAX_POINTS,
                            got: points_total,
                        });
                    }
                    let mut points = Vec::with_capacity(m);
                    for _ in 0..m {
                        let offset = r.u32()?;
                        if offset >= frames {
                            return Err(WireError::Invalid("point offset"));
                        }
                        points.push((offset, r.f64()?));
                    }
                    params.push(ParamQueue { id, points });
                }
                Request::Process {
                    generation,
                    seq,
                    frames,
                    input,
                    events,
                    params,
                }
            }
            REQ_GET_STATE => Request::GetState,
            REQ_SET_STATE => Request::SetState(r.bytes("state", MAX_STATE)?.to_vec()),
            REQ_PARAMETERS => Request::Parameters,
            REQ_UNITS => Request::Units,
            REQ_PROGRAMS => Request::Programs,
            REQ_GET_CONTROLLER_STATE => Request::GetControllerState,
            REQ_SET_CONTROLLER_STATE => {
                Request::SetControllerState(r.bytes("controller state", MAX_STATE)?.to_vec())
            }
            REQ_PARAM_TEXT => Request::ParamText {
                id: r.u32()?,
                value: r.f64()?,
            },
            REQ_PARAM_VALUE => Request::ParamValue {
                id: r.u32()?,
                text: r.str("typed value", MAX_TEXT)?,
            },
            REQ_SET_PARAM => Request::SetParam {
                id: r.u32()?,
                value: r.f64()?,
            },
            REQ_OPEN_EDITOR => Request::OpenEditor {
                bounds: read_bounds(&mut r)?,
            },
            REQ_CLOSE_EDITOR => Request::CloseEditor,
            REQ_SHOW_EDITOR => Request::ShowEditor(r.bool()?),
            REQ_EDITOR_STATUS => Request::EditorStatus,
            REQ_TAKE_EDITS => Request::TakeEdits,
            REQ_ATTACH => {
                let base = r.str("base name", MAX_BASE)?;
                if base.is_empty() || !base.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(WireError::Invalid("base name"));
                }
                let layout = Layout {
                    max_frames: r.u32()?,
                    inputs: r.u8()?,
                    outputs: r.u8()?,
                    max_events: r.u32()?,
                    max_queues: r.u32()?,
                    max_points: r.u32()?,
                };
                layout
                    .check()
                    .map_err(|_| WireError::Invalid("shared layout"))?;
                let mut waiting = || -> Result<Waiting, WireError> {
                    let tag = r.u8()?;
                    let micros = r.u32()?;
                    Waiting::from_parts(tag, micros).ok_or(WireError::Invalid("wake mode"))
                };
                let host = waiting()?;
                let lumen = waiting()?;
                Request::Attach {
                    base,
                    layout,
                    wake: Wake { host, lumen },
                }
            }
            REQ_CAPTURE_KEYS => Request::CaptureKeys(r.bytes("keys", MAX_KEYS)?.to_vec()),
            REQ_QUIT => Request::Quit,
            _ => return Err(WireError::Invalid("request kind")),
        };
        r.finish()?;
        Ok(request)
    }
}

impl Response {
    fn encode(&self) -> (u16, Vec<u8>) {
        let mut w = Writer::new();
        let kind = match self {
            Response::Loaded(class) => {
                put_class(&mut w, class);
                RES_LOADED
            }
            Response::Configured {
                generation,
                latency,
                tail,
            } => {
                w.u32(*generation);
                w.u32(*latency);
                w.u32(*tail);
                RES_CONFIGURED
            }
            Response::Processed {
                generation,
                seq,
                status,
                latency,
                restart,
                repaired,
                output,
            } => {
                w.u32(*generation);
                w.u64(*seq);
                w.i32(*status);
                w.u32(*latency);
                w.u32(*restart);
                w.u32(*repaired);
                put_audio(&mut w, output);
                RES_PROCESSED
            }
            Response::State(bytes) => {
                w.bytes(bytes);
                RES_STATE
            }
            Response::Parameters { total, list } => {
                w.u32(*total);
                w.u32(list.len() as u32);
                for p in list {
                    w.u32(p.id);
                    w.str(&p.title);
                    w.str(&p.units);
                    w.i32(p.steps);
                    w.f64(p.default);
                    w.u32(p.flags);
                    w.i32(p.unit);
                    w.f64(p.value);
                }
                RES_PARAMETERS
            }
            Response::Units(list) => {
                w.u32(list.len() as u32);
                for u in list {
                    w.i32(u.id);
                    w.i32(u.parent);
                    w.str(&u.name);
                }
                RES_UNITS
            }
            Response::Programs(lists) => {
                w.u32(lists.len() as u32);
                for l in lists {
                    w.i32(l.id);
                    w.str(&l.name);
                    w.i32(l.unit);
                    w.bool(l.parameter.is_some());
                    w.u32(l.parameter.unwrap_or(0));
                    w.u32(l.programs.len() as u32);
                    for name in &l.programs {
                        w.str(name);
                    }
                }
                RES_PROGRAMS
            }
            Response::Text(text) => {
                w.str(text);
                RES_TEXT
            }
            Response::Value(value) => {
                w.f64(*value);
                RES_VALUE
            }
            Response::Editor(status) => {
                w.bool(status.open);
                w.bool(status.visible);
                write_bounds(&mut w, status.bounds.as_ref());
                RES_EDITOR
            }
            Response::Edits {
                list,
                dropped,
                ignored,
            } => {
                w.u32(*dropped);
                w.u32(*ignored);
                w.u32(list.len() as u32);
                for e in list {
                    w.u32(e.id);
                    w.f64(e.value);
                    w.u8(e.kind as u8);
                }
                RES_EDITS
            }
            Response::Done => RES_DONE,
            Response::Attached => RES_ATTACHED,
            Response::Failed { kind, message } => {
                w.u8(*kind as u8);
                w.str(message);
                RES_FAILED
            }
            Response::Key { code, down } => {
                w.u8(*code);
                w.bool(*down);
                RES_KEY
            }
        };
        (kind, w.into_bytes())
    }

    fn decode(kind: u16, payload: &[u8]) -> Result<Self, WireError> {
        let mut r = Reader::new(payload);
        let response = match kind {
            RES_LOADED => Response::Loaded(get_class(&mut r)?),
            RES_CONFIGURED => Response::Configured {
                generation: r.u32()?,
                latency: r.u32()?,
                tail: r.u32()?,
            },
            RES_PROCESSED => Response::Processed {
                generation: r.u32()?,
                seq: r.u64()?,
                status: r.i32()?,
                latency: r.u32()?,
                restart: r.u32()?,
                repaired: r.u32()?,
                output: get_audio(&mut r, "output channels")?,
            },
            RES_STATE => Response::State(r.bytes("state", MAX_STATE)?.to_vec()),
            RES_PARAMETERS => {
                let total = r.u32()?;
                let n = r.count("parameters", MAX_PARAMETERS, 40)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(ParamInfo {
                        id: r.u32()?,
                        title: r.str("title", MAX_TEXT)?,
                        units: r.str("units", MAX_TEXT)?,
                        steps: r.i32()?,
                        default: r.f64()?,
                        flags: r.u32()?,
                        unit: r.i32()?,
                        value: r.f64()?,
                    });
                }
                Response::Parameters { total, list }
            }
            RES_UNITS => {
                let n = r.count("units", MAX_UNITS, 12)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(UnitInfo {
                        id: r.i32()?,
                        parent: r.i32()?,
                        name: r.str("unit name", MAX_TEXT)?,
                    });
                }
                Response::Units(list)
            }
            RES_PROGRAMS => {
                let n = r.count("program lists", MAX_PROGRAM_LISTS, 21)?;
                let mut all = 0;
                let mut lists = Vec::with_capacity(n);
                for _ in 0..n {
                    let id = r.i32()?;
                    let name = r.str("list name", MAX_TEXT)?;
                    let unit = r.i32()?;
                    let has = r.bool()?;
                    let parameter = r.u32()?;
                    let count = r.count("programs", MAX_PROGRAMS, 4)?;
                    all += count;
                    if all > MAX_PROGRAMS_ALL {
                        return Err(WireError::TooLarge {
                            what: "programs in all",
                            max: MAX_PROGRAMS_ALL,
                            got: all,
                        });
                    }
                    let mut programs = Vec::with_capacity(count);
                    for _ in 0..count {
                        programs.push(r.str("program name", MAX_TEXT)?);
                    }
                    lists.push(ProgramList {
                        id,
                        name,
                        unit,
                        parameter: has.then_some(parameter),
                        programs,
                    });
                }
                Response::Programs(lists)
            }
            RES_TEXT => Response::Text(r.str("text", MAX_TEXT)?),
            RES_VALUE => Response::Value(r.f64()?),
            RES_EDITOR => Response::Editor(EditorStatus {
                open: r.bool()?,
                visible: r.bool()?,
                bounds: read_bounds(&mut r)?,
            }),
            RES_EDITS => {
                let dropped = r.u32()?;
                let ignored = r.u32()?;
                let n = r.count("edits", MAX_EDITS, 13)?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(Edit {
                        id: r.u32()?,
                        value: r.f64()?,
                        kind: EditKind::from_u8(r.u8()?)?,
                    });
                }
                Response::Edits {
                    list,
                    dropped,
                    ignored,
                }
            }
            RES_DONE => Response::Done,
            RES_ATTACHED => Response::Attached,
            RES_FAILED => Response::Failed {
                kind: FailureKind::from_u8(r.u8()?)?,
                message: r.str("message", MAX_TEXT)?,
            },
            RES_KEY => Response::Key {
                code: r.u8()?,
                down: r.bool()?,
            },
            _ => return Err(WireError::Invalid("response kind")),
        };
        r.finish()?;
        Ok(response)
    }
}

/// Why reading a frame failed.
#[derive(Debug)]
pub enum FrameError {
    /// The other side closed the stream between frames.
    Closed,
    Io(io::Error),
    Wire(WireError),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Closed => write!(f, "stream closed"),
            FrameError::Io(e) => write!(f, "{e}"),
            FrameError::Wire(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<WireError> for FrameError {
    fn from(e: WireError) -> Self {
        FrameError::Wire(e)
    }
}

fn write_frame<W: Write>(out: &mut W, kind: u16, payload: &[u8]) -> io::Result<()> {
    let mut header = [0_u8; 12];
    header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    header[4..6].copy_from_slice(&VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&kind.to_le_bytes());
    header[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out.write_all(&header)?;
    out.write_all(payload)?;
    out.flush()
}

fn read_frame<R: Read>(input: &mut R) -> Result<(u16, Vec<u8>), FrameError> {
    let mut header = [0_u8; 12];
    // End of stream before the first byte is an orderly close; partway
    // through a header it is a torn frame.
    let mut got = 0;
    while got < header.len() {
        match input.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Wire(WireError::Truncated)),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    let magic = u32::from_le_bytes(header[0..4].try_into().expect("four bytes"));
    let version = u16::from_le_bytes(header[4..6].try_into().expect("two bytes"));
    let kind = u16::from_le_bytes(header[6..8].try_into().expect("two bytes"));
    let len = u32::from_le_bytes(header[8..12].try_into().expect("four bytes")) as usize;
    if magic != MAGIC {
        return Err(WireError::Invalid("frame marker").into());
    }
    if version != VERSION {
        return Err(WireError::Invalid("protocol version").into());
    }
    if len > MAX_MESSAGE {
        return Err(WireError::TooLarge {
            what: "message",
            max: MAX_MESSAGE,
            got: len,
        }
        .into());
    }
    let mut payload = vec![0; len];
    input.read_exact(&mut payload).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Wire(WireError::Truncated)
        } else {
            FrameError::Io(e)
        }
    })?;
    Ok((kind, payload))
}

pub fn write_request<W: Write>(out: &mut W, request: &Request) -> io::Result<()> {
    let (kind, payload) = request.encode();
    write_frame(out, kind, &payload)
}

pub fn read_request<R: Read>(input: &mut R) -> Result<Request, FrameError> {
    let (kind, payload) = read_frame(input)?;
    Ok(Request::decode(kind, &payload)?)
}

pub fn write_response<W: Write>(out: &mut W, response: &Response) -> io::Result<()> {
    let (kind, payload) = response.encode();
    write_frame(out, kind, &payload)
}

pub fn read_response<R: Read>(input: &mut R) -> Result<Response, FrameError> {
    let (kind, payload) = read_frame(input)?;
    Ok(Response::decode(kind, &payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(offset: u32, pitch: i16) -> Event {
        Event {
            offset,
            kind: EventKind::NoteOn {
                channel: 0,
                pitch,
                velocity: 0.75,
            },
        }
    }

    fn round_trip(request: &Request) -> Request {
        let mut bytes = Vec::new();
        write_request(&mut bytes, request).unwrap();
        read_request(&mut bytes.as_slice()).unwrap()
    }

    #[test]
    fn requests_round_trip() {
        let process = Request::Process {
            generation: 3,
            seq: 99,
            frames: 4,
            input: vec![vec![0.0, 0.5, -0.5, 1.0]; 2],
            events: vec![
                note(1, 60),
                Event {
                    offset: 3,
                    kind: EventKind::NoteOff {
                        channel: 2,
                        pitch: -1,
                        velocity: 0.0,
                    },
                },
                Event {
                    offset: 2,
                    kind: EventKind::Controller {
                        channel: 1,
                        number: 64,
                        value: 1.0,
                    },
                },
                Event {
                    offset: 2,
                    kind: EventKind::PitchBend {
                        channel: 0,
                        value: 0.5,
                    },
                },
                Event {
                    offset: 3,
                    kind: EventKind::Pressure {
                        channel: 15,
                        value: 0.25,
                    },
                },
            ],
            params: vec![ParamQueue {
                id: 7,
                points: vec![(0, 0.25), (2, 1.0)],
            }],
        };
        for request in [
            Request::Load {
                path: "/x/y.vst3".into(),
                class: Some([9; 16]),
            },
            Request::Load {
                path: "z".into(),
                class: None,
            },
            Request::Configure {
                generation: 1,
                sample_rate: 48_000.0,
                max_frames: 512,
                inputs: 2,
                outputs: 2,
            },
            process,
            Request::GetState,
            Request::SetState(vec![1, 2, 3]),
            Request::Parameters,
            Request::Units,
            Request::Programs,
            Request::GetControllerState,
            Request::SetControllerState(vec![3; 12]),
            Request::ParamText { id: 3, value: 0.5 },
            Request::ParamValue {
                id: 3,
                text: "2.4 kHz".into(),
            },
            Request::OpenEditor { bounds: None },
            Request::OpenEditor {
                bounds: Some(EditorBounds {
                    x: -1200,
                    y: 80,
                    width: 640,
                    height: 480,
                    dpi: 144,
                }),
            },
            Request::CloseEditor,
            Request::ShowEditor(true),
            Request::ShowEditor(false),
            Request::EditorStatus,
            Request::SetParam { id: 3, value: 1.0 },
            Request::TakeEdits,
            Request::Attach {
                base: "1f2-3".into(),
                layout: Layout::new(512, 2, 2),
                wake: Wake {
                    host: Waiting::Hybrid(std::time::Duration::from_micros(150)),
                    lumen: Waiting::Event,
                },
            },
            Request::Attach {
                base: "a".into(),
                layout: Layout::new(8192, 0, 16),
                wake: Wake::SPIN,
            },
            Request::CaptureKeys(vec![b'A', b'W', b'Z']),
            Request::CaptureKeys(Vec::new()),
            Request::Quit,
        ] {
            assert_eq!(round_trip(&request), request);
        }
    }

    #[test]
    fn responses_round_trip() {
        for response in [
            Response::Loaded(ClassInfo {
                id: [1; 16],
                name: "n".into(),
                category: "c".into(),
                vendor: "v".into(),
                subcategories: "s".into(),
            }),
            Response::Configured {
                generation: 2,
                latency: 24,
                tail: 24,
            },
            Response::Processed {
                generation: 2,
                seq: 5,
                status: 0,
                latency: 24,
                restart: 0,
                repaired: 3,
                output: vec![vec![0.25; 8]; 2],
            },
            Response::State(vec![5; 40]),
            Response::Parameters {
                total: 99,
                list: vec![ParamInfo {
                    id: 4,
                    title: "Level".into(),
                    units: "dB".into(),
                    steps: 0,
                    default: 0.5,
                    flags: FLAG_CAN_AUTOMATE | FLAG_HIDDEN,
                    unit: 2,
                    value: 0.25,
                }],
            },
            Response::Units(vec![
                UnitInfo {
                    id: 0,
                    parent: -1,
                    name: "Root".into(),
                },
                UnitInfo {
                    id: 2,
                    parent: 0,
                    name: "Filter".into(),
                },
            ]),
            Response::Programs(vec![
                ProgramList {
                    id: 3,
                    name: "Factory".into(),
                    unit: 0,
                    parameter: Some(91),
                    programs: vec!["Warm pad".into(), "Glass bells".into()],
                },
                ProgramList {
                    id: 4,
                    name: "Empty".into(),
                    unit: 1,
                    parameter: None,
                    programs: Vec::new(),
                },
            ]),
            Response::Text("2.4 kHz".into()),
            Response::Value(0.25),
            Response::Editor(EditorStatus::default()),
            Response::Editor(EditorStatus {
                open: true,
                visible: false,
                bounds: Some(EditorBounds {
                    x: 10,
                    y: 20,
                    width: 320,
                    height: 200,
                    dpi: 96,
                }),
            }),
            Response::Edits {
                list: vec![
                    Edit {
                        id: 1,
                        value: 0.0,
                        kind: EditKind::Begin,
                    },
                    Edit {
                        id: 1,
                        value: 0.7,
                        kind: EditKind::Perform,
                    },
                    Edit {
                        id: 1,
                        value: 0.0,
                        kind: EditKind::End,
                    },
                    Edit {
                        id: 9,
                        value: 0.1,
                        kind: EditKind::Output,
                    },
                ],
                dropped: 2,
                ignored: 7,
            },
            Response::Key {
                code: b'K',
                down: true,
            },
            Response::Done,
            Response::Attached,
            Response::Attached,
            Response::Failed {
                kind: FailureKind::StaleConfiguration,
                message: "old".into(),
            },
        ] {
            let mut bytes = Vec::new();
            write_response(&mut bytes, &response).unwrap();
            assert_eq!(read_response(&mut bytes.as_slice()).unwrap(), response);
        }
    }

    fn frame(kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, kind, payload).unwrap();
        bytes
    }

    fn rejects(bytes: &[u8]) -> bool {
        matches!(read_request(&mut &*bytes), Err(FrameError::Wire(_)))
    }

    #[test]
    fn editor_bounds_no_window_could_have_are_refused() {
        let at = |width, height, dpi| Request::OpenEditor {
            bounds: Some(EditorBounds {
                x: 0,
                y: 0,
                width,
                height,
                dpi,
            }),
        };
        assert!(!rejects(&frame(
            REQ_OPEN_EDITOR,
            &at(800, 600, 96).encode().1
        )));
        for request in [
            at(MAX_EDITOR_SIDE + 1, 600, 96),
            at(800, MAX_EDITOR_SIDE + 1, 96),
            at(800, 600, 50_000),
        ] {
            assert!(rejects(&frame(REQ_OPEN_EDITOR, &request.encode().1)));
        }
    }

    #[test]
    fn too_many_keys_to_capture_are_refused() {
        let mut w = crate::wire::Writer::new();
        w.bytes(&[b'A'; MAX_KEYS + 1]);
        assert!(rejects(&frame(REQ_CAPTURE_KEYS, &w.into_bytes())));
    }

    #[test]
    fn only_a_reload_a_bus_change_or_a_new_latency_sets_a_plugin_up_again() {
        // The VST3 restart flags.
        let (reload, io, values, latency, titles, cc) = (1, 1 << 1, 1 << 2, 1 << 3, 1 << 4, 1 << 5);
        for flag in [reload, io, latency] {
            assert_ne!(RESTART_RECONFIGURE & flag, 0, "{flag}");
        }
        // What a plugin says when it loads a patch.
        for flag in [values, titles, cc] {
            assert_eq!(RESTART_RECONFIGURE & flag, 0, "{flag}");
        }
    }

    #[test]
    fn only_a_key_is_news_rather_than_an_answer() {
        assert!(
            Response::Key {
                code: 65,
                down: false
            }
            .is_notice()
        );
        assert!(!Response::Done.is_notice());
    }

    #[test]
    fn a_wrong_marker_or_version_is_refused() {
        let mut bytes = frame(REQ_QUIT, &[]);
        bytes[0] ^= 1;
        assert!(rejects(&bytes));
        let mut bytes = frame(REQ_QUIT, &[]);
        bytes[4] = 9;
        assert!(rejects(&bytes));
    }

    #[test]
    fn an_absurd_length_is_refused_without_reading_it() {
        let mut bytes = frame(REQ_QUIT, &[]);
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(rejects(&bytes));
    }

    #[test]
    fn a_torn_frame_is_not_a_clean_close() {
        assert!(matches!(
            read_request(&mut &[][..]),
            Err(FrameError::Closed)
        ));
        let bytes = frame(REQ_GET_STATE, &[]);
        assert!(rejects(&bytes[..5]));
        let bytes = frame(REQ_SET_STATE, &[4, 0, 0, 0, 1, 2, 3, 4]);
        assert!(rejects(&bytes[..bytes.len() - 2]));
    }

    #[test]
    fn malformed_counts_in_a_process_request_are_refused() {
        let good = Request::Process {
            generation: 1,
            seq: 1,
            frames: 4,
            input: vec![],
            events: vec![note(1, 60)],
            params: vec![],
        };
        let mut payload = good.encode().1;
        // generation 4 + seq 8 + frames 4 + channels 4 = 20: the event count.
        payload[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(rejects(&frame(REQ_PROCESS, &payload)));
        // An event past the end of the block.
        let late = Request::Process {
            generation: 1,
            seq: 1,
            frames: 4,
            input: vec![],
            events: vec![note(4, 60)],
            params: vec![],
        };
        assert!(rejects(&frame(REQ_PROCESS, &late.encode().1)));
        // Input shorter than the block.
        let short = Request::Process {
            generation: 1,
            seq: 1,
            frames: 4,
            input: vec![vec![0.0; 3]],
            events: vec![],
            params: vec![],
        };
        assert!(rejects(&frame(REQ_PROCESS, &short.encode().1)));
        // Zero frames.
        let none = Request::Process {
            generation: 1,
            seq: 1,
            frames: 0,
            input: vec![],
            events: vec![],
            params: vec![],
        };
        assert!(rejects(&frame(REQ_PROCESS, &none.encode().1)));
    }

    #[test]
    fn too_many_points_across_queues_are_refused() {
        let queues = (0..3)
            .map(|id| ParamQueue {
                id,
                points: vec![(0, 0.0); MAX_POINTS / 2],
            })
            .collect();
        let request = Request::Process {
            generation: 1,
            seq: 1,
            frames: 4,
            input: vec![],
            events: vec![],
            params: queues,
        };
        assert!(rejects(&frame(REQ_PROCESS, &request.encode().1)));
    }

    #[test]
    fn a_bad_format_is_refused() {
        for (rate, frames, inputs) in [
            (f64::NAN, 512, 2),
            (1.0, 512, 2),
            (48_000.0, 0, 2),
            (48_000.0, 100_000, 2),
            (48_000.0, 512, 200),
        ] {
            let request = Request::Configure {
                generation: 1,
                sample_rate: rate,
                max_frames: frames,
                inputs,
                outputs: 2,
            };
            assert!(rejects(&frame(REQ_CONFIGURE, &request.encode().1)));
        }
    }

    #[test]
    fn a_bad_attach_is_refused() {
        let attach = |base: &str, layout: Layout, wake: Wake| Request::Attach {
            base: base.into(),
            layout,
            wake,
        };
        let good = Layout::new(512, 2, 2);
        // Names go into the operating system's namespace: letters, digits
        // and dashes only, so a hostile base cannot reach another object.
        for base in ["", "a/b", "..", "a b", "a\\b", "x\0y"] {
            let request = attach(base, good, Wake::EVENT);
            assert!(rejects(&frame(REQ_ATTACH, &request.encode().1)), "{base:?}");
        }
        let too_long = "a".repeat(MAX_BASE + 1);
        assert!(rejects(&frame(
            REQ_ATTACH,
            &attach(&too_long, good, Wake::EVENT).encode().1
        )));
        for layout in [
            Layout::new(0, 2, 2),
            Layout::new(100_000, 2, 2),
            Layout::new(512, 200, 2),
            Layout {
                max_events: u32::MAX,
                ..good
            },
        ] {
            let request = attach("ok", layout, Wake::EVENT);
            assert!(rejects(&frame(REQ_ATTACH, &request.encode().1)));
        }
        // An unknown wake mode.
        let mut payload = attach("ok", good, Wake::EVENT).encode().1;
        let at = payload.len() - 10; // host tag
        payload[at] = 9;
        assert!(rejects(&frame(REQ_ATTACH, &payload)));
    }

    #[test]
    fn trailing_bytes_and_unknown_kinds_are_refused() {
        let mut payload = Request::GetState.encode().1;
        payload.push(0);
        assert!(rejects(&frame(REQ_GET_STATE, &payload)));
        assert!(rejects(&frame(999, &[])));
    }
}
