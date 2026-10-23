//! Clips, notes and the media they play (the project document model).

use super::*;

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
    /// Elapsed tick offset from the source loop's origin at this clip start
    /// (format v26); consumers wrap it by `loop_length` for source lookup.
    #[serde(default, skip_serializing_if = "ticks_are_zero")]
    pub loop_phase: Ticks,
    /// Elapsed source seconds from the integer media offset (format v26).
    /// Signed so a start trim can reveal earlier source frames. Looped clips
    /// use `loop_phase` for their repeated source position.
    #[serde(default, skip_serializing_if = "seconds_are_zero")]
    pub audio_phase_seconds: f64,
    /// Gain and pan envelopes of an audio clip (format v25), measured from
    /// the clip's start. Written only when there are some.
    #[serde(default, skip_serializing_if = "ClipEnvelopes::is_empty")]
    pub envelopes: ClipEnvelopes,
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
        /// Integer source-frame anchor (format v26); `Clip.audio_phase_seconds`
        /// gives the elapsed source time from this frame to the clip start.
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
