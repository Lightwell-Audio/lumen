use std::collections::HashMap;

use lumen_core::time::Ticks;
use lumen_project::model::{DrumIcon, Id, Project, Route, Sound, Track, TrackKind};

use crate::App;
use crate::document::{Document, Routed};
use crate::hosting::State;
use crate::midi::runs::sounding_inputs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundOutput {
    Main,
    Pair { bus: u32, name: String, told: bool },
    Track { id: Id, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundOnTrack {
    pub map: Id,
    pub input: u8,
    pub name: String,
    pub icon: DrumIcon,
    pub sounding: bool,
}

impl App {
    pub fn sound_output_choices(&self, track: Id) -> Vec<SoundOutput> {
        let pairs = self.sound_output_pairs(track);
        choices(&self.document, track, &pairs)
    }

    pub fn sound_output_pairs(&self, track: Id) -> Vec<(u32, String)> {
        let Some(device) = self.instrument_device(track) else {
            return Vec::new();
        };
        if !matches!(self.hosting.state(device), Some(State::Ready)) {
            return Vec::new();
        }
        self.hosting.extra_outputs(device)
    }

    pub fn sound_output_pair_name(&self, track: Id, bus: u32) -> Option<String> {
        let names = self.sound_output_pairs(track);
        names
            .iter()
            .find(|(b, _)| *b == bus)
            .map(|(_, name)| name.clone())
    }

    pub fn sound_output_pair(&self, map: Id, input: u8) -> Option<u32> {
        match self.document.drum_map(map)?.sound(input)?.route {
            Route::Output(bus) => Some(bus),
            Route::Main | Route::Track(_) => None,
        }
    }

    pub fn sounds_on_track(&self, track: Id) -> Vec<SoundOnTrack> {
        let at = self.playhead_while_playing();
        let mut cache = self.drum_sounding.borrow_mut();
        sounds_on(&self.document, &mut cache, track, at)
    }

    pub fn sounds_sounding(&self, track: Id) -> Vec<u8> {
        let Some(at) = self.playhead_while_playing() else {
            return Vec::new();
        };
        let Some(kit) = self.project().tracks.iter().find(|t| t.id == track) else {
            return Vec::new();
        };
        let mut cache = self.drum_sounding.borrow_mut();
        cache.held(&self.document, kit, Some(at)).to_vec()
    }

    fn playhead_while_playing(&self) -> Option<Ticks> {
        self.transport.state().playing.then(|| self.playhead())
    }

    pub(crate) fn drum_routing_track(&self) -> Option<(Id, Id)> {
        self.selected_midi_tracks()
            .into_iter()
            .find_map(|id| self.document.drum_map_of(id).map(|map| (id, map.id)))
    }

    fn instrument_device(&self, track: Id) -> Option<Id> {
        self.project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.sound_source())
            .filter(|device| device.is_vst3())
            .map(|device| device.id)
    }

    pub(crate) fn route_all_sounds_command(&mut self) {
        let Some((track, map)) = self.drum_routing_track() else {
            return;
        };
        let pairs = self.sound_output_pairs(track).len() as u32;
        if pairs == 0 {
            self.say(
                "The instrument on this track has no separate outputs to route sounds to"
                    .to_owned(),
            );
            return;
        }
        match self.document.route_all_drum_sounds(track, map, pairs, true) {
            Some(done) => self.say(routed_words(&done)),
            None => self.say("Every sound already plays where it is routed.".to_owned()),
        }
    }

    pub(crate) fn clear_sound_routes_command(&mut self) {
        let Some((track, map)) = self.drum_routing_track() else {
            return;
        };
        match self.document.clear_drum_routes(track, map) {
            Some(done) => {
                let back = done.tracks.len();
                self.say(format!(
                    "{} back on the kit track; the tracks it made that were empty are gone",
                    plural(back, "1 sound".to_owned(), format!("{back} sounds"))
                ));
            }
            None => self.say("Every sound of the map is on the kit track already".to_owned()),
        }
    }
}

fn routed_words(done: &Routed) -> String {
    let made = done.made.len();
    let mut words = plural(
        made,
        "1 track made for the kit's sounds".to_owned(),
        format!("{made} tracks made for the kit's sounds"),
    );
    if done.on_main > 0 {
        words.push_str(&format!(
            "; {} left on the kit track, which has no pair for them",
            done.on_main
        ));
    }
    words
}

fn plural(count: usize, one: String, many: String) -> String {
    if count == 1 { one } else { many }
}

pub(crate) fn choices(doc: &Document, track: Id, pairs: &[(u32, String)]) -> Vec<SoundOutput> {
    let project = doc.project();
    let told = told_internally(project, track);
    let mut out = vec![SoundOutput::Main];
    for (bus, name) in pairs {
        out.push(SoundOutput::Pair {
            bus: *bus,
            name: name.clone(),
            told,
        });
    }
    for other in &project.tracks {
        if !crate::midi::sends::can_send_to(project, track, other.id) {
            continue;
        }
        out.push(SoundOutput::Track {
            id: other.id,
            name: other.name.clone(),
        });
    }
    out
}

fn told_internally(project: &Project, track: Id) -> bool {
    project
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.sound_source())
        .is_some_and(|device| !device.is_vst3())
}

