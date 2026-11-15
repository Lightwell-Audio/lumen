use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    pub start: Ticks,
    pub length: Ticks,
    #[serde(default)]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Fade::is_none")]
    pub fade_in: Fade,
    #[serde(default, skip_serializing_if = "is_false")]
    pub crossfade: bool,
    #[serde(default, skip_serializing_if = "Fade::is_none")]
    pub fade_out: Fade,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_length: Option<Ticks>,
    #[serde(default, skip_serializing_if = "ticks_are_zero")]
    pub loop_phase: Ticks,
    #[serde(default, skip_serializing_if = "seconds_are_zero")]
    pub audio_phase_seconds: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub inverted: bool,
    #[serde(default, skip_serializing_if = "ClipEnvelopes::is_empty")]
    pub envelopes: ClipEnvelopes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<Takes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warp: Option<Warp>,
    pub content: ClipContent,
}

fn ticks_are_zero(ticks: &Ticks) -> bool {
    *ticks == Ticks::ZERO
}

fn seconds_are_zero(seconds: &f64) -> bool {
    *seconds == 0.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClipContent {
    Audio {
        media: Id,
        offset: i64,
        gain_db: f32,
    },
    Midi {
        notes: Vec<Note>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        events: Vec<ClipEvent>,
    },
}

impl ClipContent {
    pub fn midi() -> Self {
        ClipContent::Midi {
            notes: Vec::new(),
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEvent {
    pub at: Ticks,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Controller {
        number: u8,
        value: u8,
    },
    PitchBend {
        value: i16,
    },
    Pressure {
        value: u8,
    },
    Program {
        program: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bank: Option<u16>,
    },
}

impl EventKind {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note(pub Ticks, pub Ticks, pub u8, pub u8, pub bool, pub bool);

impl Note {
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
    pub fn muted(self) -> bool {
        self.4
    }
    pub fn carries_on(self) -> bool {
        self.5
    }
    pub fn with_carries_on(mut self, carries_on: bool) -> Self {
        self.5 = carries_on;
        self
    }
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
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
    pub size_bytes: u64,
}
