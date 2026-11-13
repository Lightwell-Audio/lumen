//! What a sampler's device holds (M8 slice B5, format v36): the samples it
//! plays, where their regions are, and how they answer notes.
//!
//! The device is one instrument with three faces (the owner's SP A): **One
//! shot** plays one region, **Zones** plays the zone whose key and velocity
//! ranges hold the note, and **Pads** plays one pad per sound of the track's
//! drum map (16 pads of its own when the track names none).
//!
//! Every sample is a file of the project's media (`Project::media`), so the
//! media pool lists it, Relink finds it and Collect and save copies it; a
//! sample whose file is gone reads the same way a missing clip's does. The
//! decoded audio never goes in the project: only which file, and where in it.
//!
//! Region positions are **frames of the file's own sample rate**, as a
//! waveform shows them, so a project played at another rate keeps its edits
//! where the person put them.

use super::*;

/// The stable name of the built-in sampler in `PluginRef::Builtin`. Never the
/// product name, and never changed.
pub const SAMPLER: &str = "sampler";

/// The most zones one sampler may hold: a map of a keyboard, not a library.
pub const MOST_ZONES: usize = 128;

/// The most pads: every key a pad could answer to.
pub const MOST_PADS: usize = 128;

/// The first of the sixteen pads a track with no drum map has (C1 in the
/// naming drum samplers use), and the last.
pub const FIRST_PAD: u8 = 36;
pub const LAST_PAD: u8 = FIRST_PAD + 15;

/// How far a pad or a filter setting may move, in semitones.
pub const MOST_TUNE: f32 = 24.0;
/// A pad's level, in decibels.
pub const LOWEST_LEVEL: f32 = -60.0;
pub const HIGHEST_LEVEL: f32 = 12.0;
/// How long a choked or re-struck pad fades, in seconds.
pub const SHORTEST_CHOKE: f32 = 0.0005;
pub const LONGEST_CHOKE: f32 = 2.0;
/// The default choke fade: 5 ms, the owner's CO A.
pub const DEFAULT_CHOKE: f32 = 0.005;
/// The most output pairs a sampler has (the instrument's auxiliary outputs).
pub const PAIRS: u32 = 8;

/// Which of the three faces plays (SP A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    /// One region, played at its root key and transposed elsewhere.
    #[default]
    OneShot,
    /// The zone whose key and velocity ranges hold the note.
    Zones,
    /// The pad of the note.
    Pads,
}

impl Face {
    /// Every face, in the order the switch offers them.
    pub const ALL: [Face; 3] = [Face::OneShot, Face::Zones, Face::Pads];

    /// The face after this one: what the switch cycles through.
    pub fn next(self) -> Face {
        match self {
            Face::OneShot => Face::Zones,
            Face::Zones => Face::Pads,
            Face::Pads => Face::OneShot,
        }
    }

    /// What the face is called.
    pub fn name(self) -> &'static str {
        match self {
            Face::OneShot => "One shot",
            Face::Zones => "Zones",
            Face::Pads => "Pads",
        }
    }
}

/// Where a sample plays: the file, and the part of it (format v36). `start`
/// and `end` are frames of the file's own rate; `end` is `None` for its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// The media file it plays.
    pub media: Id,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub start: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
    /// The key it plays at its own speed (0 to 127).
    pub root: u8,
}

impl Region {
    /// The whole file at `root`.
    pub fn whole(media: Id, root: u8) -> Self {
        Self {
            media,
            start: 0,
            end: None,
            root,
        }
    }

    /// Whether `[start, end)` is a part of a file of `frames` frames, in
    /// order and inside it (`end` included when it is the file's end).
    pub fn fits(&self, frames: u64) -> bool {
        let end = self.end.unwrap_or(frames);
        self.start < end && end <= frames.max(1)
    }
}

/// The One shot face: one region, looped or reversed (format v36).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OneShot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    /// The region repeats until the note ends.
    #[serde(default, skip_serializing_if = "is_false")]
    pub looping: bool,
    /// The region plays backwards.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
}

/// A zone of the Zones face: a file that answers a range of keys and
/// velocities, played at its own speed on `root` (format v36).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub media: Id,
    /// The keys it answers, lowest first, inside the 0 to 127 range.
    pub low: u8,
    pub high: u8,
    /// The velocities it answers, lowest first.
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub root: u8,
}

impl Zone {
    /// A zone of `media` for one key at one velocity, played at its own speed
    /// on `root`.
    pub fn single(media: Id, key: u8, root: u8) -> Self {
        Self {
            media,
            low: key,
            high: key,
            low_velocity: 1,
            high_velocity: 127,
            root,
        }
    }

    /// Whether the zone answers a note of `key` played at `velocity`.
    pub fn holds(&self, key: u8, velocity: u8) -> bool {
        (self.low..=self.high).contains(&key)
            && (self.low_velocity..=self.high_velocity).contains(&velocity)
    }

    /// Whether its ranges are in order and inside the note range.
    pub fn valid(&self) -> bool {
        self.low <= self.high
            && self.high <= 127
            && self.low_velocity <= self.high_velocity
            && self.high_velocity <= 127
            && self.low_velocity >= 1
    }
}

