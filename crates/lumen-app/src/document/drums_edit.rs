//! The drum map's own edits (M8 slice D1): the fields the map editor changes
//! in place that D0 did not need to name, plus the copy "Make unique" makes
//! and the list an import adds to.
//!
//! Every method is one undo step, like D0's (`document/drums.rs`). A sound is
//! found by its **input** note, as D0's methods find it; [`set_sound_input`]
//! is the one that moves that key, and refuses a note another sound already
//! answers to, so a map's input notes stay unique (the model's rule).

use std::sync::Arc;

use lumen_project::model::{Color, DrumMap, Id, Sound, TrackKind};

use super::Document;

impl Document {
    /// Moves the sound answering to `from` onto the note `to`. One undo step;
    /// false when there is no such sound, when another one already answers to
    /// `to`, or when `to` is past 127.
    pub fn set_sound_input(&mut self, map: Id, from: u8, to: u8) -> bool {
        if to > 127 {
            return false;
        }
        let Some(held) = self.drum_map(map) else {
            return false;
        };
        if held.sound(from).is_none() || (from != to && held.sound(to).is_some()) {
            return false;
        }
        if from == to {
            return false;
        }
        self.history
            .apply("Change the drum sound's note", None, move |project| {
                if let Some(m) = project.drum_maps.iter_mut().find(|m| m.id == map)
                    && let Some(sound) = m.sounds.iter_mut().find(|s| s.input == from)
                {
                    sound.input = to;
                }
            });
        true
    }

    /// Sets a sound's colour in its map. One undo step; false when the map or
    /// the sound is not there.
    pub fn set_sound_color(&mut self, map: Id, input: u8, color: Color) -> bool {
        self.edit_sound(map, input, "Change the drum sound's colour", |sound| {
            sound.color = color;
        })
    }

    /// Adds `sound` to the map, after the sound answering to `after` when it
    /// is given (else at the end, where the lanes draw it). One undo step;
    /// false when the map is not there or another sound answers to the note.
    pub fn add_drum_sound(&mut self, map: Id, sound: Sound, after: Option<u8>) -> bool {
        let Some(held) = self.drum_map(map) else {
            return false;
        };
        if held.sound(sound.input).is_some() {
            return false;
        }
        self.history
            .apply("Add a drum sound", None, move |project| {
                if let Some(m) = project.drum_maps.iter_mut().find(|m| m.id == map) {
                    let at = after
                        .and_then(|note| {
                            m.sounds.iter().position(|s| s.input == note).map(|i| i + 1)
                        })
                        .unwrap_or(m.sounds.len());
                    m.sounds.insert(at.min(m.sounds.len()), sound);
                }
            });
        true
    }

    /// Takes a sound out of the map: its clips keep their notes, which then
    /// play as written (nothing is destructive here). One undo step.
    pub fn remove_drum_sound(&mut self, map: Id, input: u8) -> bool {
        let present = self.drum_map(map).is_some_and(|m| m.sound(input).is_some());
        if !present {
            return false;
        }
        self.history
            .apply("Remove a drum sound", None, move |project| {
                if let Some(m) = project.drum_maps.iter_mut().find(|m| m.id == map) {
                    m.sounds.retain(|s| s.input != input);
                }
            });
        true
    }

    /// Moves the sound at `from` in the map's order to `to` (the lanes' and
    /// the editor's order). One undo step; false when either is past the end
    /// or the move changes nothing.
    pub fn move_drum_sound(&mut self, map: Id, from: usize, to: usize) -> bool {
        let Some(held) = self.drum_map(map) else {
            return false;
        };
        if from >= held.sounds.len() || to >= held.sounds.len() || from == to {
            return false;
        }
        self.history
            .apply("Move a drum sound", None, move |project| {
                if let Some(m) = project.drum_maps.iter_mut().find(|m| m.id == map)
                    && from < m.sounds.len()
                    && to < m.sounds.len()
                {
                    let sound = m.sounds.remove(from);
                    m.sounds.insert(to, sound);
                }
            });
        true
    }

