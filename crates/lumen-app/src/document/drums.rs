//! Drum maps on the document (M8 slice D0): attaching one to a MIDI track
//! (a preset's copy, or one the project already holds), detaching it, editing
//! its sounds, and converting a clip's notes to another map.
//!
//! Every method is one undo step. None of them touches a note but
//! [`Document::convert_drum_map`], the one destructive drum command (§6.4):
//! attaching, swapping and detaching leave every note exactly as it was,
//! because a clip stores the map's **input** notes.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::drums::{self, Converted, presets::Preset};
use lumen_project::model::{ClipContent, DrumIcon, DrumMap, Id, Quantize, Route, Sound, TrackKind};

use super::Document;

impl Document {
    /// The drum map called `id`, if the project holds it.
    pub fn drum_map(&self, id: Id) -> Option<&DrumMap> {
        self.project().drum_map(id)
    }

    /// The drum map `track` plays through, if it names one.
    pub fn drum_map_of(&self, track: Id) -> Option<&DrumMap> {
        let track = self.project().tracks.iter().find(|t| t.id == track)?;
        self.project().drum_map_of(track)
    }

    /// Gives `track` a copy of `preset` as a new map of the project (the
    /// owner's DM A: a preset is copied in, so the project never depends on a
    /// file elsewhere). One undo step. `None` when the track is not a MIDI
    /// track.
    #[allow(dead_code)] // D1's map editor and this slice's tests call it.
    pub fn attach_drum_map_preset(&mut self, track: Id, preset: Preset) -> Option<Id> {
        self.attach_drum_map_preset_to(&[track], preset)
    }

    /// Gives every MIDI track in `tracks` one new copy of `preset`, which
    /// they share. One undo step; `None` when none of them is a MIDI track.
    pub fn attach_drum_map_preset_to(&mut self, tracks: &[Id], preset: Preset) -> Option<Id> {
        let targets: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| self.is_midi_track(*id))
            .collect();
        if targets.is_empty() {
            return None;
        }
        let id = self.fresh_id();
        self.history.apply("Attach drum map", None, |project| {
            project.drum_maps.push(preset.map(id));
            for track in &targets {
                if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) {
                    Arc::make_mut(slot).drum_map = Some(id);
                }
            }
        });
        Some(id)
    }

    /// Gives `track` a map the project already holds, so two kit tracks can
    /// share one. One undo step; false when the map or the track is not
    /// there, or the track is not a MIDI track.
    #[allow(dead_code)] // D1's map editor and this slice's tests call it.
    pub fn attach_drum_map(&mut self, track: Id, map: Id) -> bool {
        self.attach_drum_map_to(&[track], map)
    }

    /// Gives every MIDI track in `tracks` the map `map`, which they then
    /// share. One undo step; false when it would change nothing or the map
    /// is not there.
    pub fn attach_drum_map_to(&mut self, tracks: &[Id], map: Id) -> bool {
        if self.drum_map(map).is_none() {
            return false;
        }
        let targets: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| {
                self.project()
                    .tracks
                    .iter()
                    .any(|t| t.id == *id && t.kind == TrackKind::Midi && t.drum_map != Some(map))
            })
            .collect();
        if targets.is_empty() {
            return false;
        }
        self.history.apply("Attach drum map", None, |project| {
            for track in &targets {
                if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) {
                    Arc::make_mut(slot).drum_map = Some(map);
                }
            }
        })
    }

    /// Takes the map off `track`: its notes are untouched and play as they
    /// are. One undo step.
    #[allow(dead_code)] // D1's map editor and this slice's tests call it.
    pub fn detach_drum_map(&mut self, track: Id) {
        self.detach_drum_maps(&[track]);
    }

    /// Takes the map off every track in `tracks` that has one, in one undo
    /// step: their notes are untouched and play as they are.
    pub fn detach_drum_maps(&mut self, tracks: &[Id]) {
        let targets: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| {
                self.project()
                    .tracks
                    .iter()
                    .any(|t| t.id == *id && t.drum_map.is_some())
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        self.history.apply("Detach drum map", None, |project| {
            for track in &targets {
                if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) {
                    Arc::make_mut(slot).drum_map = None;
                }
            }
        });
    }

    /// Mutes or unmutes a sound of `map`. One undo step; false when the map
    /// or the sound is not there.
    pub fn set_sound_mute(&mut self, map: Id, input: u8, mute: bool) -> bool {
        let label = if mute {
            "Mute drum sound"
        } else {
            "Unmute drum sound"
        };
        self.edit_sound(map, input, label, |sound| sound.mute = mute)
    }
}

