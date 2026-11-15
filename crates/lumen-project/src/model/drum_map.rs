use super::*;
use lumen_core::grid::{Feel, Size};

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

    pub fn from_name(name: &str) -> DrumIcon {
        DrumIcon::ALL
            .into_iter()
            .find(|icon| icon.name() == name)
            .unwrap_or(DrumIcon::Percussion)
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    #[default]
    Main,
    Output(u32),
    Track(Id),
}

impl Route {
    pub fn is_main(&self) -> bool {
        *self == Route::Main
    }

    pub fn sent_to(&self) -> Option<Id> {
        match self {
            Route::Track(id) => Some(*id),
            Route::Main | Route::Output(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Quantize {
    pub size: Size,
    pub feel: Feel,
}

impl<'de> Deserialize<'de> for Quantize {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sound {
    pub name: String,
    pub input: u8,
    pub output: u8,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Ticks>,
    pub color: Color,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke: Option<u8>,
    pub icon: DrumIcon,
}

impl Sound {
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

    pub fn choking(mut self, group: u8) -> Self {
        self.choke = Some(group);
        self
    }
}

pub const MOST_SOUND_NAME: usize = 64;

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

    pub fn sound(&self, input: u8) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.input == input)
    }

    pub fn sound_playing(&self, output: u8) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.output == output)
    }

    pub fn any_soloed(&self) -> bool {
        self.sounds.iter().any(|s| s.solo)
    }

    pub fn heard(&self, sound: &Sound) -> bool {
        !sound.mute && (sound.solo || !self.any_soloed())
    }

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
        let read: Quantize =
            serde_json::from_str(r#"{"size":{"bars":1},"feel":"triplet"}"#).expect("a grid");
        assert_eq!(read.size, Size::Bars(1));
        assert_eq!(read.feel, Feel::Straight);
        let dotted: Quantize =
            serde_json::from_str(r#"{"size":{"bars":2},"feel":"dotted"}"#).expect("a grid");
        assert_eq!(dotted.feel, Feel::Straight);
        let note: Quantize =
            serde_json::from_str(r#"{"size":{"note":8},"feel":"triplet"}"#).expect("a grid");
        assert_eq!(note.feel, Feel::Triplet);
        let text = serde_json::to_string(&note).expect("a grid");
        assert!(text.contains("triplet"), "{text}");
    }
}
