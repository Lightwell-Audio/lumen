//! The project document model, as stored in `.lumen` files.
//!
//! Minimal in M1: enough structure to prove the format at scale (tracks,
//! groups, clips, notes, media, opaque plugin state). It grows feature by
//! feature; every change to what is persisted bumps the format version and
//! adds a migration (see `migrate`).
//!
//! Tracks and clips are held in [`Arc`]s so that undo snapshots share
//! everything an edit did not touch (ADR-0024). Edit through
//! [`Arc::make_mut`]; serde writes the `Arc`s transparently.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use lumen_core::fade::Fade;
use lumen_core::grid::Grid;
use lumen_core::pan::PanLaw;
use lumen_core::time::{TempoPoint, Ticks};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Stable identity of an object within a project. Written as 16 hex digits
/// so tools that read numbers as doubles (JavaScript) cannot corrupt it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub u64);

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:016x}", self.0))
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        u64::from_str_radix(&text, 16)
            .map(Id)
            .map_err(|_| serde::de::Error::custom(format!("invalid id {text:?}")))
    }
}

/// `#RRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:06X}", self.0 & 0xFF_FFFF))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .map(Color)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid colour {text:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: Vec<TempoPoint>,
    pub time_signature: TimeSignature,
    /// The arrangement's tracks, in order. Never holds the master.
    pub tracks: Vec<Arc<Track>>,
    /// Where every track ends up (format v7): one per project, kept apart
    /// from `tracks` so the arrangement chooses where to show it.
    pub master: Arc<Track>,
    /// How pan positions become left and right gains (format v7).
    #[serde(default)]
    pub pan_law: PanLaw,
    #[serde(default)]
    pub media: Vec<Media>,
    /// Named locators, in time order (format v4). Part of undo.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// The loop and punch (format v2; part of undo from v4).
    #[serde(default)]
    pub transport: TransportState,
    /// How the project was last viewed (format v2). Not part of undo.
    #[serde(default)]
    pub view: ViewState,
}

/// A named locator (format v4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    pub at: Ticks,
    pub name: String,
}

/// The loop brace and the punch switches. Part of undo from format v4
/// (M3 slice 5c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportState {
    pub loop_start: Ticks,
    pub loop_end: Ticks,
    pub loop_enabled: bool,
    /// Punch in and out (format v4) at the loop brace's start and end:
    /// recording starts and stops there.
    pub punch_in: bool,
    pub punch_out: bool,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            loop_start: Ticks::ZERO,
            // Four bars of 4/4.
            loop_end: Ticks::quarters(16),
            loop_enabled: false,
            punch_in: false,
            punch_out: false,
        }
    }
}

/// Scroll, zoom, panels and track heights, restored on open (not undo
/// steps: resizing a track is not an edit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    /// First visible quarter note.
    pub left_beat: f32,
    /// Horizontal zoom.
    pub px_per_beat: f32,
    pub browser_open: bool,
    pub detail_open: bool,
    pub browser_size: f32,
    pub detail_size: f32,
    /// Vertical scroll of the arrangement, in points (format v3).
    pub top: f32,
    /// Unfolded heights of tracks resized from the default (format v3).
    pub track_heights: BTreeMap<Id, f32>,
    /// Tracks shown unfolded, with their detailed header (format v3).
    pub expanded: BTreeSet<Id>,
    /// The editing grid and snapping (format v4).
    pub grid: Grid,
    /// Groups folded to hide their tracks (format v8).
    pub collapsed: BTreeSet<Id>,
    /// Width of the track header column, in points (format v9).
    pub header_width: f32,
    /// Height of the area pinned at the bottom (returns and the master),
    /// in points, when dragged from its natural size (format v10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns_height: Option<f32>,
    /// Returns folded away in that area, leaving the master (format v10).
    pub returns_folded: bool,
    /// Device cards folded to a strip in the Devices tab (format v18).
    /// Written only when there are some.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub folded_devices: BTreeSet<Id>,
    /// The parameters on a device card's strip, by device (format v19):
    /// the plugin's parameter ids in the order shown. A device with none
    /// chosen shows the first few the plugin lists. Written only when some
    /// device has a choice.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shown_params: BTreeMap<Id, Vec<u32>>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            left_beat: 0.0,
            px_per_beat: 24.0,
            browser_open: true,
            detail_open: true,
            browser_size: 220.0,
            detail_size: 220.0,
            top: 0.0,
            track_heights: BTreeMap::new(),
            expanded: BTreeSet::new(),
            grid: Grid::default(),
            collapsed: BTreeSet::new(),
            header_width: 204.0,
            returns_height: None,
            returns_folded: false,
            folded_devices: BTreeSet::new(),
            shown_params: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Midi,
    Group,
    Return,
    Master,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub kind: TrackKind,
    pub color: Color,
    /// Group track this track belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    pub gain_db: f32,
    pub pan: f32,
    // Format v7 (M4 slice 2). Written only when set.
    /// Where its audio goes.
    #[serde(default, skip_serializing_if = "Output::is_auto")]
    pub output: Output,
    /// Sends to return tracks. A return with no send here gets nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<Send>,
    /// Polarity inverted.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// Input trim in dB, before everything else on the track.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_db: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    /// Record-armed (format v3; recording itself arrives in M9).
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub devices: Vec<Device>,
    /// Where a MIDI track plays its notes instead of its instrument (format
    /// v12): a MIDI output port and channel. Written only when set; the
    /// instrument stays on the track for switching back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_to: Option<MidiTo>,
    /// Which MIDI inputs a MIDI track listens to (format v13). Written only
    /// when not "all inputs, all channels".
    #[serde(default, skip_serializing_if = "MidiFrom::is_default")]
    pub midi_from: MidiFrom,
    /// When the track listens to its input (format v13). Stored on every
    /// kind of track; audio tracks use it from M9.
    #[serde(default, skip_serializing_if = "Monitor::is_default")]
    pub monitor: Monitor,
    /// The plugin output this audio track plays, if it plays one (format
    /// v21): an auxiliary output of an instrument, which Lumen made the track
    /// for. Written only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_output: Option<PluginOutput>,
    #[serde(default)]
    pub clips: Vec<Arc<Clip>>,
}

