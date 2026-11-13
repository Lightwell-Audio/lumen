//! What a sampler plays, built on the interface side and shared with the audio
//! thread as immutable data (M8 slice B5).
//!
//! A [`Sample`] is one decoded file at the rate the engine runs at, in
//! channel-major frames; a [`Sound`] is what a device plays: the face it was
//! left showing and the regions, zones and pads that face uses. Neither is
//! ever changed once made: a project edit builds a new `Sound` and the engine
//! is handed it, so a note already sounding keeps the data it started with
//! (each voice holds its own handle), and nothing is copied while it plays.
//!
//! The buffers belong to the interface's library for as long as they are
//! loaded, so the audio thread is never the last owner of one; see
//! `crate::sampler::Sampler` and the allocation-guard test.

use std::sync::Arc;

/// One decoded sample at the engine's rate.
///
/// Frames are channel-major: `left` then `right` for a stereo file, `left`
/// alone for a mono one (which plays on both sides).
#[derive(Debug, PartialEq)]
pub struct Sample {
    rate: u32,
    frames: usize,
    channels: u8,
    data: Vec<f32>,
}

impl Sample {
    /// A sample of `channels` (1 or 2) channels at `rate`, from channel-major
    /// `data` of `frames` frames each. `None` when the two do not agree.
    pub fn new(rate: u32, channels: u8, frames: usize, data: Vec<f32>) -> Option<Self> {
        let channels = channels.clamp(1, 2);
        (rate > 0 && data.len() == frames * usize::from(channels)).then_some(Self {
            rate,
            frames,
            channels,
            data,
        })
    }

    /// A mono sample.
    pub fn mono(rate: u32, data: Vec<f32>) -> Self {
        let frames = data.len();
        Self {
            rate,
            frames,
            channels: 1,
            data,
        }
    }

    /// The sample rate its frames are at.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// How many frames it holds.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// 1 or 2.
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Whether it holds sound at all: a one-frame click is a hit, an empty
    /// file is not.
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }

    /// Frame `frame` of `channel`, or silence outside the file.
    #[inline]
    pub fn at(&self, channel: usize, frame: isize) -> f32 {
        if frame < 0 || frame as usize >= self.frames {
            return 0.0;
        }
        let channel = channel.min(usize::from(self.channels) - 1);
        let index = channel * self.frames + frame as usize;
        self.data.get(index).copied().unwrap_or(0.0)
    }

    /// How many bytes the frames take, for a memory report.
    pub fn memory(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }
}

/// One region of a sample: the part of it a voice plays, at the rate it was
/// decoded to, and where the speed it plays at its own pitch is counted from.
#[derive(Debug, Clone)]
pub struct Region {
    pub sample: Arc<Sample>,
    /// The first frame it plays, and the one past its last.
    pub start: usize,
    pub end: usize,
    /// The MIDI key that plays it at its own speed.
    pub root: u8,
    pub looping: bool,
    pub reversed: bool,
}

impl Region {
    /// The whole of `sample` on `root`.
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

    /// Frames of the region.
    pub fn length(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether it holds anything to play.
    pub fn plays(&self) -> bool {
        self.length() > 0
    }
}

/// A zone of the Zones face: the keys and velocities a region answers.
#[derive(Debug, Clone)]
pub struct Zone {
    pub region: Region,
    pub low: u8,
    pub high: u8,
    pub low_velocity: u8,
    pub high_velocity: u8,
}

impl Zone {
    /// Whether it answers `key` played at `velocity`.
    pub fn holds(&self, key: u8, velocity: u8) -> bool {
        (self.low..=self.high).contains(&key)
            && (self.low_velocity..=self.high_velocity).contains(&velocity)
    }
}

/// A pad of the Pads face: its region and what the pad does to it.
#[derive(Debug, Clone)]
pub struct Pad {
    /// The note it answers.
    pub note: u8,
    pub region: Option<Region>,
    /// Linear gain, from the pad's level in decibels.
    pub gain: f32,
    /// −1 hard left, +1 hard right.
    pub pan: f32,
    /// Semitones, before the note's distance from the region's root is
    /// counted.
    pub tune: f32,
    /// The instrument's output pair: 0 is the track's own, 1 and up auxiliary.
    pub output: u32,
    /// How long it fades when it is choked or struck again, in seconds.
    pub choke_fade: f32,
    /// The group that chokes it, when the track's drum map puts it in one.
    pub choke: Option<u8>,
}

impl Pad {
    /// A pad on `note` that plays `region` at unity, centred, untuned, on the
    /// track's own output.
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

/// Which of the three faces plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    /// One region, transposed from its root.
    #[default]
    OneShot,
    /// The zone whose ranges hold the note.
    Zones,
    /// The pad of the note.
    Pads,
}

/// What a sampler plays: its face and that face's regions (M8 slice B5).
#[derive(Debug, Clone, Default)]
pub struct Sound {
    pub face: Face,
    /// The One shot face's region.
    pub one_shot: Option<Region>,
    /// The Zones face's zones, in the order they were added (the first that
    /// holds a note plays it).
    pub zones: Vec<Zone>,
    /// The Pads face's pads, by note.
    pub pads: Vec<Pad>,
}

impl Sound {
    /// Whether it has anything to play at all.
    pub fn is_silent(&self) -> bool {
        self.one_shot.as_ref().is_none_or(|r| !r.plays())
            && !self.zones.iter().any(|z| z.region.plays())
            && !self
                .pads
                .iter()
                .any(|p| p.region.as_ref().is_some_and(Region::plays))
    }

    /// The pad of `note`, if it has one.
    pub fn pad(&self, note: u8) -> Option<&Pad> {
        self.pads.iter().find(|p| p.note == note)
    }

    /// What a note sounds: the region it plays, and the pad it is, when the
    /// face has pads. `None` when nothing answers it.
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

    /// Every pad in `group`, which a new hit of one of them chokes.
    pub(crate) fn choked_by(&self, group: u8) -> impl Iterator<Item = u8> + '_ {
        self.pads
            .iter()
            .filter(move |pad| pad.choke == Some(group))
            .map(|pad| pad.note)
    }
}

/// What one note plays: the region, and how the pad it is shapes it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Source<'a> {
    pub region: &'a Region,
    pub gain: f32,
    pub pan: f32,
    pub tune: f32,
    pub output: u32,
    /// The pad's choke fade, when the note is a pad's.
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
        // Mono reads on both sides; a stereo file keeps them apart.
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
        // One shot plays its region for any note.
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
