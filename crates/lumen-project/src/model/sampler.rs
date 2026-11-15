use super::*;

pub const SAMPLER: &str = "sampler";

pub const MOST_ZONES: usize = 128;

pub const MOST_PADS: usize = 128;

pub const FIRST_PAD: u8 = 36;
pub const LAST_PAD: u8 = FIRST_PAD + 15;

pub const MOST_TUNE: f32 = 24.0;

pub const LOWEST_LEVEL: f32 = -60.0;
pub const HIGHEST_LEVEL: f32 = 12.0;

pub const SHORTEST_CHOKE: f32 = 0.0005;
pub const LONGEST_CHOKE: f32 = 2.0;

pub const DEFAULT_CHOKE: f32 = 0.005;

pub const PAIRS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Face {
    #[default]
    OneShot,

    Zones,

    Pads,
}

impl Face {
    pub const ALL: [Face; 3] = [Face::OneShot, Face::Zones, Face::Pads];

    pub fn next(self) -> Face {
        match self {
            Face::OneShot => Face::Zones,
            Face::Zones => Face::Pads,
            Face::Pads => Face::OneShot,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Face::OneShot => "One shot",
            Face::Zones => "Zones",
            Face::Pads => "Pads",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub media: Id,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub start: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,

    pub root: u8,
}

impl Region {
    pub fn whole(media: Id, root: u8) -> Self {
        Self {
            media,
            start: 0,
            end: None,
            root,
        }
    }

    pub fn fits(&self, frames: u64) -> bool {
        let end = self.end.unwrap_or(frames);
        self.start < end && end <= frames.max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OneShot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub looping: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub media: Id,

    pub low: u8,
    pub high: u8,

    pub low_velocity: u8,
    pub high_velocity: u8,
    pub root: u8,
}

impl Zone {
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

    pub fn holds(&self, key: u8, velocity: u8) -> bool {
        (self.low..=self.high).contains(&key)
            && (self.low_velocity..=self.high_velocity).contains(&velocity)
    }

    pub fn valid(&self) -> bool {
        self.low <= self.high
            && self.high <= 127
            && self.low_velocity <= self.high_velocity
            && self.high_velocity <= 127
            && self.low_velocity >= 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub note: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Id>,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub level_db: f32,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub tune: f32,

    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub output: u32,

    #[serde(default = "default_choke")]
    pub choke_fade: f32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<u8>,
}

fn default_choke() -> f32 {
    DEFAULT_CHOKE
}

impl Pad {
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

    pub fn root(&self) -> u8 {
        self.root.unwrap_or(self.note)
    }

    pub fn playing(note: u8, media: Id) -> Self {
        Self {
            media: Some(media),
            ..Self::empty(note)
        }
    }

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
    pub fn empty(map: Option<&DrumMap>) -> Self {
        let mut sound = Self::default();
        let notes = Self::pads_for(map);
        sound.keep_pads(&notes);
        sound
    }

    pub fn pad(&self, note: u8) -> Option<&Pad> {
        self.pads.iter().find(|p| p.note == note)
    }

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

    pub fn zone(&self, key: u8, velocity: u8) -> Option<&Zone> {
        self.zones.iter().find(|z| z.holds(key, velocity))
    }

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

    pub fn keep_pads(&mut self, notes: &[u8]) {
        self.pads.retain(|pad| notes.contains(&pad.note));
        for &note in notes {
            if self.pad(note).is_none() {
                self.pads.push(Pad::empty(note));
            }
        }
        self.pads.sort_by_key(|pad| pad.note);
    }

    pub fn valid(&self) -> bool {
        self.zones.len() <= MOST_ZONES
            && self.pads.len() <= MOST_PADS
            && self.zones.iter().all(Zone::valid)
            && self.pads.iter().all(Pad::valid)
            && self.one_shot.region.is_none_or(|r| r.root <= 127)
    }
}
