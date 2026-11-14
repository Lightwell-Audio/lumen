//! Drum maps (M8 slice D0, format v35): a kit's sounds, and the note each
//! one answers to.
//!
//! A clip keeps its notes as the map's **input** notes, so attaching,
//! swapping or editing a map never changes a note (§6.4). What plays is
//! worked out from the map at play time (`crate::drums::played`), on the
//! track's own channel unless a sound names another.
//!
//! Maps live in the project's list and a track names one by id (the owner's
//! DM A), so two kit tracks share one map; a preset is copied into the list
//! when it is attached, so a project never depends on a file elsewhere.

use super::*;
use lumen_core::grid::{Feel, Size};

/// The drawing a sound carries (M8, the owner's IC): the fifteen line icons
/// of round 3. Saved by name; a name this build does not know reads as
/// [`DrumIcon::Percussion`], so a project from a later version opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum DrumIcon {
    Kick,
    Snare,
    Rim,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Tom,
    FloorTom,
    Crash,
    Ride,
    Cowbell,
    Shaker,
    #[default]
    Percussion,
    Effect,
}

impl DrumIcon {
    /// Every icon, in the order the drawing showcases them.
    pub const ALL: [DrumIcon; 15] = [
        DrumIcon::Kick,
        DrumIcon::Snare,
        DrumIcon::Rim,
        DrumIcon::Clap,
        DrumIcon::ClosedHat,
        DrumIcon::PedalHat,
        DrumIcon::OpenHat,
        DrumIcon::Tom,
        DrumIcon::FloorTom,
        DrumIcon::Crash,
        DrumIcon::Ride,
        DrumIcon::Cowbell,
        DrumIcon::Shaker,
        DrumIcon::Percussion,
        DrumIcon::Effect,
    ];

    /// The name it is saved by.
    pub fn name(self) -> &'static str {
        match self {
            DrumIcon::Kick => "kick",
            DrumIcon::Snare => "snare",
            DrumIcon::Rim => "rim",
            DrumIcon::Clap => "clap",
            DrumIcon::ClosedHat => "closed_hat",
            DrumIcon::PedalHat => "pedal_hat",
            DrumIcon::OpenHat => "open_hat",
            DrumIcon::Tom => "tom",
            DrumIcon::FloorTom => "floor_tom",
            DrumIcon::Crash => "crash",
            DrumIcon::Ride => "ride",
            DrumIcon::Cowbell => "cowbell",
            DrumIcon::Shaker => "shaker",
            DrumIcon::Percussion => "percussion",
            DrumIcon::Effect => "effect",
        }
    }

    /// The icon `name` written for, or [`DrumIcon::Percussion`] for a name
    /// this build does not know.
    pub fn from_name(name: &str) -> DrumIcon {
        DrumIcon::ALL
            .into_iter()
            .find(|icon| icon.name() == name)
            .unwrap_or(DrumIcon::Percussion)
    }

    /// The colour of a sound's lane for this piece: the mixed track palette,
    /// so a kit reads in blocks (kick, snare, hats, toms, cymbals,
    /// percussion). It lives here, beside the icon, so a sound's colour does
    /// not depend on the preset tables (review L5).
    pub fn color(self) -> Color {
        let index = match self {
            DrumIcon::Kick => 0,
            DrumIcon::Snare | DrumIcon::Rim => 4,
            DrumIcon::Clap => 7,
            DrumIcon::ClosedHat | DrumIcon::PedalHat | DrumIcon::OpenHat => 5,
            DrumIcon::Tom | DrumIcon::FloorTom | DrumIcon::Cowbell => 3,
            DrumIcon::Crash | DrumIcon::Ride => 8,
            DrumIcon::Shaker => 9,
            DrumIcon::Percussion => 2,
            DrumIcon::Effect => 6,
        };
        Color::in_turn(index)
    }
}

impl Serialize for DrumIcon {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for DrumIcon {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(DrumIcon::from_name(&String::deserialize(d)?))
    }
}

/// Where a sound plays (format v35): the instrument's main output, one of
/// its output pairs, or another instrument track (§6.4's "the instrument or
/// a specific instrument output"). D0 stores it; D4 is the routing that
/// honours it.
///
/// `Output(n)` counts the instrument's outputs as it numbers them, 1 and up:
/// for a hosted instrument the audio bus after the main one (bus 0 is the
/// track itself), which is also the pair the host numbers while every
/// earlier bus is on; for the built-in sampler, the pad's output pair (B5).
/// A sound sent to another track (`Track`) plays that track's instrument,
/// wherever this track's own output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// The instrument's main output: the track's own.
    #[default]
    Main,
    /// An auxiliary output pair of the instrument, 1 and up.
    Output(u32),
    /// Another instrument track.
    Track(Id),
}

impl Route {
    pub fn is_main(&self) -> bool {
        *self == Route::Main
    }