    /// Gives `track` a copy of the map it names, with its own id, so edits to
    /// it no longer reach the tracks that share the original (the owner's
    /// "Make unique"). One undo step; `None` when the track has no map.
    pub fn make_drum_map_unique(&mut self, track: Id) -> Option<Id> {
        let held = self
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| self.project().drum_map_of(t))
            .cloned()?;
        let id = self.fresh_id();
        let name = format!("{} copy", held.name);
        let name: String = name.chars().take(64).collect();
        self.history
            .apply("Make the drum map unique", None, move |project| {
                let map = DrumMap::new(id, name, held.sounds);
                project.drum_maps.push(map.clone());
                if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == track) {
                    let track = Arc::make_mut(slot);
                    track.drum_map = Some(id);
                    // The copy has the same sounds, and the pads follow it in
                    // this same step (M8 slice B5, review M1).
                    super::sampler::follow_drum_map(track, Some(&map));
                }
            });
        Some(id)
    }

    /// Adds `map` to the project under a new id, and gives it to every MIDI
    /// track in `tracks` (an import: the file's own id means nothing here).
    /// One undo step; gives the map's new id.
    pub fn import_drum_map(&mut self, map: DrumMap, tracks: &[Id]) -> Id {
        let id = self.fresh_id();
        let targets: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| {
                self.project()
                    .tracks
                    .iter()
                    .any(|t| t.id == *id && t.kind == TrackKind::Midi)
            })
            .collect();
        let mut map = map;
        map.id = id;
        self.history
            .apply("Import a drum map", None, move |project| {
                project.drum_maps.push(map.clone());
                for track in &targets {
                    if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) {
                        let track = Arc::make_mut(slot);
                        track.drum_map = Some(id);
                        // A sampler on the track gives its pads the imported
                        // map's sounds, in this same step (M8 slice B5, M1).
                        super::sampler::follow_drum_map(track, Some(&map));
                    }
                }
            });
        id
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::grid::Size;
    use lumen_core::time::Ticks;
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::{DrumIcon, Quantize, TrackKind};

    use super::*;

    /// A document with one MIDI track on the 808-style map.
    fn kit() -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let map = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .expect("a MIDI track takes a map");
        doc.seal();
        (doc, track, map)
    }

    fn sound(doc: &Document, map: Id, input: u8) -> Option<Sound> {
        doc.drum_map(map)?.sound(input).cloned()
    }

    #[test]
    fn every_field_of_a_sound_is_one_undo_step() {
        let (mut doc, _track, map) = kit();
        assert!(doc.set_sound_color(map, 36, Color::in_turn(3)));
        assert_eq!(
            sound(&doc, map, 36).map(|s| s.color),
            Some(Color::in_turn(3))
        );
        assert_eq!(
            doc.undo().as_deref(),
            Some("Change the drum sound's colour")
        );
        assert_eq!(
            doc.redo().as_deref(),
            Some("Change the drum sound's colour")
        );
        // The input note moves, and the sound is found by it afterwards.
        assert!(doc.set_sound_input(map, 36, 30));
        assert!(sound(&doc, map, 36).is_none());
        assert_eq!(sound(&doc, map, 30).map(|s| s.name), Some("Kick".into()));
        assert_eq!(doc.undo().as_deref(), Some("Change the drum sound's note"));
        assert!(sound(&doc, map, 36).is_some());
    }

    #[test]
    fn an_input_note_another_sound_answers_to_is_refused() {
        let (mut doc, _track, map) = kit();
        assert!(!doc.set_sound_input(map, 36, 38), "the snare answers to 38");
        assert!(!doc.set_sound_input(map, 36, 200), "past 127");
        assert!(!doc.set_sound_input(map, 36, 36), "no change");
        assert!(!doc.set_sound_input(Id(99_999), 36, 40), "no such map");
        assert!(!doc.set_sound_input(map, 99, 40), "no such sound");
        assert!(sound(&doc, map, 36).is_some());
    }

    #[test]
    fn adding_moving_and_removing_sounds_each_undo_in_one_step() {
        let (mut doc, _track, map) = kit();
        let before = doc.drum_map(map).unwrap().sounds.len();
        let new = Sound::new("Rimshot", 60, 60, DrumIcon::Rim);
        assert!(doc.add_drum_sound(map, new, Some(36)));
        let sounds = &doc.drum_map(map).unwrap().sounds;
        assert_eq!(sounds.len(), before + 1);
        assert_eq!(sounds[1].name, "Rimshot", "added after the kick");
        assert!(!doc.add_drum_sound(map, Sound::new("Two", 36, 36, DrumIcon::Kick), None));
        assert_eq!(doc.undo().as_deref(), Some("Add a drum sound"));
        assert_eq!(doc.drum_map(map).unwrap().sounds.len(), before);
        doc.redo();
        // The order matters (the lanes'): move it to the front.
        let at = doc
            .drum_map(map)
            .unwrap()
            .sounds
            .iter()
            .position(|s| s.name == "Rimshot")
            .expect("added");
        assert!(doc.move_drum_sound(map, at, 0));
        assert_eq!(
            doc.drum_map(map).unwrap().sounds[0].name,
            "Rimshot",
            "moved to the front"
        );
        assert_eq!(doc.undo().as_deref(), Some("Move a drum sound"));
        assert!(!doc.move_drum_sound(map, 0, 0), "no change");
        assert!(!doc.move_drum_sound(map, 0, 99), "past the end");
        // Removing the sound leaves every note alone: nothing is destructive.
        assert!(doc.remove_drum_sound(map, 60));
        assert_eq!(doc.drum_map(map).unwrap().sounds.len(), before);
        assert_eq!(doc.undo().as_deref(), Some("Remove a drum sound"));
    }

    #[test]
    fn make_unique_copies_the_map_and_gives_the_copy_to_the_track() {
        let (mut doc, track, map) = kit();
        let other = doc.add_track(TrackKind::Midi, None);
        assert!(doc.attach_drum_map(other, map));
        let copy = doc
            .make_drum_map_unique(track)
            .expect("the track has a map");
        assert_ne!(copy, map);
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(copy));
        assert_eq!(
            doc.drum_map_of(other).map(|m| m.id),
            Some(map),
            "the other track keeps the original"
        );
        assert_eq!(
            doc.drum_map(copy).map(|m| m.name.as_str()),
            Some("808 style copy")
        );
        assert_eq!(doc.project().drum_maps.len(), 2);
        // An edit to the copy leaves the original alone.
        assert!(doc.set_sound_mute(copy, 36, true));
        assert!(!doc.drum_map(map).unwrap().sound(36).unwrap().mute);
        assert_eq!(doc.undo().as_deref(), Some("Mute drum sound"));
        assert_eq!(doc.undo().as_deref(), Some("Make the drum map unique"));
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(map));
        // An audio track has no map to copy.
        let audio = doc.add_track(TrackKind::Audio, None);
        assert!(doc.make_drum_map_unique(audio).is_none());
    }

    #[test]
    fn an_imported_map_gets_a_new_id_and_can_go_straight_to_the_track() {
        let (mut doc, track, map) = kit();
        let mut file = Preset::GENERAL_MIDI.map(map);
        file.name = "From a file".into();
        let imported = doc.import_drum_map(file, &[track]);
        assert_ne!(imported, map, "the file's id means nothing here");
        assert_eq!(doc.project().drum_maps.len(), 2);
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(imported));
        assert_eq!(doc.drum_map(imported).map(|m| m.sounds.len()), Some(47));
        assert_eq!(doc.history().steps().0.last(), Some(&"Import a drum map"));
        // The notes were never touched.
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        doc.undo();
        assert!(doc.drum_map(imported).is_none());
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(map));
    }

    #[test]
    fn a_new_sound_draws_at_the_end_and_carries_what_the_editor_gives_it() {
        let (mut doc, _track, map) = kit();
        let sound = Sound::new("Clave", 61, 61, DrumIcon::Percussion);
        assert!(doc.add_drum_sound(map, sound, None));
        let sounds = &doc.drum_map(map).unwrap().sounds;
        assert_eq!(sounds.last().unwrap().name, "Clave");
        // A quantize and a length the map editor sets survive a save.
        assert!(doc.set_sound_quantize(
            map,
            61,
            Some(Quantize {
                size: Size::Note(16),
                feel: Default::default(),
            })
        ));
        assert!(doc.set_sound_length(map, 61, Some(Ticks(120))));
        let bytes = lumen_project::to_bytes(doc.project()).expect("a valid project");
        let read = lumen_project::from_bytes(&bytes).expect("it reads again");
        let sound = read.drum_map(map).unwrap().sound(61).unwrap();
        assert_eq!(sound.length, Some(Ticks(120)));
        assert!(sound.quantize.is_some());
    }
}
