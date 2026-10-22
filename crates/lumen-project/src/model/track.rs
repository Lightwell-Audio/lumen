//! Tracks and how they are routed (the project document model).

use super::*;

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
    /// The lanes that automate this track's values and its devices'
    /// parameters (format v22), at most one for each target. Written only
    /// when there are some.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
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
            automation: Vec::new(),
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
