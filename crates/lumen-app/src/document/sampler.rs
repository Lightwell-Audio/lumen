use lumen_project::model::{DrumMap, Face, Id, OneShot, Pad, Region, Sampler, Track, Zone};

use crate::builtin::samples::{self, Loaded};

use super::{Document, Refusal};

pub const ONE_SHOT_ROOT: u8 = 60;

const MOST_LOADED: usize = lumen_project::model::MOST_ZONES;

#[allow(dead_code, reason = "the drawn face calls these when it is built")]
impl Document {
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

    pub fn sampler_map(&self, device: Id) -> Option<&DrumMap> {
        let (t, _) = self.sampler(device)?;
        self.project().drum_map_of(&self.project().tracks[t])
    }

    pub fn set_sampler_face(&mut self, device: Id, face: Face) -> Result<(), Refusal> {
        self.edit_sampler(device, format!("Sampler: {}", face.name()), |held| {
            held.face = face;
        })
    }

    pub fn next_sampler_face(&mut self, device: Id) -> Result<Face, Refusal> {
        let held = self.sampler_sound(device).ok_or(Refusal::NoDevice)?;
        let next = held.face.next();
        self.set_sampler_face(device, next)?;
        Ok(next)
    }

    pub fn sampler_sound(&self, device: Id) -> Option<&Sampler> {
        let (t, d) = self.sampler(device)?;
        self.project().tracks[t].devices[d].sampler.as_ref()
    }

    pub fn set_sampler_loop(&mut self, device: Id, looping: bool) -> Result<(), Refusal> {
        self.edit_sampler(device, "Loop the sample".to_owned(), |held| {
            held.one_shot.looping = looping;
        })
    }

    pub fn set_sampler_reverse(&mut self, device: Id, reversed: bool) -> Result<(), Refusal> {
        self.edit_sampler(device, "Reverse the sample".to_owned(), |held| {
            held.one_shot.reversed = reversed;
        })
    }

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
                held.one_shot.region = Some(Region {
                    media,
                    start,
                    end,
                    root,
                });
            }
        })
    }

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

    pub fn remove_sampler_zone(&mut self, device: Id, index: usize) -> Result<(), Refusal> {
        self.edit_sampler(device, "Remove a zone".to_owned(), |held| {
            if index < held.zones.len() {
                held.zones.remove(index);
            }
        })
    }

    pub fn set_sampler_pad(&mut self, device: Id, pad: Pad) -> Result<(), Refusal> {
        self.edit_sampler(device, format!("Set pad {}", pad.note), |held| {
            let existing = held.pad_mut(pad.note);
            *existing = pad;
        })
    }

    pub fn clear_sampler(&mut self, device: Id) -> Result<(), Refusal> {
        self.edit_sampler(device, "Clear the sampler".to_owned(), |held| {
            held.one_shot = OneShot::default();
            held.zones.clear();
            for pad in &mut held.pads {
                *pad = Pad::empty(pad.note);
            }
        })
    }

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
        let fresh: Vec<(Id, &Loaded)> = files.iter().map(|file| (file.media, file)).collect();

        let used = match face {
            Face::OneShot => fresh.len().min(1),
            Face::Zones => fresh.len().min(MOST_LOADED),
            Face::Pads => fresh.len(),
        };
        for (_, file) in &fresh[..used] {
            self.media_files.insert(file.media, file.path.clone());
        }
        let media: Vec<lumen_project::model::Media> = fresh[..used]
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
                    if let Some((_, file)) = fresh.first() {
                        held.one_shot.region = Some(Region {
                            media: file.media,
                            start: 0,
                            end: None,
                            root: ONE_SHOT_ROOT,
                        });
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
        Ok(used)
    }

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

