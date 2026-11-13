//! A sampler's sound (M8 slice B5): loading files into it, choosing the face,
//! moving a region, a zone or a pad, and clearing it. Every one is a named
//! method that applies through the history as one undo step, so the face that
//! is built next calls these and nothing else.
//!
//! The files are decoded by `crate::builtin::samples` before they arrive here
//! (off the audio thread); a method here only records which media file the
//! project holds and what the sampler plays of it, never the audio itself.

use lumen_project::model::{DrumMap, Face, Id, OneShot, Pad, Region, Sampler, Zone};

use crate::builtin::samples::{self, Loaded};

use super::{Document, Refusal};

/// The note a one-shot's sample plays at its own speed when it is loaded:
/// the middle of the keyboard, where a melodic sample is played.
pub const ONE_SHOT_ROOT: u8 = 60;

/// The most zones a drop makes from files at once.
const MOST_LOADED: usize = lumen_project::model::MOST_ZONES;

// The zone and pad edits below are what the drawn face calls when it is built
// (M8, the face after this slice); the palette reaches the rest.
#[allow(dead_code, reason = "the drawn face calls these when it is built")]
impl Document {
    /// The track and device of the sampler `device`, if it is one.
    fn sampler(&self, device: Id) -> Option<(usize, usize)> {
        self.project()
            .tracks
            .iter()
            .enumerate()
            .find_map(|(t, track)| {
                let d = track.devices.iter().position(|d| d.id == device)?;
                track.devices[d].is_sampler().then_some((t, d))
            })
    }

    /// The drum map of the track the sampler `device` is on: its pads answer
    /// the map's sounds (M8 slice B5).
    pub fn sampler_map(&self, device: Id) -> Option<&DrumMap> {
        let (t, _) = self.sampler(device)?;
        self.project().drum_map_of(&self.project().tracks[t])
    }

    /// Which face `device` shows, and plays (SP A's One shot, Zones, Pads).
    pub fn set_sampler_face(&mut self, device: Id, face: Face) -> Result<(), Refusal> {
        self.edit_sampler(device, format!("Sampler: {}", face.name()), |held| {
            held.face = face;
        })
    }

    /// Cycles the face: what the one command in the palette does, so the
    /// behaviour is reachable before the drawn face exists.
    pub fn next_sampler_face(&mut self, device: Id) -> Result<Face, Refusal> {
        let held = self.sampler_sound(device).ok_or(Refusal::NoDevice)?;
        let next = held.face.next();
        self.set_sampler_face(device, next)?;
        Ok(next)
    }

    /// The sound `device` holds now, if it is a sampler.
    pub fn sampler_sound(&self, device: Id) -> Option<&Sampler> {
        let (t, d) = self.sampler(device)?;
        self.project().tracks[t].devices[d].sampler.as_ref()
    }

    /// Whether One shot loops, and whether it plays backwards.
    pub fn set_sampler_loop(&mut self, device: Id, looping: bool) -> Result<(), Refusal> {
        self.edit_sampler(device, "Loop the sample".to_owned(), |held| {
            held.one_shot.looping = looping;
        })
    }

    /// The same for reverse.
    pub fn set_sampler_reverse(&mut self, device: Id, reversed: bool) -> Result<(), Refusal> {
        self.edit_sampler(device, "Reverse the sample".to_owned(), |held| {
            held.one_shot.reversed = reversed;
        })
    }

    /// The part of its file One shot plays, in frames of the file, and the key
    /// it plays at its own speed.
    pub fn set_sampler_region(
        &mut self,
        device: Id,
        start: u64,
        end: Option<u64>,
        root: u8,
    ) -> Result<(), Refusal> {
        self.edit_sampler(device, "Move the sample's region".to_owned(), |held| {
            if let Some(region) = held.one_shot.region.as_mut() {
                region.start = start;
                region.end = end;
                region.root = root;
            } else if let Some(media) = held.pads.iter().find_map(|p| p.media) {
                // A device with no one-shot region yet: the first sample it
                // holds becomes one.
                held.one_shot.region = Some(Region {
                    media,
                    start,
                    end,
                    root,
                });
            }
        })
    }

    /// A zone: its key and velocity ranges, and the key its file plays at its
    /// own speed. `index` past the end adds one.
    pub fn set_sampler_zone(
        &mut self,
        device: Id,
        index: usize,
        zone: Zone,
    ) -> Result<(), Refusal> {
        self.edit_sampler(device, "Change a zone".to_owned(), |held| {
            if index < held.zones.len() {
                held.zones[index] = zone;
            } else if held.zones.len() < MOST_LOADED {
                held.zones.push(zone);
            }
        })
    }

    /// Takes zone `index` away.
    pub fn remove_sampler_zone(&mut self, device: Id, index: usize) -> Result<(), Refusal> {
        self.edit_sampler(device, "Remove a zone".to_owned(), |held| {
            if index < held.zones.len() {
                held.zones.remove(index);
            }
        })
    }

    /// A pad: its sample, level, pan, tune, output pair and choke fade.
    pub fn set_sampler_pad(&mut self, device: Id, pad: Pad) -> Result<(), Refusal> {
        self.edit_sampler(device, format!("Set pad {}", pad.note), |held| {
            let existing = held.pad_mut(pad.note);
            *existing = pad;
        })
    }

