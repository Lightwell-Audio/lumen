use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::automation::AutomationLane;
use crate::envelope::ClipEnvelopes;
use lumen_core::fade::Fade;
use lumen_core::grid::Grid;
use lumen_core::meter::{Meter, MeterMap, MeterPoint};
use lumen_core::pan::PanLaw;
use lumen_core::time::{TempoPoint, Ticks};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod clip;
mod device;
mod drum_map;
mod takes;
mod track;
mod warp;

pub use clip::*;
pub use device::*;
pub use drum_map::*;
pub use takes::*;
pub use track::*;
pub use warp::*;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const PALETTE: [Color; 12] = [
        Color(0x4C7A9E),
        Color(0x7A6A9E),
        Color(0x5E8C6A),
        Color(0x9E7A4C),
        Color(0x9E5A6A),
        Color(0x5A8E8E),
        Color(0x6A7EB0),
        Color(0x8E6A8E),
        Color(0x7E8E56),
        Color(0xA06E5A),
        Color(0x5686A0),
        Color(0x887660),
    ];

    pub fn in_turn(index: usize) -> Color {
        Self::PALETTE[index % Self::PALETTE.len()]
    }
}

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
            .ok_or_else(|| serde::de::Error::custom(format!("invalid color {text:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: Vec<TempoPoint>,
    #[serde(default = "default_auto_crossfade", skip_serializing_if = "is_true")]
    pub auto_crossfade: bool,
    pub meter: Vec<MeterPoint>,
    pub tracks: Vec<Arc<Track>>,
    pub master: Arc<Track>,
    #[serde(default)]
    pub pan_law: PanLaw,
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub transport: TransportState,
    #[serde(default)]
    pub view: ViewState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<crate::mapping::Mapping>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_track_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drum_maps: Vec<DrumMap>,
}

pub const MOST_NOTES_BYTES: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    pub at: Ticks,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportState {
    pub loop_start: Ticks,
    pub loop_end: Ticks,
    pub loop_enabled: bool,
    pub punch_in: bool,
    pub punch_out: bool,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            loop_start: Ticks::ZERO,
            loop_end: Ticks::quarters(16),
            loop_enabled: false,
            punch_in: false,
            punch_out: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    pub left_beat: f32,
    pub px_per_beat: f32,
    pub browser_open: bool,
    pub detail_open: bool,
    pub browser_size: f32,
    pub detail_size: f32,
    pub top: f32,
    pub track_heights: BTreeMap<Id, f32>,
    pub expanded: BTreeSet<Id>,
    pub grid: Grid,
    pub collapsed: BTreeSet<Id>,
    pub header_width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns_height: Option<f32>,
    pub returns_folded: bool,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub folded_devices: BTreeSet<Id>,
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

impl TimeSignature {
    pub const FOUR_FOUR: TimeSignature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };

    pub fn meter(self) -> Meter {
        Meter {
            numerator: self.numerator.into(),
            denominator: self.denominator.into(),
        }
    }
}

impl Project {
    pub fn meter_map(&self) -> MeterMap {
        MeterMap::new(self.meter.clone())
            .unwrap_or_else(|_| MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is valid"))
    }

    pub fn drum_map(&self, id: Id) -> Option<&DrumMap> {
        self.drum_maps.iter().find(|m| m.id == id)
    }

    pub fn drum_map_of(&self, track: &Track) -> Option<&DrumMap> {
        self.drum_map(track.drum_map?)
    }
}

pub fn constant_meter(signature: TimeSignature) -> Vec<MeterPoint> {
    vec![MeterPoint {
        at: Ticks::ZERO,
        meter: signature.meter(),
    }]
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

fn is_true(b: &bool) -> bool {
    *b
}

fn default_auto_crossfade() -> bool {
    true
}

fn is_zero(x: &f32) -> bool {
    *x == 0.0
}