#[derive(Debug, Default)]
pub(crate) struct Sounding {
    revision: u64,
    at: Option<Ticks>,
    held: HashMap<Id, Vec<u8>>,
    #[cfg(test)]
    translations: usize,
}

impl Sounding {
    fn held<'a>(&'a mut self, doc: &Document, kit: &Track, at: Option<Ticks>) -> &'a [u8] {
        let revision = doc.state();
        if self.revision != revision || self.at != at {
            self.held.clear();
            self.revision = revision;
            self.at = at;
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = self.held.entry(kit.id) {
            let notes = at.map_or_else(Vec::new, |at| sounding_inputs(doc.project(), kit, at));
            #[cfg(test)]
            {
                self.translations += 1;
            }
            slot.insert(notes);
        }
        self.held.get(&kit.id).map_or(&[], |notes| notes.as_slice())
    }

    #[cfg(test)]
    fn translations(&self) -> usize {
        self.translations
    }
}

pub(crate) fn sounds_on(
    doc: &Document,
    sounding: &mut Sounding,
    track: Id,
    at: Option<Ticks>,
) -> Vec<SoundOnTrack> {
    let project = doc.project();
    let mut out: Vec<SoundOnTrack> = Vec::new();
    for kit in &project.tracks {
        if kit.kind != TrackKind::Midi || kit.id == track {
            continue;
        }
        let Some(map) = project.drum_map_of(kit) else {
            continue;
        };
        let mine: Vec<&Sound> = map
            .sounds
            .iter()
            .filter(|sound| doc.sound_playing_track(kit.id, map.id, sound.input) == Some(track))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let lit: Vec<bool> = {
            let held = sounding.held(doc, kit, at);
            mine.iter()
                .map(|sound| held.contains(&sound.input))
                .collect()
        };
        for (sound, sounding) in mine.into_iter().zip(lit) {
            out.push(SoundOnTrack {
                map: map.id,
                input: sound.input,
                name: sound.name.clone(),
                icon: sound.icon,
                sounding,
            });
        }
    }
    out
}

