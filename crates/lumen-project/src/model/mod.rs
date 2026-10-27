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
mod takes;
mod track;

pub use clip::*;
pub use device::*;
pub use takes::*;
pub use track::*;

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
            .ok_or_else(|| serde::de::Error::custom(format!("invalid color {text:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: Vec<TempoPoint>,
    /// Automatically crossfade eligible audio joins (format v28). New
    /// projects enable this; migration explicitly disables it for old files.
    #[serde(default = "default_auto_crossfade", skip_serializing_if = "is_true")]
    pub auto_crossfade: bool,
    /// The time signature along the song (format v24; one `time_signature`
    /// before): the first change at zero, each later one on a bar line of
    /// the meter before it. Part of undo.
    pub meter: Vec<MeterPoint>,
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
    /// Which control each MIDI controller number moves (format v23). Part of
    /// undo. Written only when there are some.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<crate::mapping::Mapping>,
    /// Free text kept with the project (format v32), written only when there
    /// is some. Part of undo.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The colour new tracks take (format v32); none cycles the palette.
    /// Part of undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_track_color: Option<Color>,
}

/// The most text a project's notes may hold, in bytes (`validate`).
pub const MOST_NOTES_BYTES: usize = 200_000;

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

    /// The signature of `meter`, if a project can hold it (at most 64
    /// beats to a bar).
    pub fn of(meter: Meter) -> Option<TimeSignature> {
        Some(TimeSignature {
            numerator: u8::try_from(meter.numerator).ok().filter(|&n| n <= 64)?,
            denominator: u8::try_from(meter.denominator).ok()?,
        })
    }
}

impl Project {
    /// The time-signature map. A project that passed validation always has
    /// a valid one; a damaged one reads as 4/4 throughout.
    pub fn meter_map(&self) -> MeterMap {
        MeterMap::new(self.meter.clone())
            .unwrap_or_else(|_| MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is valid"))
    }

    /// The time signature in force at `at`.
    pub fn signature_at(&self, at: Ticks) -> TimeSignature {
        TimeSignature::of(self.meter_map().meter_at(at)).unwrap_or(TimeSignature::FOUR_FOUR)
    }
}

/// One time signature for the whole song, as a project's `meter`.
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