/// An output of a plugin instrument that a track plays (format v21): the
/// device, and the number of its audio output bus (0 is the instrument's own
/// track, so an output track is 1 or more).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOutput {
    pub device: Id,
    pub bus: u32,
}

impl Track {
    /// An empty track at unity, centred, routed automatically.
    pub fn new(id: Id, name: impl Into<String>, kind: TrackKind, color: Color) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            color,
            parent: None,
            gain_db: 0.0,
            pan: 0.0,
            output: Output::Auto,
            sends: Vec::new(),
            invert: false,
            trim_db: 0.0,
            mute: false,
            solo: false,
            armed: false,
            devices: Vec::new(),
            midi_to: None,
            midi_from: MidiFrom::default(),
            monitor: Monitor::default(),
            plugin_output: None,
            clips: Vec::new(),
        }
    }

    /// The track's built-in synth: its first one (format v12). The engine's
    /// own instrument; a VST3 instrument is found by [`Track::sound_source`].
    pub fn instrument(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.is_synth())
    }

    /// What makes the sound on a MIDI track: its first instrument of any
    /// kind, the built-in synth or a VST3 plugin (format v17).
    pub fn sound_source(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.is_instrument())
    }

    /// A project's master track.
    pub fn master(id: Id) -> Self {
        Self::new(id, "Master", TrackKind::Master, MASTER_COLOR)
    }

    /// The send to `to`, if there is one.
    pub fn send_to(&self, to: Id) -> Option<&Send> {
        self.sends.iter().find(|s| s.to == to)
    }
}

/// The master track's colour: a quiet grey, apart from the track palette.
pub const MASTER_COLOR: Color = Color(0x8A8F99);

/// Where a track's audio goes (format v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// Its group when it is in one, otherwise the master. For the master,
    /// the device's first pair of outputs.
    #[default]
    Auto,
    /// The master, even from inside a group.
    Master,
    /// A group or return track.
    Track(Id),
    /// A pair of device outputs: 0 is outputs 1 and 2.
    Device(u16),
}

impl Output {
    pub fn is_auto(&self) -> bool {
        *self == Output::Auto
    }
}

/// A send into a return track (format v7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Send {
    pub to: Id,
    pub level: Level,
    /// Taken before the track's volume and pan; otherwise after (the
    /// default, owner's choice).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pre_fader: bool,
}

/// A level in dB that may be silent: written as a number, or `"-inf"`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Level(pub f32);

impl Level {
    pub const SILENT: Level = Level(f32::NEG_INFINITY);

    pub fn is_silent(self) -> bool {
        self.0 == f32::NEG_INFINITY
    }