    /// Empties every face of `device`, keeping the file list: the samples stay
    /// in the media pool until they are unused and purged.
    pub fn clear_sampler(&mut self, device: Id) -> Result<(), Refusal> {
        self.edit_sampler(device, "Clear the sampler".to_owned(), |held| {
            held.one_shot = OneShot::default();
            held.zones.clear();
            for pad in &mut held.pads {
                *pad = Pad::empty(pad.note);
            }
        })
    }

    /// Joins `files`, just decoded, to the sampler `device` as its face's
    /// sound (M8 slice B5): one undo step that adds the media entries and what
    /// the face plays of them. Answers what became of each file, for a word to
    /// the person.
    pub fn add_samples(&mut self, device: Id, files: &[Loaded]) -> Result<usize, Refusal> {
        let (t, d) = self.sampler(device).ok_or(Refusal::NoDevice)?;
        if files.is_empty() {
            return Ok(0);
        }
        let map = self
            .project()
            .drum_map_of(&self.project().tracks[t])
            .cloned();
        let face = self.project().tracks[t].devices[d]
            .sampler
            .as_ref()
            .map_or(Face::OneShot, |held| held.face);
        let label = match files {
            [one] => format!("Load {}", one.name),
            many => format!("Load {} samples", many.len()),
        };
        // One new id per file that the project does not hold already.
        let _ = self.project().media.len();
        let fresh: Vec<(Id, &Loaded)> = files.iter().map(|file| (file.media, file)).collect();
        for (_, file) in &fresh {
            self.media_files.insert(file.media, file.path.clone());
        }
        let media: Vec<lumen_project::model::Media> = fresh
            .iter()
            .map(|(_, file)| samples::media_entry(file))
            .collect();
        self.history.apply(&label, None, |project| {
            let track = std::sync::Arc::make_mut(&mut project.tracks[t]);
            let device = &mut track.devices[d];
            let held = device.sampler.get_or_insert_with(Sampler::default);
            for entry in &media {
                if !project.media.iter().any(|m| m.id == entry.id) {
                    project.media.push(entry.clone());
                }
            }
            match face {
                Face::OneShot => {
                    // One file is the sample; several are its zones, which is
                    // the Zones face's rule, so the first is the one shot.
                    if let Some((_, file)) = fresh.first() {
                        let frames = file.source.frames;
                        held.one_shot.region = Some(Region {
                            media: file.media,
                            start: 0,
                            end: None,
                            root: ONE_SHOT_ROOT,
                        });
                        let _ = frames;
                    }
                }
                Face::Zones => {
                    let files: Vec<(Id, u64)> = fresh
                        .iter()
                        .take(MOST_LOADED)
                        .map(|(_, file)| (file.media, file.source.frames))
                        .collect();
                    add_zones(held, &files);
                }
                Face::Pads => {
                    let notes = Sampler::pads_for(map.as_ref());
                    let names: Vec<String> =
                        fresh.iter().map(|(_, file)| file.name.clone()).collect();
                    let named: Vec<(u8, Option<&str>)> = notes
                        .iter()
                        .map(|&note| {
                            let name = map
                                .as_ref()
                                .and_then(|map| map.sound(note))
                                .map(|sound| sound.name.as_str());
                            (note, name)
                        })
                        .collect();
                    let placed = samples::place_by_name_or_order(&named, &names);
                    for (note, index) in placed {
                        let (id, _) = fresh[index];
                        held.pad_mut(note).media = Some(id);
                    }
                }
            }
            let pads = Sampler::pads_for(map.as_ref());
            held.keep_pads(&pads);
        });
        Ok(fresh.len())
    }

    /// Applies `edit` to the sampler `device` as one undo step named `label`.
    fn edit_sampler(
        &mut self,
        device: Id,
        label: String,
        edit: impl FnOnce(&mut Sampler),
    ) -> Result<(), Refusal> {
        let (t, d) = self.sampler(device).ok_or(Refusal::NoDevice)?;
        let map = self
            .project()
            .drum_map_of(&self.project().tracks[t])
            .cloned();
        self.history.apply(&label, None, |project| {
            let track = std::sync::Arc::make_mut(&mut project.tracks[t]);
            let held = track.devices[d]
                .sampler
                .get_or_insert_with(Sampler::default);
            edit(held);
            let pads = Sampler::pads_for(map.as_ref());
            held.keep_pads(&pads);
        });
        Ok(())
    }
}

/// Lays `files` across the keyboard as zones, in order: each takes an equal
/// share of the keys and plays at its own speed from the middle of it.
fn add_zones(held: &mut Sampler, files: &[(Id, u64)]) {
    let count = files.len().max(1);
    let share = 128 / count;
    held.zones.clear();
    for (index, (media, _)) in files.iter().enumerate() {
        let low = (index * share) as u8;
        let high = if index + 1 == count {
            127
        } else {
            ((index + 1) * share - 1) as u8
        };
        let root = low + (high - low) / 2;
        held.zones.push(Zone {
            media: *media,
            low,
            high,
            low_velocity: 1,
            high_velocity: 127,
            root,
        });
    }
}
