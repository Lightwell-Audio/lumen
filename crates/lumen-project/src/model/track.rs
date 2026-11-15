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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    pub gain_db: f32,
    pub pan: f32,
    #[serde(default, skip_serializing_if = "Output::is_auto")]
    pub output: Output,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<Send>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim_db: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_to: Option<MidiTo>,
    #[serde(default, skip_serializing_if = "MidiFrom::is_default")]
    pub midi_from: MidiFrom,
    #[serde(default, skip_serializing_if = "Monitor::is_default")]
    pub monitor: Monitor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<AudioInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_output: Option<PluginOutput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<Frozen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drum_map: Option<Id>,
    #[serde(default)]
    pub clips: Vec<Arc<Clip>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInput {
    pub channel: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stereo: bool,
}

impl AudioInput {
    pub fn mono(channel: u16) -> Self {
        Self {
            channel,
            stereo: false,
        }
    }

    pub fn pair(channel: u16) -> Self {
        Self {
            channel,
            stereo: true,
        }
    }

    pub fn channels(&self) -> std::ops::Range<u16> {
        self.channel..self.channel + 1 + u16::from(self.stereo)
    }

    pub fn overlaps(&self, other: &AudioInput) -> bool {
        let (a, b) = (self.channels(), other.channels());
        a.start < b.end && b.start < a.end
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frozen {
    pub id: Id,
    pub media: Id,
    pub start: Ticks,
    pub length: Ticks,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mono: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOutput {
    pub device: Id,
    pub bus: u32,
}

impl Track {
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
            input: None,
            plugin_output: None,
            automation: Vec::new(),
            frozen: None,
            drum_map: None,
            clips: Vec::new(),
        }
    }

    pub fn plays_mono(&self, media: &[Media]) -> bool {
        if let Some(frozen) = &self.frozen {
            return frozen.mono;
        }
        if self.kind != TrackKind::Audio || self.clips.is_empty() {
            return false;
        }
        self.clips.iter().all(|clip| match clip.content {
            ClipContent::Audio { media: id, .. } => media
                .iter()
                .find(|m| m.id == id)
                .is_some_and(|m| m.channels == 1),
            ClipContent::Midi { .. } => false,
        })
    }

    pub fn instrument(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.is_synth())
    }

    pub fn sound_source(&self) -> Option<&Device> {
        self.devices.iter().find(|d| d.is_instrument())
    }

    pub fn master(id: Id) -> Self {
        Self::new(id, "Master", TrackKind::Master, MASTER_COLOR)
    }

    pub fn send_to(&self, to: Id) -> Option<&Send> {
        self.sends.iter().find(|s| s.to == to)
    }
}

pub const MASTER_COLOR: Color = Color(0x8A8F99);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    #[default]
    Auto,
    Master,
    Track(Id),
    Device(u16),
}

impl Output {
    pub fn is_auto(&self) -> bool {
        *self == Output::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Send {
    pub to: Id,
    pub level: Level,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pre_fader: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Level(pub f32);

impl Level {
    pub const SILENT: Level = Level(f32::NEG_INFINITY);

    pub fn is_silent(self) -> bool {
        self.0 == f32::NEG_INFINITY
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiTo {
    pub port: String,
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromSource {
    #[default]
    All,
    Keyboard,
    Port(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MidiFrom {
    #[serde(default)]
    pub source: FromSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
}

impl MidiFrom {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Monitor {
    #[default]
    Auto,
    In,
    Off,
}

impl Monitor {
    pub fn is_default(&self) -> bool {
        *self == Self::Auto
    }
}