    /// Linear gain: 0 when silent.
    pub fn gain(self) -> f32 {
        if self.is_silent() {
            0.0
        } else {
            10f32.powf(self.0 / 20.0)
        }
    }
}

impl Serialize for Level {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.is_silent() {
            s.serialize_str("-inf")
        } else {
            s.serialize_f32(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Db(f32),
            Text(String),
        }
        match Written::deserialize(d)? {
            Written::Db(db) => Ok(Level(db)),
            Written::Text(text) if text == "-inf" => Ok(Level::SILENT),
            Written::Text(text) => Err(serde::de::Error::custom(format!("invalid level {text:?}"))),
        }
    }
}

/// A MIDI output (format v12): a port by the name the system gave it, and a
/// channel from 0 (shown as 1) to 15. The port may be unplugged: the name is
/// kept so the track finds the device again when it returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiTo {
    pub port: String,
    pub channel: u8,
}

/// Which inputs feed a MIDI track (format v13).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromSource {
    /// Every input switched on for tracks, and the computer keyboard.
    #[default]
    All,
    /// The computer keyboard only.
    Keyboard,
    /// One input port, by the name the system gave it. It may be unplugged:
    /// the name is kept so the track finds the device again.
    Port(String),
}

/// A MIDI track's input (format v13): a source and a channel from 0 (shown
/// as 1) to 15, or every channel.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MidiFrom {
    #[serde(default)]
    pub source: FromSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
}

impl MidiFrom {
    /// All inputs, all channels: the default, which is not written.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// When a track passes its input through to its output (format v13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Monitor {
    /// While the track is armed or selected.
    #[default]
    Auto,
    /// Always.
    In,
    /// Never.
    Off,
}

impl Monitor {
    pub fn is_default(&self) -> bool {
        *self == Self::Auto
    }
}

/// The stable name of the built-in synth in `PluginRef::Builtin` (format
/// v12). Never the product name, and never changed.
pub const SYNTH: &str = "synth";

/// What a device does in a track's chain (format v17): an instrument makes
/// sound from MIDI, an effect processes audio. Saved with a plugin so a
/// plugin that is missing on this machine still sits in the right place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Instrument,
    /// What a device written before v17 means.
    #[default]
    Effect,
}

impl DeviceKind {
    pub fn is_default(&self) -> bool {
        *self == Self::Effect
    }
}

/// The biggest saved state of one blob of one device: the host's own limit.
pub const MAX_DEVICE_STATE: usize = 16 << 20;

/// A tail that never ends (VST3's `kInfiniteTail`).
pub const INFINITE_TAIL: u32 = u32::MAX;

/// What a plugin last said about its timing (format v17), kept so a plugin
/// that goes missing or fails keeps its place in time: the bypass holds this
/// latency (the owner's D1 and D7). Frames at `rate` Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reported {
    pub latency: u32,
    /// Frames the sound rings on after the input stops; [`INFINITE_TAIL`]
    /// when it never stops.
    pub tail: u32,
    /// The sample rate the two numbers are in.
    pub rate: u32,
}

impl Reported {
    /// The latency in frames at `rate`, for a device that reported it at
    /// another rate.
    pub fn latency_at(&self, rate: u32) -> u32 {
        if self.rate == 0 || self.rate == rate {
            return self.latency;
        }
        (u64::from(self.latency) * u64::from(rate)).div_ceil(u64::from(self.rate)) as u32
    }
}

/// Where on the source track a sidechain is taken from (format v20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidechainTap {
    /// Before the track's devices: mute and fader do not touch it.
    PreFx,
    /// After the track's devices, before mute and fader.
    #[default]
    PostFx,
    /// What the track plays: after mute, fader and pan.
    PostMixer,
}

