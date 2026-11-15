use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub struct Sample {
    rate: u32,
    frames: usize,
    channels: u8,
    data: Vec<f32>,
}

impl Sample {
    pub fn new(rate: u32, channels: u8, frames: usize, data: Vec<f32>) -> Option<Self> {
        let channels = channels.clamp(1, 2);
        (rate > 0 && data.len() == frames * usize::from(channels)).then_some(Self {
            rate,
            frames,
            channels,
            data,
        })
    }

    pub fn mono(rate: u32, data: Vec<f32>) -> Self {
        let frames = data.len();
        Self {
            rate,
            frames,
            channels: 1,
            data,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn channels(&self) -> u8 {
        self.channels
    }

    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    #[inline]
    pub fn at(&self, channel: usize, frame: isize) -> f32 {
        if frame < 0 || frame as usize >= self.frames {
            return 0.0;
        }
        let channel = channel.min(usize::from(self.channels) - 1);
        let index = channel * self.frames + frame as usize;
        self.data.get(index).copied().unwrap_or(0.0)
    }

    pub fn memory(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }
}

#[derive(Debug, Clone)]
pub struct Region {
    pub sample: Arc<Sample>,

    pub start: usize,
    pub end: usize,

    pub root: u8,
    pub looping: bool,
    pub reversed: bool,
}

impl Region {
    pub fn whole(sample: Arc<Sample>, root: u8) -> Self {
        let end = sample.frames();
        Self {
            sample,
            start: 0,
            end,
            root,
            looping: false,
            reversed: false,
        }
    }

    pub fn length(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn plays(&self) -> bool {
        self.length() > 0
    }
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub region: Region,
    pub low: u8,
    pub high: u8,
    pub low_velocity: u8,
    pub high_velocity: u8,
}

impl Zone {
    pub fn holds(&self, key: u8, velocity: u8) -> bool {
        (self.low..=self.high).contains(&key)
            && (self.low_velocity..=self.high_velocity).contains(&velocity)
    }
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub note: u8,
    pub region: Option<Region>,

    pub gain: f32,

    pub pan: f32,

    pub tune: f32,

    pub output: u32,

    pub choke_fade: f32,

    pub choke: Option<u8>,
}

impl Pad {
    pub fn new(note: u8, region: Option<Region>) -> Self {
        Self {
            note,
            region,
            gain: 1.0,
            pan: 0.0,
            tune: 0.0,
            output: 0,
            choke_fade: crate::sampler::DEFAULT_CHOKE,
            choke: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    #[default]
    OneShot,

    Zones,

    Pads,
}

#[derive(Debug, Clone, Default)]
pub struct Sound {
    pub face: Face,

    pub one_shot: Option<Region>,

    pub zones: Vec<Zone>,

    pub pads: Vec<Pad>,
}

impl Sound {
    pub fn is_silent(&self) -> bool {
        self.one_shot.as_ref().is_none_or(|r| !r.plays())
            && !self.zones.iter().any(|z| z.region.plays())
            && !self
                .pads
                .iter()
                .any(|p| p.region.as_ref().is_some_and(Region::plays))
    }

    pub fn pad(&self, note: u8) -> Option<&Pad> {
        self.pads.iter().find(|p| p.note == note)
    }

    pub(crate) fn voice_for(&self, key: u8, velocity: u8) -> Option<Source<'_>> {
        match self.face {
            Face::OneShot => self
                .one_shot
                .as_ref()
                .filter(|r| r.plays())
                .map(|region| Source {
                    region,
                    gain: 1.0,
                    pan: 0.0,
                    tune: 0.0,
                    output: 0,
                    choke_fade: None,
                    choke: None,
                }),
            Face::Zones => self
                .zones
                .iter()
                .find(|zone| zone.holds(key, velocity))
                .filter(|zone| zone.region.plays())
                .map(|zone| Source {
                    region: &zone.region,
                    gain: 1.0,
                    pan: 0.0,
                    tune: 0.0,
                    output: 0,
                    choke_fade: None,
                    choke: None,
                }),
            Face::Pads => self.pad(key).and_then(|pad| {
                pad.region
                    .as_ref()
                    .filter(|r| r.plays())
                    .map(|region| Source {
                        region,
                        gain: pad.gain,
                        pan: pad.pan,
                        tune: pad.tune,
                        output: pad.output,
                        choke_fade: Some(pad.choke_fade),
                        choke: pad.choke,
                    })
            }),
        }
    }

    pub(crate) fn choked_by(&self, group: u8) -> impl Iterator<Item = u8> + '_ {
        self.pads
            .iter()
            .filter(move |pad| pad.choke == Some(group))
            .map(|pad| pad.note)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Source<'a> {
    pub region: &'a Region,
    pub gain: f32,
    pub pan: f32,
    pub tune: f32,
    pub output: u32,

    pub choke_fade: Option<f32>,
    pub choke: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(frames: usize) -> Arc<Sample> {
        Arc::new(Sample::mono(
            48_000,
            (0..frames).map(|k| k as f32 / frames as f32).collect(),
        ))
    }

    #[test]
    fn a_sample_reads_its_frames_and_nothing_outside_them() {
        let one = Sample::mono(48_000, vec![0.5, -0.5]);
        assert_eq!(one.at(0, 0), 0.5);
        assert_eq!(one.at(0, 1), -0.5);
        assert_eq!(one.at(0, 2), 0.0, "past the end");
        assert_eq!(one.at(0, -1), 0.0, "before the start");

        assert_eq!(one.at(1, 1), -0.5);
        let two = Sample::new(48_000, 2, 2, vec![1.0, 2.0, -1.0, -2.0]).unwrap();
        assert_eq!((two.at(0, 1), two.at(1, 0)), (2.0, -1.0));
        assert!(Sample::new(48_000, 2, 3, vec![0.0; 4]).is_none());
    }

    #[test]
    fn a_region_knows_what_it_plays() {
        let region = Region {
            start: 2,
            end: 5,
            ..Region::whole(sample(8), 60)
        };
        assert_eq!(region.length(), 3);
        assert!(region.plays());
        let empty = Region {
            start: 4,
            end: 4,
            ..Region::whole(sample(8), 60)
        };
        assert!(!empty.plays());
    }

    #[test]
    fn the_face_decides_which_region_a_note_plays() {
        let mut sound = Sound {
            one_shot: Some(Region::whole(sample(4), 60)),
            ..Sound::default()
        };
        sound.zones.push(Zone {
            region: Region::whole(sample(8), 40),
            low: 36,
            high: 47,
            low_velocity: 1,
            high_velocity: 127,
        });
        sound
            .pads
            .push(Pad::new(36, Some(Region::whole(sample(16), 36))));

        assert_eq!(sound.voice_for(36, 100).unwrap().region.root, 60);
        sound.face = Face::Zones;
        assert_eq!(sound.voice_for(36, 100).unwrap().region.root, 40);
        assert!(sound.voice_for(60, 100).is_none());
        sound.face = Face::Pads;
        assert_eq!(sound.voice_for(36, 100).unwrap().region.root, 36);
        assert!(sound.voice_for(37, 100).is_none(), "no pad on 37");
    }

    #[test]
    fn a_choke_group_names_its_pads() {
        let mut sound = Sound::default();
        let mut closed = Pad::new(42, None);
        closed.choke = Some(1);
        let mut open = Pad::new(46, None);
        open.choke = Some(1);
        sound.pads = vec![closed, open, Pad::new(36, None)];
        let notes: Vec<u8> = sound.choked_by(1).collect();
        assert_eq!(notes, [42, 46]);
        assert_eq!(sound.choked_by(2).count(), 0);
    }

    #[test]
    fn an_empty_sound_is_silent() {
        assert!(Sound::default().is_silent());
        let mut sound = Sound::default();
        sound.pads.push(Pad::new(36, None));
        assert!(sound.is_silent(), "a pad with no sample plays nothing");
    }
}