pub(crate) fn follow_drum_map(track: &mut Track, map: Option<&DrumMap>) {
    let pads = Sampler::pads_for(map);
    for device in &mut track.devices {
        let sampler = device.is_sampler();
        if let Some(held) = device.sampler.as_mut().filter(|_| sampler) {
            held.keep_pads(&pads);
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Slot};
    use lumen_devices::sampler::Sample;
    use lumen_project::model::{PluginRef, SAMPLER, TrackKind};
    use std::sync::Arc;

    fn loaded(id: u64, name: &str, frames: u64) -> Loaded {
        Loaded {
            media: Id(id),
            path: std::path::PathBuf::from(format!("Samples/{name}.wav")),
            name: name.to_owned(),
            source: crate::builtin::samples::Source {
                sample: Arc::new(Sample::mono(48_000, vec![0.1; frames as usize])),
                file_rate: 48_000,
                frames,
            },
            size_bytes: frames * 4,
        }
    }

    fn with_sampler() -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let device = doc
            .add_device(
                track,
                PluginRef::Builtin {
                    name: SAMPLER.to_owned(),
                },
                Slot::End,
            )
            .expect("the sampler goes on a MIDI track");
        (doc, track, device)
    }

    #[test]
    fn loading_a_sample_is_one_undo_step_and_puts_the_file_in_the_project() {
        let (mut doc, _track, device) = with_sampler();
        let before = doc.project().clone();
        let steps = doc.history().steps().0.len();
        doc.add_samples(device, &[loaded(50, "kick", 4_800)])
            .expect("it loads");
        assert_eq!(doc.history().steps().0.len(), steps + 1, "one step");
        assert_eq!(doc.project().media.len(), 1);
        assert!(doc.media_files.contains_key(&Id(50)));
        let held = doc.sampler_sound(device).expect("the sound");
        let region = held.one_shot.region.expect("the region");
        assert_eq!(region.media, Id(50));
        assert_eq!(region.root, ONE_SHOT_ROOT);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        assert!(doc.undo().is_some());
        assert_eq!(doc.project(), &before, "one step back is before the load");
        assert!(doc.sampler_sound(device).unwrap().one_shot.region.is_none());
    }

    #[test]
    fn one_shot_keeps_the_first_file_and_leaves_the_rest_out_of_the_project() {
        let (mut doc, _, device) = with_sampler();
        let files: Vec<Loaded> = (0..3)
            .map(|k| loaded(60 + k, &format!("take{k}"), 100))
            .collect();
        let used = doc.add_samples(device, &files).expect("it loads");
        assert_eq!(used, 1, "one file is the sample");
        let held = doc.sampler_sound(device).unwrap();
        assert_eq!(held.one_shot.region.map(|r| r.media), Some(Id(60)));
        assert_eq!(doc.project().media.len(), 1, "only the one it plays");
        assert_eq!(doc.project().media[0].id, Id(60));
        assert!(!doc.media_files.contains_key(&Id(61)));
        assert!(!doc.media_files.contains_key(&Id(62)));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_face_decides_what_a_drop_becomes() {
        let (mut doc, _, device) = with_sampler();

        doc.set_sampler_face(device, Face::Zones).unwrap();
        let n = doc
            .add_samples(device, &[loaded(1, "low", 100), loaded(2, "high", 100)])
            .unwrap();
        assert_eq!(n, 2);
        let held = doc.sampler_sound(device).unwrap().clone();
        assert_eq!(held.zones.len(), 2);
        assert_eq!((held.zones[0].low, held.zones[0].high), (0, 63));
        assert_eq!((held.zones[1].low, held.zones[1].high), (64, 127));
        assert_eq!(held.zones[1].root, 95);

        doc.set_sampler_face(device, Face::Pads).unwrap();
        doc.add_samples(device, &[loaded(3, "a", 100), loaded(4, "b", 100)])
            .unwrap();
        let held = doc.sampler_sound(device).unwrap();
        assert_eq!(held.pad(36).and_then(|p| p.media), Some(Id(3)));
        assert_eq!(held.pad(37).and_then(|p| p.media), Some(Id(4)));
        assert_eq!(held.pad(38).and_then(|p| p.media), None);
        assert_eq!(held.pads.len(), 16, "the sixteen pads are there");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn zones_share_the_keyboard_and_play_from_their_middles() {
        let (mut doc, _, device) = with_sampler();
        doc.set_sampler_face(device, Face::Zones).unwrap();
        let files: Vec<Loaded> = (0..3)
            .map(|k| loaded(10 + k, &format!("s{k}"), 100))
            .collect();
        doc.add_samples(device, &files).unwrap();
        let held = doc.sampler_sound(device).unwrap().clone();
        let ranges: Vec<(u8, u8, u8, Id)> = held
            .zones
            .iter()
            .map(|zone| (zone.low, zone.high, zone.root, zone.media))
            .collect();

        assert_eq!(
            ranges,
            [
                (0, 41, 20, Id(10)),
                (42, 83, 62, Id(11)),
                (84, 127, 105, Id(12)),
            ]
        );

        for key in 0..=127u8 {
            assert_eq!(
                held.zones.iter().filter(|z| z.holds(key, 100)).count(),
                1,
                "key {key}"
            );
        }

        for (count, want) in [
            (2usize, vec![(0u8, 63u8, 31u8), (64, 127, 95)]),
            (1, vec![(0, 127, 63)]),
        ] {
            let (mut doc, _, device) = with_sampler();
            doc.set_sampler_face(device, Face::Zones).unwrap();
            let files: Vec<Loaded> = (0..count)
                .map(|k| loaded(20 + k as u64, &format!("t{k}"), 100))
                .collect();
            doc.add_samples(device, &files).unwrap();
            let held = doc.sampler_sound(device).unwrap().clone();
            let got: Vec<(u8, u8, u8)> = held
                .zones
                .iter()
                .map(|zone| (zone.low, zone.high, zone.root))
                .collect();
            assert_eq!(got, want, "{count} files");
        }
    }

    #[test]
    fn a_map_attached_after_the_sampler_gives_it_its_pads() {
        let (mut doc, track, device) = with_sampler();
        doc.set_sampler_face(device, Face::Pads).unwrap();
        doc.add_samples(device, &[loaded(1, "kick", 100)]).unwrap();
        assert_eq!(
            doc.sampler_sound(device).unwrap().pad(36).unwrap().media,
            Some(Id(1))
        );
        assert_eq!(doc.sampler_sound(device).unwrap().pads.len(), 16);

        let steps = doc.history().steps().0.len();
        doc.attach_drum_map_preset(track, lumen_project::drums::presets::Preset::GENERAL_MIDI)
            .expect("the track takes the map");
        assert_eq!(doc.history().steps().0.len(), steps + 1, "one undo step");
        let held = doc.sampler_sound(device).unwrap();

        assert_eq!(held.pads.len(), 47);
        assert_eq!(held.pad(36).and_then(|p| p.media), Some(Id(1)));

        assert!(held.pad(99).is_none());
        assert!(held.pad(42).is_some(), "the closed hat's note");

        doc.detach_drum_map(track);
        let held = doc.sampler_sound(device).unwrap();
        assert_eq!(held.pads.len(), 16);
        assert_eq!(held.pad(36).and_then(|p| p.media), Some(Id(1)));
    }

    #[test]
    fn a_maps_sounds_name_the_pads_a_drop_fills() {
        let (mut doc, track, device) = with_sampler();
        doc.attach_drum_map_preset(track, lumen_project::drums::presets::Preset::EIGHT_OH_EIGHT);
        let map = doc.drum_map_of(track).expect("the map").sounds.len();
        doc.set_sampler_face(device, Face::Pads).unwrap();
        doc.add_samples(
            device,
            &[loaded(1, "Kick 808", 100), loaded(2, "Snare", 100)],
        )
        .unwrap();
        let held = doc.sampler_sound(device).unwrap().clone();
        assert_eq!(held.pad(36).and_then(|p| p.media), Some(Id(1)));
        assert_eq!(held.pad(38).and_then(|p| p.media), Some(Id(2)));
        assert_eq!(held.pads.len(), map);
    }

    #[test]
    fn every_sampler_edit_is_one_named_undo_step() {
        let (mut doc, _, device) = with_sampler();
        doc.add_samples(device, &[loaded(1, "one", 100)]).unwrap();
        let steps = doc.history().steps().0.len();
        doc.next_sampler_face(device).unwrap();
        assert_eq!(doc.history().steps().0.len(), steps + 1);
        assert_eq!(doc.history().undo_label(), Some("Sampler: Zones"));
        doc.set_sampler_loop(device, true).unwrap();
        doc.set_sampler_reverse(device, true).unwrap();
        doc.set_sampler_region(device, 10, Some(20), 62).unwrap();
        doc.set_sampler_face(device, Face::Pads).unwrap();
        let mut pad = Pad::playing(36, Id(1));
        pad.level_db = -6.0;
        pad.output = 2;
        doc.set_sampler_pad(device, pad).unwrap();
        assert_eq!(doc.history().steps().0.len(), steps + 6);
        let held = doc.sampler_sound(device).unwrap();
        assert!(held.one_shot.looping && held.one_shot.reversed);
        let region = held.one_shot.region.unwrap();
        assert_eq!((region.start, region.end, region.root), (10, Some(20), 62));
        let pad = held.pad(36).unwrap();
        assert_eq!((pad.level_db, pad.output), (-6.0, 2));
        doc.clear_sampler(device).unwrap();
        let held = doc.sampler_sound(device).unwrap();
        assert!(held.one_shot.region.is_none());
        assert!(held.pads.iter().all(|p| p.media.is_none()));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_zone_is_added_moved_and_removed() {
        let (mut doc, _, device) = with_sampler();
        doc.add_samples(device, &[loaded(1, "one", 100)]).unwrap();
        let zone = Zone::single(Id(1), 60, 60);
        doc.set_sampler_zone(device, 0, zone).unwrap();
        assert_eq!(doc.sampler_sound(device).unwrap().zones, [zone]);
        doc.set_sampler_zone(
            device,
            0,
            Zone {
                low: 10,
                high: 20,
                ..zone
            },
        )
        .unwrap();
        assert_eq!(doc.sampler_sound(device).unwrap().zones[0].low, 10);
        doc.remove_sampler_zone(device, 0).unwrap();
        assert!(doc.sampler_sound(device).unwrap().zones.is_empty());

        let before = doc.project().clone();
        doc.remove_sampler_zone(device, 3).unwrap();
        assert_eq!(doc.project(), &before);
    }

    #[test]
    fn only_a_sampler_takes_a_sampler_edit() {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        doc.add_device_with(
            track,
            PluginRef::Builtin {
                name: "utility".to_owned(),
            },
            &[],
            Slot::End,
        )
        .expect("an effect goes on an audio track");
        assert_eq!(
            doc.set_sampler_face(Id(400), Face::Zones),
            Err(Refusal::NoDevice)
        );
        assert!(doc.sampler_sound(Id(400)).is_none());
    }
}