/// The rest of the map's edits, one undo step each. D1's map editor is their
/// caller; until it lands, this slice's tests are, so the allowance goes with
/// that slice.
#[allow(dead_code)]
impl Document {
    /// Solos or unsolos a sound of `map`. One undo step.
    pub fn set_sound_solo(&mut self, map: Id, input: u8, solo: bool) -> bool {
        let label = if solo {
            "Solo drum sound"
        } else {
            "Unsolo drum sound"
        };
        self.edit_sound(map, input, label, |sound| sound.solo = solo)
    }

    /// Sets how long a sound plays, or lets its notes' lengths through again
    /// (`None`). One undo step.
    pub fn set_sound_length(&mut self, map: Id, input: u8, length: Option<Ticks>) -> bool {
        self.edit_sound(map, input, "Change the drum sound's length", |sound| {
            sound.length = length.filter(|l| *l > Ticks::ZERO)
        })
    }

    /// Sets the channel a sound plays on, or the track's (`None`). One undo
    /// step.
    pub fn set_sound_channel(&mut self, map: Id, input: u8, channel: Option<u8>) -> bool {
        self.edit_sound(map, input, "Change the drum sound's channel", |sound| {
            sound.channel = channel.filter(|c| *c <= 15)
        })
    }

    /// Sets the note a sound plays, leaving the note its clips store as it
    /// is. One undo step.
    pub fn set_sound_output(&mut self, map: Id, input: u8, output: u8) -> bool {
        self.edit_sound(map, input, "Change the drum sound's note", |sound| {
            sound.output = output.min(127)
        })
    }

    /// Puts a sound in a choke group, or in none (`None`). One undo step.
    pub fn set_sound_choke(&mut self, map: Id, input: u8, choke: Option<u8>) -> bool {
        self.edit_sound(map, input, "Change the drum sound's choke group", |sound| {
            sound.choke = choke.filter(|g| *g > 0)
        })
    }

    /// Renames a sound; its drawing follows the name as a new sound's does.
    /// One undo step.
    pub fn set_sound_name(&mut self, map: Id, input: u8, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let icon = drums::icon_for_name(name);
        self.edit_sound(map, input, "Rename the drum sound", |sound| {
            sound.name = name.to_owned();
            sound.icon = icon;
        })
    }

    /// Sets a sound's drawing without touching its name. One undo step.
    pub fn set_sound_icon(&mut self, map: Id, input: u8, icon: DrumIcon) -> bool {
        self.edit_sound(map, input, "Change the drum sound's icon", |sound| {
            sound.icon = icon
        })
    }

    /// Sets a sound's own quantize grid, or takes it away. One undo step.
    pub fn set_sound_quantize(&mut self, map: Id, input: u8, quantize: Option<Quantize>) -> bool {
        self.edit_sound(map, input, "Change the drum sound's quantize", |sound| {
            sound.quantize = quantize
        })
    }

    /// Sets where a sound plays: the instrument, one of its outputs, or
    /// another instrument track. One undo step.
    pub fn set_sound_route(&mut self, map: Id, input: u8, route: Route) -> bool {
        self.edit_sound(map, input, "Change where the drum sound plays", |sound| {
            sound.route = route
        })
    }

    /// Renames a map. One undo step; false when it is not there or the name
    /// is empty.
    pub fn rename_drum_map(&mut self, map: Id, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.drum_map(map).is_none() {
            return false;
        }
        self.history.apply("Rename drum map", None, |project| {
            if let Some(found) = project.drum_maps.iter_mut().find(|m| m.id == map) {
                found.name = name.to_owned();
            }
        })
    }
}