    /// The track the sound's notes are given to, when it plays another
    /// instrument track's instrument rather than its own (M8 D4). An output
    /// pair keeps the notes on its own track: the instrument routes them
    /// inside itself.
    pub fn sent_to(&self) -> Option<Id> {
        match self {
            Route::Track(id) => Some(*id),
            Route::Main | Route::Output(_) => None,
        }
    }
}

/// A sound's own quantize grid (§6.4): the step the drum editor's quantize
/// uses on its lane. A bar has no triplet or dotted feel (review L8): a file
/// that names one reads as straight, and the map editor does not offer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Quantize {
    pub size: Size,
    pub feel: Feel,
}

impl<'de> Deserialize<'de> for Quantize {
    /// Reads a grid, normalising a bar's feel to straight (review L8). This
    /// belongs to format v35, the version that introduced `Quantize`: no
    /// build that offers a dotted or triplet bar can have written one, so
    /// every such file is a hand edit. A later format version that offers a
    /// bar's feel must gate this normalisation on the version it is reading,
    /// or the value is silently lost.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            size: Size,
            #[serde(default)]
            feel: Feel,
        }
        let raw = Raw::deserialize(deserializer)?;
        let feel = match raw.size {
            Size::Bars(_) => Feel::Straight,
            Size::Note(_) => raw.feel,
        };
        Ok(Quantize {
            size: raw.size,
            feel,
        })
    }
}

/// One sound of a map: what it is called, the note it answers to and the
/// note it plays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sound {
    pub name: String,
    /// The note a clip stores, and a player's pad sends (0 to 127).
    pub input: u8,
    /// The note the instrument gets (0 to 127).
    pub output: u8,
    /// The channel it plays on, or the track's when none (0 to 15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(default, skip_serializing_if = "Route::is_main")]
    pub route: Route,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantize: Option<Quantize>,
    /// How long the sound plays, whatever the note's length (ticks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Ticks>,
    pub color: Color,
    /// The sounds in one group end each other: a new hit sends a note-off to
    /// the others at the same sample (1 and up; the owner's CO A).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke: Option<u8>,
    pub icon: DrumIcon,
}

impl Sound {
    /// A sound of `name` on `input`, playing `output`, in the palette's
    /// colour for its icon.
    pub fn new(name: impl Into<String>, input: u8, output: u8, icon: DrumIcon) -> Self {
        Self {
            name: name.into(),
            input,
            output,
            channel: None,
            route: Route::Main,
            mute: false,
            solo: false,
            quantize: None,
            length: None,
            color: icon.color(),
            choke: None,
            icon,
        }
    }

    /// The same sound in choke group `group`.
    pub fn choking(mut self, group: u8) -> Self {
        self.choke = Some(group);
        self
    }
}

/// The longest name a drum sound or a map may carry, in characters.
pub const MOST_SOUND_NAME: usize = 64;

/// A kit: an id, a name and its sounds, in the lanes' order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrumMap {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub sounds: Vec<Sound>,
}

impl DrumMap {
    pub fn new(id: Id, name: impl Into<String>, sounds: Vec<Sound>) -> Self {
        Self {
            id,
            name: name.into(),
            sounds,
        }
    }

    /// The first sound answering to `input` (input notes are unique in a
    /// map, so it is the one).
    pub fn sound(&self, input: u8) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.input == input)
    }

    /// The first sound that plays `output`, for the note a recording keeps.
    pub fn sound_playing(&self, output: u8) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.output == output)
    }

    /// Whether a sound is soloed: every other one is then silent.
    pub fn any_soloed(&self) -> bool {
        self.sounds.iter().any(|s| s.solo)
    }

    /// Whether `sound` is heard: not muted, and soloed when any is.
    pub fn heard(&self, sound: &Sound) -> bool {
        !sound.mute && (sound.solo || !self.any_soloed())
    }

    /// The choke group a played note is in, by the note it plays: 0 for
    /// none. Two sounds with one output note share a group (the first
    /// sound's, as the map is read in order).
    pub fn choke_of_output(&self, output: u8) -> u8 {
        self.sounds
            .iter()
            .find(|s| s.output == output)
            .and_then(|s| s.choke)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quantize_with_a_bar_and_a_feel_reads_as_straight() {
        // Review L8: a bar has no triplet or dotted feel; a file that names
        // one (a later build, a hand-edited file) reads as straight.
        let read: Quantize =
            serde_json::from_str(r#"{"size":{"bars":1},"feel":"triplet"}"#).expect("a grid");
        assert_eq!(read.size, Size::Bars(1));
        assert_eq!(read.feel, Feel::Straight);
        let dotted: Quantize =
            serde_json::from_str(r#"{"size":{"bars":2},"feel":"dotted"}"#).expect("a grid");
        assert_eq!(dotted.feel, Feel::Straight);
        // A note keeps its feel.
        let note: Quantize =
            serde_json::from_str(r#"{"size":{"note":8},"feel":"triplet"}"#).expect("a grid");
        assert_eq!(note.feel, Feel::Triplet);
        // And it is written as it is read.
        let text = serde_json::to_string(&note).expect("a grid");
        assert!(text.contains("triplet"), "{text}");
    }
}
