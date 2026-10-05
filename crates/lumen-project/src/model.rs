//! The project document model, as stored in `.lumen` files.
//!
//! Minimal in M1: enough structure to prove the format at scale (tracks,
//! groups, clips, notes, media, opaque plugin state). It grows feature by
//! feature; every change to what is persisted bumps the format version and
//! adds a migration (see `migrate`).

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
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub media: Vec<Media>,
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
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: Id,
    pub plugin: PluginRef,
    #[serde(default)]
    pub bypass: bool,
    /// Opaque state blob from the plugin, stored as base64.
    #[serde(with = "base64_bytes", default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum PluginRef {
    Vst3 {
        class_id: String,
        name: String,
        vendor: String,
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
    pub length: Ticks,
    #[serde(default)]
    pub muted: bool,
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
    },
}

/// A note, written as `[start, length, pitch, velocity]` to keep large clips
/// readable and compact. `start` is relative to the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note(pub Ticks, pub Ticks, pub u8, pub u8);

impl Note {
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