/// A sidechain: another track whose sound keys an effect (format v20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidechain {
    pub from: Id,
    #[serde(default)]
    pub tap: SidechainTap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: Id,
    pub plugin: PluginRef,
    #[serde(default)]
    pub bypass: bool,
    /// Opaque state blob from the plugin, stored as base64. For a VST3
    /// plugin this is the component's state (and, in files before v17, all
    /// the state there was).
    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<u8>,
    /// A VST3 plugin's edit-controller state, apart from the component's
    /// (format v17); empty when it keeps none.
    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub controller_state: Vec<u8>,
    /// The plugin's version when its state was captured (format v17), so an
    /// update can be told from the build that wrote the state.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plugin_version: String,
    /// The plugin's last reported latency and tail (format v17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<Reported>,
    /// Parameter values the project holds over the saved state (format v19),
    /// normalized 0 to 1 by the plugin's parameter id: what was set from
    /// Lumen or from the plugin's own window since the state was captured. A
    /// plugin restores its state and then these are set, so undo and redo of a
    /// parameter change are edits to this map. Written only when there are
    /// some; a save that captures the plugin's state afresh clears it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<u32, f64>,
    /// The track whose sound keys this effect, if it takes a sidechain
    /// (format v20): where the signal is taken from on that track. Written
    /// only when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<Sidechain>,
    /// How many times the device's sound was replaced as a whole (a preset
    /// loaded), so the running plugin can tell that the project holds another
    /// sound than the one it has, after an undo or redo, without comparing
    /// megabytes of state. Not saved: a project opens with the plugin set up
    /// from what the file holds.
    #[serde(skip)]
    pub state_rev: u32,
}

impl Device {
    /// A device with no VST3-only data: what every device was before v17.
    pub fn new(id: Id, plugin: PluginRef, bypass: bool, state: Vec<u8>) -> Self {
        Self {
            id,
            plugin,
            bypass,
            state,
            controller_state: Vec::new(),
            plugin_version: String::new(),
            reported: None,
            params: BTreeMap::new(),
            sidechain: None,
            state_rev: 0,
        }
    }

    /// A VST3 device with no state yet.
    pub fn vst3(id: Id, class_id: &str, name: &str, vendor: &str, kind: DeviceKind) -> Self {
        Self::new(
            id,
            PluginRef::Vst3 {
                class_id: class_id.into(),
                name: name.into(),
                vendor: vendor.into(),
                kind,
            },
            false,
            Vec::new(),
        )
    }

    /// What the device does in a chain: the synth is an instrument, a VST3
    /// plugin says which it is, any other built-in device is an effect.
    pub fn kind(&self) -> DeviceKind {
        self.plugin.kind()
    }

    pub fn is_instrument(&self) -> bool {
        self.kind() == DeviceKind::Instrument
    }

    pub fn is_vst3(&self) -> bool {
        matches!(self.plugin, PluginRef::Vst3 { .. })
    }

    /// The built-in synth with a preset (`"poly"`, `"keys"`, ...). For a
    /// built-in device `state` is its own state; the synth's is its
    /// preset's stable name as text.
    pub fn synth(id: Id, preset: &str) -> Self {
        Self::new(
            id,
            PluginRef::Builtin { name: SYNTH.into() },
            false,
            preset.as_bytes().to_vec(),
        )
    }

    pub fn is_synth(&self) -> bool {
        matches!(&self.plugin, PluginRef::Builtin { name } if name == SYNTH)
    }

    /// The preset of a synth: its stable name, if the state is readable.
    pub fn preset(&self) -> Option<&str> {
        self.is_synth()
            .then(|| std::str::from_utf8(&self.state).ok())
            .flatten()
    }
}