impl Document {
    /// The one real drum edit: rewrites the selected clips' notes from the
    /// maps their tracks play through to `target`, by sound name and then by
    /// the note the sound plays, and switches those tracks to `target` (one
    /// undo step). A note whose sound `target` lacks is kept when
    /// `keep_missing`, else left out. Notes with no sound, and clips on a
    /// track with no map, are untouched.
    pub fn convert_drum_map(&mut self, clips: &[Id], target: Id, keep_missing: bool) -> Converted {
        let Some(target_map) = self.drum_map(target).cloned() else {
            return Converted::default();
        };
        // Each track converts from the map it plays through now, before
        // anything changes.
        let mut sources: Vec<(Id, DrumMap)> = Vec::new();
        let mut found: Vec<(Id, Id)> = Vec::new();
        for track in &self.project().tracks {
            if !track.clips.iter().any(|c| clips.contains(&c.id)) {
                continue;
            }
            let Some(source) = self.project().drum_map_of(track) else {
                continue;
            };
            if source.id != target {
                sources.push((track.id, source.clone()));
                found.extend(
                    track
                        .clips
                        .iter()
                        .filter(|c| clips.contains(&c.id))
                        .map(|c| (c.id, track.id)),
                );
            }
        }
        if found.is_empty() {
            return Converted::default();
        }
        let mut done = Converted::default();
        self.history.apply("Convert drum map", None, |project| {
            for (clip, track) in &found {
                let Some(source) = sources.iter().find(|(id, _)| id == track) else {
                    continue;
                };
                let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) else {
                    continue;
                };
                let Some(slot) = Arc::make_mut(slot).clips.iter_mut().find(|c| c.id == *clip)
                else {
                    continue;
                };
                let ClipContent::Midi { notes, .. } = &mut Arc::make_mut(slot).content else {
                    continue;
                };
                let (rewritten, counts) =
                    drums::convert_notes(&source.1, &target_map, notes, keep_missing);
                *notes = rewritten;
                done.moved += counts.moved;
                done.kept += counts.kept;
                done.dropped += counts.dropped;
            }
            for (track, _) in &sources {
                if let Some(slot) = project.tracks.iter_mut().find(|t| t.id == *track) {
                    Arc::make_mut(slot).drum_map = Some(target);
                }
            }
        });
        done
    }

    /// Whether `track` is a MIDI track of the project.
    fn is_midi_track(&self, track: Id) -> bool {
        self.project()
            .tracks
            .iter()
            .any(|t| t.id == track && t.kind == TrackKind::Midi)
    }

    /// One undo step that changes sound `input` of `map` through `change`.
    fn edit_sound(
        &mut self,
        map: Id,
        input: u8,
        label: &str,
        change: impl FnOnce(&mut Sound),
    ) -> bool {
        let present = self.drum_map(map).is_some_and(|m| m.sound(input).is_some());
        if !present {
            return false;
        }
        self.history.apply(label, None, move |project| {
            if let Some(held) = project
                .drum_maps
                .iter_mut()
                .find(|m| m.id == map)
                .and_then(|m| m.sounds.iter_mut().find(|s| s.input == input))
            {
                change(held);
            }
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::grid::{Feel, Size};
    use lumen_project::model::{Clip, Note, Track, TrackKind};

    fn midi_clip(id: u64, notes: Vec<Note>) -> Clip {
        Clip {
            id: Id(id),
            name: "Kit".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Ticks::ZERO,
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            warp: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        }
    }

    /// A project with one MIDI track holding `notes`, on no map yet.
    fn kit_track(notes: Vec<Note>) -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        let clip = doc.fresh_id();
        doc.history.apply("clip", None, |project| {
            let slot = project.tracks.iter_mut().find(|t| t.id == track).unwrap();
            std::sync::Arc::make_mut(slot)
                .clips
                .push(std::sync::Arc::new(midi_clip(clip.0, notes)));
        });
        doc.seal();
        (doc, track, clip)
    }

    fn notes_of(doc: &Document, clip: Id) -> Vec<Note> {
        doc.project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip)
            .and_then(|c| match &c.content {
                ClipContent::Midi { notes, .. } => Some(notes.clone()),
                ClipContent::Audio { .. } => None,
            })
            .expect("a MIDI clip")
    }

    /// A repeatable stream of numbers.
    fn rng(mut seed: u64) -> impl FnMut() -> u64 {
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        }
    }

    #[test]
    fn attach_swap_edit_and_detach_in_any_order_leave_every_note_as_it_was() {
        let mut next = rng(0x5eed_1234);
        for round in 0..6 {
            let notes: Vec<Note> = (0..24)
                .map(|_| {
                    Note::new(
                        Ticks((next() % 8_000) as i64),
                        Ticks(1 + (next() % 900) as i64),
                        24 + (next() % 60) as u8,
                        (next() % 128) as u8,
                    )
                })
                .collect();
            let (mut doc, track, clip) = kit_track(notes.clone());
            let first = doc
                .attach_drum_map_preset(track, Preset::GENERAL_MIDI)
                .expect("a MIDI track takes a map");
            assert!(
                !doc.attach_drum_map(track, first),
                "it names that map already: nothing changes"
            );
            // One map is in the project, named after the preset.
            assert_eq!(doc.project().drum_maps.len(), 1);
            assert_eq!(
                doc.drum_map(first).map(|m| m.name.as_str()),
                Some("General MIDI")
            );
            // Swap to another preset, then edit the map it left behind: the
            // notes never change.
            let second = doc
                .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
                .expect("a MIDI track takes a map");
            let third = doc
                .attach_drum_map_preset(track, Preset::GENERAL_MIDI_2)
                .unwrap();
            assert_eq!(doc.project().drum_maps.len(), 3);
            for map in [first, second, third] {
                let sounds: Vec<u8> = doc
                    .drum_map(map)
                    .unwrap()
                    .sounds
                    .iter()
                    .map(|s| s.input)
                    .collect();
                for (n, input) in sounds.iter().copied().enumerate() {
                    let key = (round + n) as u8;
                    match key % 5 {
                        0 => {
                            doc.set_sound_mute(map, input, true);
                            doc.set_sound_mute(map, input, false);
                        }
                        1 => {
                            doc.set_sound_solo(map, input, true);
                            doc.set_sound_solo(map, input, false);
                        }
                        2 => {
                            doc.set_sound_output(map, input, 127 - (input % 60));
                            doc.set_sound_length(map, input, Some(Ticks(240)));
                            doc.set_sound_channel(map, input, Some(9));
                        }
                        3 => {
                            doc.set_sound_name(map, input, "A sound renamed");
                            doc.set_sound_icon(map, input, DrumIcon::Effect);
                            doc.set_sound_quantize(
                                map,
                                input,
                                Some(Quantize {
                                    size: Size::Note(16),
                                    feel: Feel::Triplet,
                                }),
                            );
                        }
                        _ => {
                            doc.set_sound_choke(map, input, Some(3));
                            doc.set_sound_route(map, input, Route::Output(2));
                        }
                    }
                    assert_eq!(notes_of(&doc, clip), notes, "round {round}, map {map:?}");
                }
            }
            // Back to the first map, then off entirely: still bit-identical.
            assert!(doc.attach_drum_map(track, first));
            doc.detach_drum_map(track);
            assert!(doc.drum_map_of(track).is_none());
            assert_eq!(notes_of(&doc, clip), notes);
            // The project it would save is one a later build reads.
            assert_eq!(lumen_project::validate(doc.project()), Ok(()));
            let bytes = lumen_project::to_bytes(doc.project()).expect("a valid project");
            assert_eq!(
                lumen_project::from_bytes(&bytes)
                    .expect("it reads again")
                    .drum_maps
                    .len(),
                doc.project().drum_maps.len()
            );
        }
    }

    #[test]
    fn each_attach_and_detach_is_one_undo_step_and_keeps_the_notes() {
        let notes = vec![Note::new(Ticks(0), Ticks(240), 36, 100)];
        let (mut doc, track, clip) = kit_track(notes.clone());
        let map = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        assert_eq!(doc.history().steps().0.last(), Some(&"Attach drum map"));
        doc.attach_drum_map_preset(track, Preset::GENERAL_MIDI);
        assert_eq!(doc.project().drum_maps.len(), 2);
        doc.undo();
        assert_eq!(
            doc.project().drum_maps.len(),
            1,
            "one step takes the copy back"
        );
        doc.undo();
        assert_eq!(doc.project().drum_maps.len(), 0, "and the first");
        assert!(doc.drum_map_of(track).is_none());
        assert_eq!(notes_of(&doc, clip), notes, "the notes never moved");
        doc.redo();
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(map));
        doc.detach_drum_map(track);
        assert!(doc.drum_map_of(track).is_none());
        assert_eq!(notes_of(&doc, clip), notes);
        doc.undo();
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(map));
        // Detaching a track with no map changes nothing.
        doc.detach_drum_map(track);
        doc.detach_drum_map(track);
        assert_eq!(doc.history().steps().0.last(), Some(&"Detach drum map"));
    }

    #[test]
    fn two_tracks_share_one_map_and_an_edit_to_it_reaches_both() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.add_track(TrackKind::Midi, None);
        let b = doc.add_track(TrackKind::Midi, None);
        let map = doc
            .attach_drum_map_preset(a, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        assert!(doc.attach_drum_map(b, map));
        assert_eq!(doc.drum_map_of(a).map(|m| m.id), Some(map));
        assert_eq!(doc.drum_map_of(b).map(|m| m.id), Some(map));
        doc.set_sound_mute(map, 42, true);
        assert!(
            doc.drum_map_of(a)
                .unwrap()
                .sounds
                .iter()
                .find(|s| s.input == 42)
                .unwrap()
                .mute
        );
        assert!(
            doc.drum_map_of(b)
                .unwrap()
                .sounds
                .iter()
                .find(|s| s.input == 42)
                .unwrap()
                .mute
        );
        assert_eq!(doc.project().drum_maps.len(), 1);
        // An audio track takes no map.
        let audio = doc.add_track(TrackKind::Audio, None);
        assert!(
            doc.attach_drum_map_preset(audio, Preset::GENERAL_MIDI)
                .is_none()
        );
        assert!(doc.drum_map_of(audio).is_none());
    }

    #[test]
    fn converting_rewrites_the_notes_by_sound_and_switches_the_track_in_one_step() {
        let notes = vec![
            Note::new(Ticks::ZERO, Ticks(120), 36, 100),
            Note::new(Ticks(240), Ticks(120), 42, 90),
            Note::new(Ticks(480), Ticks(120), 75, 80),
        ];
        let (mut doc, track, clip) = kit_track(notes.clone());
        let single = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        // A target map whose sounds sit on other notes, under the same names.
        let target = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        assert!(doc.attach_drum_map(track, single));
        let mut shifted = doc.drum_map(target).unwrap().clone();
        for sound in &mut shifted.sounds {
            sound.input += 20;
            sound.output += 20;
        }
        doc.history.apply("shift", None, |project| {
            let slot = project
                .drum_maps
                .iter_mut()
                .find(|m| m.id == target)
                .unwrap();
            *slot = shifted;
        });
        doc.seal();
        let converted = doc.convert_drum_map(&[clip], target, false);
        assert_eq!(converted.moved, 3, "kick, hat and claves found by name");
        assert_eq!(
            notes_of(&doc, clip)
                .iter()
                .map(|n| n.pitch())
                .collect::<Vec<_>>(),
            vec![56, 62, 95],
            "each sound's input note in the target map"
        );
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(target));
        assert_eq!(doc.history().steps().0.last(), Some(&"Convert drum map"));
        doc.undo();
        assert_eq!(notes_of(&doc, clip), notes, "one step gives the notes back");
        assert_eq!(doc.drum_map_of(track).map(|m| m.id), Some(single));
        // Committing to a map the notes already play through changes nothing.
        doc.redo();
        assert_eq!(doc.convert_drum_map(&[clip], target, false).moved, 0);
        assert_eq!(
            notes_of(&doc, clip)
                .iter()
                .map(|n| n.pitch())
                .collect::<Vec<_>>(),
            vec![56, 62, 95]
        );
    }

    #[test]
    fn converting_can_keep_notes_the_target_lacks() {
        let notes = vec![Note::new(Ticks::ZERO, Ticks(120), 36, 100)];
        let (mut doc, track, clip) = kit_track(notes.clone());
        let single = doc
            .attach_drum_map_preset(track, Preset::GENERAL_MIDI)
            .unwrap();
        let target = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        assert!(doc.attach_drum_map(track, single));
        let mut small = doc.drum_map(target).unwrap().clone();
        small.sounds.retain(|s| s.name == "Clap");
        doc.history.apply("small", None, |project| {
            let slot = project
                .drum_maps
                .iter_mut()
                .find(|m| m.id == target)
                .unwrap();
            *slot = small;
        });
        doc.seal();
        let kept = doc.convert_drum_map(&[clip], target, true);
        assert_eq!((kept.moved, kept.kept, kept.dropped), (0, 1, 0));
        assert_eq!(notes_of(&doc, clip), notes, "the kick stays where it was");
        doc.undo();
        let dropped = doc.convert_drum_map(&[clip], target, false);
        assert_eq!(dropped.dropped, 1);
        assert!(notes_of(&doc, clip).is_empty());
    }

    #[test]
    fn a_sound_edit_is_one_step_and_a_missing_map_or_sound_changes_nothing() {
        let (mut doc, track, _) = kit_track(vec![Note::new(Ticks::ZERO, Ticks(120), 36, 100)]);
        let map = doc
            .attach_drum_map_preset(track, Preset::EIGHT_OH_EIGHT)
            .unwrap();
        assert!(doc.set_sound_mute(map, 42, true));
        assert!(doc.set_sound_mute(map, 42, false));
        assert_eq!(doc.history().steps().0.last(), Some(&"Unmute drum sound"));
        assert!(!doc.set_sound_mute(map, 99, true), "no such sound");
        assert!(!doc.set_sound_mute(Id(99_999), 42, true), "no such map");
        assert!(!doc.set_sound_name(map, 42, "   "), "a nameless sound");
        assert!(doc.rename_drum_map(map, "Kit on the desk"));
        assert_eq!(
            doc.drum_map(map).map(|m| m.name.as_str()),
            Some("Kit on the desk")
        );
        assert_eq!(doc.undo().as_deref(), Some("Rename drum map"));
        assert_eq!(doc.undo().as_deref(), Some("Unmute drum sound"));
        assert_eq!(doc.undo().as_deref(), Some("Mute drum sound"));
        assert_eq!(doc.undo().as_deref(), Some("Attach drum map"));
        assert!(doc.drum_map(map).is_none());
    }

    #[test]
    fn a_track_that_is_not_midi_or_a_clip_on_no_map_is_left_alone() {
        let notes = vec![Note::new(Ticks::ZERO, Ticks(120), 36, 100)];
        let (mut doc, track, clip) = kit_track(notes.clone());
        let map = doc
            .attach_drum_map_preset(track, Preset::GENERAL_MIDI)
            .unwrap();
        doc.detach_drum_map(track);
        // A clip on a track with no map: converting is not offered.
        assert_eq!(doc.convert_drum_map(&[clip], map, false).moved, 0);
        assert_eq!(notes_of(&doc, clip), notes);
        // An unknown target changes nothing either.
        assert!(doc.attach_drum_map(track, map));
        assert_eq!(doc.convert_drum_map(&[clip], Id(77_777), false).moved, 0);
        let _ = Track::new(Id(1), "x", TrackKind::Midi, lumen_project::model::Color(0));
    }
}