pub fn made_for_sounds(project: &Project, track: Id) -> bool {
    project.tracks.iter().any(|kit| {
        kit.id != track
            && kit.kind == TrackKind::Midi
            && project.drum_map_of(kit).is_some_and(|map| {
                map.sounds.iter().any(|sound| match sound.route {
                    Route::Output(bus) => project.tracks.iter().any(|t| {
                        t.id == track
                            && t.plugin_output.is_some_and(|o| {
                                kit.sound_source().is_some_and(|d| d.id == o.device) && o.bus == bus
                            })
                    }),
                    Route::Track(to) => to == track,
                    Route::Main => false,
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use lumen_core::time::Ticks;
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::{DeviceKind, Note, PluginRef, TrackKind};

    use super::*;
    use crate::document::{OutputEdit, Slot};

    fn plugin() -> PluginRef {
        PluginRef::Vst3 {
            class_id: "00112233445566778899AABBCCDDEEFF".into(),
            name: "Kit".into(),
            vendor: "Vendor A".into(),
            kind: DeviceKind::Instrument,
        }
    }

    fn q(beats: i64) -> Ticks {
        Ticks::quarters(beats)
    }

    fn kit() -> (Document, Id, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let kit = doc.add_track(TrackKind::Midi, None);
        doc.add_device(kit, plugin(), Slot::End).unwrap();
        let other = doc.add_track(TrackKind::Midi, None);
        let map = doc
            .attach_drum_map_preset(kit, Preset::EIGHT_OH_EIGHT)
            .expect("a MIDI track takes a map");
        (doc, kit, other, map)
    }

    fn clip(doc: &mut Document, kit: Id, map: Id, at: i64, second: Option<i64>) {
        let held = doc.drum_map(map).cloned().expect("the map");
        let mut notes = vec![Note::new(q(at), q(1), held.sounds[0].input, 100)];
        if let Some(second) = second {
            notes.push(Note::new(q(second), q(1), held.sounds[1].input, 100));
        }
        let id = doc.fresh_id();
        let clip = lumen_project::model::Clip {
            id,
            name: "Kit".into(),
            start: Ticks::ZERO,
            length: q(8),
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
            content: lumen_project::model::ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        };
        doc.apply("a clip", None, |project| {
            let slot = project.tracks.iter_mut().find(|t| t.id == kit).unwrap();
            std::sync::Arc::make_mut(slot)
                .clips
                .push(std::sync::Arc::new(clip));
        });
    }

    #[test]
    fn the_choices_are_main_the_pairs_then_the_other_instrument_tracks() {
        let (mut doc, kit, other, _map) = kit();
        let audio = doc.add_track(TrackKind::Audio, None);
        let both = choices(&doc, kit, &[(1, "Kick".to_owned()), (3, "Hats".to_owned())]);
        assert_eq!(both[0], SoundOutput::Main);
        assert_eq!(
            both[1],
            SoundOutput::Pair {
                bus: 1,
                name: "Kick".into(),
                told: false,
            },
            "a hosted plugin routes inside itself"
        );
        assert_eq!(both.len(), 4, "Main, two pairs and the other track");
        assert_eq!(
            both[3],
            SoundOutput::Track {
                id: other,
                name: "MIDI 2".into()
            }
        );
        assert!(!both.contains(&SoundOutput::Track {
            id: audio,
            name: String::new()
        }));
    }

    #[test]
    fn a_track_the_edit_would_refuse_is_not_offered() {
        let (mut doc, kit, other, _map) = kit();
        let port = doc.add_track(TrackKind::Midi, None);
        doc.apply("a port", None, |project| {
            let slot = project.tracks.iter_mut().find(|t| t.id == port).unwrap();
            std::sync::Arc::make_mut(slot).midi_to = Some(lumen_project::model::MidiTo {
                port: "Synth".into(),
                channel: 0,
            });
        });
        doc.seal();
        let offered = choices(&doc, kit, &[]);
        assert_eq!(
            offered,
            vec![
                SoundOutput::Main,
                SoundOutput::Track {
                    id: other,
                    name: "MIDI 2".into()
                }
            ],
            "the port is not offered, and neither is the kit itself"
        );
        assert!(!crate::midi::sends::can_send_to(doc.project(), kit, port));
        let input = doc.drum_map(_map).map(|_| 36).unwrap();
        assert!(
            doc.route_drum_sound(kit, _map, input, Route::Track(port))
                .is_none()
        );
    }

    #[test]
    fn a_hit_under_the_playhead_lights_its_sound_on_a_pairs_track_and_a_sent_one() {
        let (mut doc, kit, other, map) = kit();
        let kick = doc.drum_map(map).unwrap().sounds[0].input;
        let made = doc
            .route_all_drum_sounds(kit, map, 1, false)
            .expect("a pair to give");
        let pair_track = made.tracks[0];
        let rim = doc.drum_map(map).unwrap().sounds[1].input;
        doc.route_drum_sound(kit, map, rim, Route::Track(other))
            .expect("the rim goes to the other track");
        clip(&mut doc, kit, map, 0, Some(2));

        let mut cache = Sounding::default();
        let on_pair = sounds_on(&doc, &mut cache, pair_track, Some(Ticks::ZERO));
        assert_eq!(on_pair.len(), 1);
        assert_eq!(on_pair[0].input, kick);
        assert!(on_pair[0].sounding, "the kick's hit is under the playhead");
        let on_other = sounds_on(&doc, &mut cache, other, Some(Ticks::ZERO));
        assert_eq!(on_other.len(), 1);
        assert_eq!(on_other[0].input, rim);
        assert!(!on_other[0].sounding, "the rim's hit is not");
        let mut cache = Sounding::default();
        let on_pair = sounds_on(&doc, &mut cache, pair_track, Some(q(2)));
        assert!(!on_pair[0].sounding, "the kick's hit is over");
        let on_other = sounds_on(&doc, &mut cache, other, Some(q(2)));
        assert!(on_other[0].sounding, "the rim's is under the playhead");
        let mut cache = Sounding::default();
        assert!(
            sounds_on(&doc, &mut cache, pair_track, Some(q(1)))
                .iter()
                .all(|s| !s.sounding)
        );
        assert!(
            sounds_on(&doc, &mut cache, other, Some(q(1)))
                .iter()
                .all(|s| !s.sounding)
        );
        assert!(
            sounds_on(&doc, &mut cache, pair_track, None)
                .iter()
                .all(|s| !s.sounding)
        );
    }

    #[test]
    fn a_sound_on_its_pairs_track_is_that_tracks_icon() {
        let (mut doc, kit, _other, map) = kit();
        let made = doc
            .route_all_drum_sounds(kit, map, 3, false)
            .expect("three pairs");
        assert_eq!(made.tracks.len(), doc.drum_map(map).unwrap().sounds.len());
        let second = made.tracks[1];
        assert_eq!(
            doc.sound_playing_track(kit, map, doc.drum_map(map).unwrap().sounds[1].input),
            Some(second)
        );
        let sounds = sounds_on(&doc, &mut Sounding::default(), second, Some(Ticks::ZERO));
        assert_eq!(
            sounds.len(),
            1,
            "one sound plays on the second pair's track"
        );
        assert_eq!(sounds[0].map, map);
        assert_eq!(sounds[0].input, doc.drum_map(map).unwrap().sounds[1].input);
        assert!(!sounds[0].sounding, "no clip is under the playhead");
        assert!(
            sounds_on(&doc, &mut Sounding::default(), kit, Some(Ticks::ZERO)).is_empty(),
            "the kit track is not a track made for a sound"
        );
        assert!(made_for_sounds(&doc.project().clone(), second));
    }

    #[test]
    fn a_sound_sent_to_another_track_is_that_tracks_icon() {
        let (mut doc, kit, other, map) = kit();
        let input = doc.drum_map(map).unwrap().sounds[2].input;
        doc.route_drum_sound(kit, map, input, Route::Track(other))
            .expect("an instrument track takes a sound");
        let sounds = sounds_on(&doc, &mut Sounding::default(), other, None);
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].input, input);
        assert!(made_for_sounds(&doc.project().clone(), other));
        assert!(!made_for_sounds(&doc.project().clone(), kit));
    }

    #[test]
    fn two_hundred_headers_read_each_kit_once() {
        let (mut doc, kit, _other, map) = kit();
        let made = doc
            .route_all_drum_sounds(kit, map, 2, false)
            .expect("two pairs");
        clip(&mut doc, kit, map, 0, Some(2));
        let mut tracks = made.tracks.clone();
        tracks.truncate(2);
        for _ in 0..198 {
            tracks.push(doc.add_track(TrackKind::Midi, None));
        }
        doc.seal();
        let mut cache = Sounding::default();
        for track in &tracks {
            sounds_on(&doc, &mut cache, *track, Some(Ticks::ZERO));
        }
        assert_eq!(cache.translations(), 1, "one read of the one kit");
        for track in &tracks {
            sounds_on(&doc, &mut cache, *track, Some(Ticks::ZERO));
        }
        assert_eq!(cache.translations(), 1);
        for track in &tracks {
            sounds_on(&doc, &mut cache, *track, Some(q(1)));
        }
        assert_eq!(cache.translations(), 2);
        doc.set_sound_mute(map, 36, true);
        for _ in 0..2 {
            sounds_on(&doc, &mut cache, tracks[0], Some(q(1)));
        }
        assert_eq!(cache.translations(), 3);
        let quiet = *tracks.last().unwrap();
        sounds_on(&doc, &mut cache, quiet, Some(q(1)));
        assert_eq!(cache.translations(), 3);
    }

    #[test]
    fn an_outputs_card_track_stays_a_track_made_for_nothing_until_a_sound_uses_it() {
        let (mut doc, kit, _other, _map) = kit();
        let device = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == kit)
            .and_then(|t| t.sound_source())
            .map(|d| d.id)
            .unwrap();
        let made = doc
            .set_outputs(
                device,
                OutputEdit {
                    on: &[(1, "Hats".to_owned())],
                    off: &[],
                    leave: None,
                },
            )
            .unwrap();
        assert!(!made_for_sounds(&doc.project().clone(), made[0]));
    }
}