impl PluginRef {
    /// What the plugin does in a chain: the synth is an instrument, a VST3
    /// plugin says which it is, any other built-in device is an effect.
    pub fn kind(&self) -> DeviceKind {
        match self {
            PluginRef::Vst3 { kind, .. } => *kind,
            PluginRef::Builtin { name } if name == SYNTH => DeviceKind::Instrument,
            PluginRef::Builtin { .. } => DeviceKind::Effect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum PluginRef {
    Vst3 {
        class_id: String,
        name: String,
        vendor: String,
        /// Instrument or effect (format v17); older files hold effects.
        #[serde(default, skip_serializing_if = "DeviceKind::is_default")]
        kind: DeviceKind,
    },
    Builtin {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    pub start: Ticks,
    /// How long the clip lasts on the timeline, repeats included.
    pub length: Ticks,
    #[serde(default)]
    pub muted: bool,
    // Format v5. Written only when set, so a plain clip reads (and costs)
    // the same as before and files stay small.
    /// Its own colour, or the track's when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Locked clips cannot be moved, trimmed or deleted.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Fades at the clip's start and end.
    #[serde(default, skip_serializing_if = "Fade::is_none")]
    pub fade_in: Fade,
    #[serde(default, skip_serializing_if = "Fade::is_none")]
    pub fade_out: Fade,
    /// When set, the content repeats every `loop_length` from the clip's
    /// start, for as long as the clip lasts (loop-extend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_length: Option<Ticks>,
    pub content: ClipContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClipContent {
    Audio {
        media: Id,
        /// First sample of the media file played at clip start.
        offset: i64,
        gain_db: f32,
    },
    Midi {
        notes: Vec<Note>,
        /// Controllers, pitch bend, pressure and program changes (format
        /// v11), in time order. Written only when there are some.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        events: Vec<ClipEvent>,
    },
}

impl ClipContent {
    /// An empty MIDI clip's content.
    pub fn midi() -> Self {
        ClipContent::Midi {
            notes: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// A non-note MIDI event in a clip, at `at` ticks from the clip's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEvent {
    pub at: Ticks,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// What a [`ClipEvent`] says. Values use the MIDI ranges: 7-bit for
/// controllers and pressure, 14-bit centred on 0 for pitch bend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Controller {
        number: u8,
        value: u8,
    },
    /// −8192 to 8191, 0 is centre.
    PitchBend {
        value: i16,
    },
    /// Channel pressure (aftertouch).
    Pressure {
        value: u8,
    },
    /// A program change, with the bank (MSB << 7 | LSB) when one is given.
    Program {
        program: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bank: Option<u16>,
    },
}

impl EventKind {
    /// Whether the numbers are inside MIDI's ranges.
    pub fn in_range(self) -> bool {
        match self {
            EventKind::Controller { number, value } => number < 128 && value < 128,
            EventKind::PitchBend { value } => (-8192..=8191).contains(&value),
            EventKind::Pressure { value } => value < 128,
            EventKind::Program { program, bank } => {
                program < 128 && bank.is_none_or(|b| b < 128 * 128)
            }
        }
    }
}

/// A note, written as `[start, length, pitch, velocity]` to keep large clips
/// readable and compact; a muted note (format v14) has a fifth element,
/// `true`. A note that cannot carry through a seam (format v16) has a sixth
/// element, `false`. `start` is relative to the clip. Build one with
/// [`Note::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note(pub Ticks, pub Ticks, pub u8, pub u8, pub bool, pub bool);

impl Note {
    /// An unmuted note.
    pub fn new(start: Ticks, length: Ticks, pitch: u8, velocity: u8) -> Self {
        Note(start, length, pitch, velocity, false, true)
    }
    pub fn start(self) -> Ticks {
        self.0
    }
    pub fn length(self) -> Ticks {
        self.1
    }
    pub fn pitch(self) -> u8 {
        self.2
    }
    pub fn velocity(self) -> u8 {
        self.3
    }
    /// Whether the note is switched off: it stays in the clip, but is not
    /// played.
    pub fn muted(self) -> bool {
        self.4
    }
    /// Whether this note may carry a sounding note through a clip seam.
    /// Recorded notes hidden outside their take window do not.
    pub fn carries_on(self) -> bool {
        self.5
    }
    /// Sets whether this note may carry through a clip seam.
    pub fn with_carries_on(mut self, carries_on: bool) -> Self {
        self.5 = carries_on;
        self
    }
    /// Where the note ends.
    pub fn end(self) -> Ticks {
        self.0 + self.1
    }
}

impl Serialize for Note {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut t = s.serialize_tuple(if self.5 {
            if self.4 { 5 } else { 4 }
        } else {
            6
        })?;
        t.serialize_element(&self.0)?;
        t.serialize_element(&self.1)?;
        t.serialize_element(&self.2)?;
        t.serialize_element(&self.3)?;
        if self.4 || !self.5 {
            t.serialize_element(&self.4)?;
        }
        if !self.5 {
            t.serialize_element(&false)?;
        }
        t.end()
    }
}

impl<'de> Deserialize<'de> for Note {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        /// Four elements, five with the mute flag, or six with mute and seam carry.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Plain(Ticks, Ticks, u8, u8),
            Muted(Ticks, Ticks, u8, u8, bool),
            Carry(Ticks, Ticks, u8, u8, bool, bool),
        }
        Ok(match Form::deserialize(d)? {
            Form::Plain(a, b, c, v) => Note(a, b, c, v, false, true),
            Form::Muted(a, b, c, v, m) => Note(a, b, c, v, m, true),
            Form::Carry(a, b, c, v, m, carry) => Note(a, b, c, v, m, carry),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    pub id: Id,
    /// Relative to the project folder when inside it (always `/`-separated),
    /// absolute otherwise.
    pub path: String,
    /// Where the file was when last saved; a relink hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
    pub size_bytes: u64,
}

mod base64_bytes {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(x: &f32) -> bool {
    *x == 0.0
}