/// A pad of the Pads face (format v36): its sample, and what the pad does to
/// it. A pad that names no sample is silent and stays in the list, so its
/// settings are kept while its file is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    /// The note it answers: the drum map's sound's input note, or one of the
    /// sixteen of a track with no map.
    pub note: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Id>,
    /// Level in decibels, from silence to +12.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level_db: f32,
    /// −1 is hard left, +1 hard right.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,
    /// Semitones, before the note's own distance from `root` is counted.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tune: f32,
    /// The instrument's output pair: 0 is the track's own output, 1 and up
    /// the auxiliary pairs (the owner's RT, §6.4).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub output: u32,
    /// How long the pad fades when it is choked or struck again, in seconds.
    #[serde(default = "default_choke")]
    pub choke_fade: f32,
    /// The key it plays at its own speed; the pad's own note unless moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<u8>,
}

fn default_choke() -> f32 {
    DEFAULT_CHOKE
}

impl Pad {
    /// An empty pad on `note` at its defaults: at unity, centred, no tune, on
    /// the track's own output.
    pub fn empty(note: u8) -> Self {
        Self {
            note,
            media: None,
            level_db: 0.0,
            pan: 0.0,
            tune: 0.0,
            output: 0,
            choke_fade: DEFAULT_CHOKE,
            root: None,
        }
    }

    /// The key it plays at its own speed.
    pub fn root(&self) -> u8 {
        self.root.unwrap_or(self.note)
    }

    /// A pad that plays `media` at its own speed.
    pub fn playing(note: u8, media: Id) -> Self {
        Self {
            media: Some(media),
            ..Self::empty(note)
        }
    }

    /// Whether its settings are inside what the model allows.
    pub fn valid(&self) -> bool {
        self.note <= 127
            && self.level_db.is_finite()
            && (LOWEST_LEVEL..=HIGHEST_LEVEL).contains(&self.level_db)
            && self.pan.is_finite()
            && (-1.0..=1.0).contains(&self.pan)
            && self.tune.is_finite()
            && (-MOST_TUNE..=MOST_TUNE).contains(&self.tune)
            && self.output <= PAIRS
            && self.choke_fade.is_finite()
            && (SHORTEST_CHOKE..=LONGEST_CHOKE).contains(&self.choke_fade)
    }
}

/// A sampler's sound: its face and what each face plays (format v36).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sampler {
    #[serde(default)]
    pub face: Face,
    #[serde(default)]
    pub one_shot: OneShot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<Pad>,
}

impl Sampler {
    /// An empty sound: nothing to play yet, and a pad for every sound of
    /// `map` (or the sixteen of its own when the track names none), so a
    /// sampler that arrives is ready for the pads it will be given.
    pub fn empty(map: Option<&DrumMap>) -> Self {
        let mut sound = Self::default();
        let notes = Self::pads_for(map);
        sound.keep_pads(&notes);
        sound
    }

    /// The pad of `note`, if the sampler has one.
    pub fn pad(&self, note: u8) -> Option<&Pad> {
        self.pads.iter().find(|p| p.note == note)
    }

    /// The pad of `note`, made at its defaults if it has none.
    pub fn pad_mut(&mut self, note: u8) -> &mut Pad {
        match self.pads.iter().position(|p| p.note == note) {
            Some(i) => &mut self.pads[i],
            None => {
                self.pads.push(Pad::empty(note));
                self.pads.sort_by_key(|p| p.note);
                let i = self
                    .pads
                    .iter()
                    .position(|p| p.note == note)
                    .expect("just added");
                &mut self.pads[i]
            }
        }
    }

    /// The zone that answers `key` at `velocity`: the first one whose ranges
    /// hold it, as a musical sampler layers them.
    pub fn zone(&self, key: u8, velocity: u8) -> Option<&Zone> {
        self.zones.iter().find(|z| z.holds(key, velocity))
    }

    /// Every media file the sampler plays, whichever face is showing: what
    /// the media pool counts as used.
    pub fn media(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self
            .one_shot
            .region
            .iter()
            .map(|region| region.media)
            .chain(self.zones.iter().map(|zone| zone.media))
            .chain(self.pads.iter().filter_map(|pad| pad.media))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Whether every pad the Pads face offers has a pad: the drum map's
    /// sounds when the track names one, sixteen of its own otherwise.
    pub fn pads_for(map: Option<&DrumMap>) -> Vec<u8> {
        match map {
            Some(map) => {
                let mut notes: Vec<u8> = map.sounds.iter().map(|s| s.input).collect();
                notes.sort_unstable();
                notes.dedup();
                notes
            }
            None => (FIRST_PAD..=LAST_PAD).collect(),
        }
    }

    /// Makes the pads `notes` expects exist, and drops the ones no longer
    /// offered (a map changed): the pads that stay keep their settings and
    /// their samples, so attaching or swapping a map never loses a sound.
    pub fn keep_pads(&mut self, notes: &[u8]) {
        self.pads.retain(|pad| notes.contains(&pad.note));
        for &note in notes {
            if self.pad(note).is_none() {
                self.pads.push(Pad::empty(note));
            }
        }
        self.pads.sort_by_key(|pad| pad.note);
    }

    /// Whether every region is in order, every range inside the note range,
    /// and every level and time one the model allows.
    pub fn valid(&self) -> bool {
        self.zones.len() <= MOST_ZONES
            && self.pads.len() <= MOST_PADS
            && self.zones.iter().all(Zone::valid)
            && self.pads.iter().all(Pad::valid)
            && self.one_shot.region.is_none_or(|r| r.root <= 127)
    }
}
