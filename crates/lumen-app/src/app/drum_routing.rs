//! What the interface needs to know about per-sound routing (M8 slice D4,
//! the owner's RT A): which outputs a sound can be given, and which sounds a
//! track was made for and is playing now.
//!
//! The routing itself is the document's (`Document::route_drum_sound`,
//! `route_all_drum_sounds`, `clear_drum_routes`) and the playback's
//! (`midi::sends`, `midi::play`). This module only reads: the map editor's
//! **Output** column asks [`App::sound_output_choices`], and a track header
//! showing the icon of the sound it was made for asks
//! [`App::sounds_on_track`] (lit while one of them sounds) and
//! [`App::sound_output_pair_name`]. Nothing here edits and nothing is saved.

use std::collections::HashMap;

use lumen_core::time::Ticks;
use lumen_project::model::{DrumIcon, Id, Project, Route, Sound, Track, TrackKind};

use crate::App;
use crate::document::{Document, Routed};
use crate::hosting::State;
use crate::midi::runs::sounding_inputs;

/// One destination the Output column offers for a sound (RT A): the kit
/// track, one of the instrument's output pairs, or another instrument track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundOutput {
    /// The kit track: the instrument's own output.
    Main,
    /// An output pair of the track's instrument, by the number a sound's
    /// [`Route::Output`] names (1 and up), with the name the instrument gives
    /// it. `told` is whether Lumen can tell the instrument which sound goes
    /// there: true for a built-in instrument with pads of its own (the
    /// sampler, B5), false for a hosted plugin, which routes inside itself —
    /// the column says "set in the plugin" and picking it still makes the
    /// track that plays the pair.
    Pair { bus: u32, name: String, told: bool },
    /// Another instrument track of the project.
    Track { id: Id, name: String },
}

/// A sound heard on a track that is not the track whose clips hold it: its
/// pair's track, or the track it is sent to (M8 D4). What a header shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundOnTrack {
    /// The map the sound belongs to, and its input note.
    pub map: Id,
    pub input: u8,
    /// Its name and drawing, for the header's icon.
    pub name: String,
    pub icon: DrumIcon,
    /// Whether a hit of it is sounding at the position asked for.
    pub sounding: bool,
}

impl App {
    /// The outputs a sound of `track` can be given: Main first, then the
    /// pairs its instrument has, then the project's other instrument tracks
    /// (D4's Output column, in that order).
    pub fn sound_output_choices(&self, track: Id) -> Vec<SoundOutput> {
        let pairs = self.sound_output_pairs(track);
        choices(&self.document, track, &pairs)
    }

    /// The output pairs `track`'s instrument has, by the number a sound
    /// names: the running plugin's own outputs, for a hosted instrument with
    /// more than its main one. A built-in instrument declares none here: the
    /// sampler's pads come with B5.
    pub fn sound_output_pairs(&self, track: Id) -> Vec<(u32, String)> {
        let Some(device) = self.instrument_device(track) else {
            return Vec::new();
        };
        if !matches!(self.hosting.state(device), Some(State::Ready)) {
            return Vec::new();
        }
        self.hosting.extra_outputs(device)
    }

    /// The name the instrument gives output pair `bus` of `track`, if it has
    /// it: "Hats", "output 3".
    pub fn sound_output_pair_name(&self, track: Id, bus: u32) -> Option<String> {
        let names = self.sound_output_pairs(track);
        names
            .iter()
            .find(|(b, _)| *b == bus)
            .map(|(_, name)| name.clone())
    }

    /// The pair a sound's route names, when it is on an output pair.
    pub fn sound_output_pair(&self, map: Id, input: u8) -> Option<u32> {
        match self.document.drum_map(map)?.sound(input)?.route {
            Route::Output(bus) => Some(bus),
            Route::Main | Route::Track(_) => None,
        }
    }

    /// The sounds heard on `track` although another track's clips hold them:
    /// the sounds on its output pairs and the sounds sent to it (M8 D4).
    /// `sounding` follows the playhead — empty while the transport is
    /// stopped, so nothing lights — and a muted sound never lights. A frame
    /// asks once per header, and each kit's notes are read once for all of
    /// them (review M4).
    pub fn sounds_on_track(&self, track: Id) -> Vec<SoundOnTrack> {
        let at = self.playhead_while_playing();
        let mut cache = self.drum_sounding.borrow_mut();
        sounds_on(&self.document, &mut cache, track, at)
    }

    /// The input notes of `track`'s own sounds sounding now: what the map
    /// editor lights, wherever the sound is routed to be heard. Empty while
    /// the transport is stopped, the rule [`App::sounds_on_track`] lights by
    /// (review L2).
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

    /// Where the playhead is while the transport plays: `None` stopped, so
    /// nothing lights.
    fn playhead_while_playing(&self) -> Option<Ticks> {
        self.transport.state().playing.then(|| self.playhead())
    }

    /// The track a routing command acts on: the first selected MIDI track
    /// that reads a drum map, and the map it reads.
    pub(crate) fn drum_routing_track(&self) -> Option<(Id, Id)> {
        self.selected_midi_tracks()
            .into_iter()
            .find_map(|id| self.document.drum_map_of(id).map(|map| (id, map.id)))
    }

    /// The plugin instrument making `track`'s sound, if it is a hosted one.
    fn instrument_device(&self, track: Id) -> Option<Id> {
        self.project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.sound_source())
            .filter(|device| device.is_vst3())
            .map(|device| device.id)
    }

    /// "Route all sounds to tracks" (`track.drum_route_all`): one undo step,
    /// the new tracks grouped under the kit's name.
    pub(crate) fn route_all_sounds_command(&mut self) {
        let Some((track, map)) = self.drum_routing_track() else {
            return;
        };
        let pairs = self.sound_output_pairs(track).len() as u32;
        if pairs == 0 {
            // The palette refuses this before it runs; a keymap or a future
            // menu that does not ask reaches it, and says the same thing.
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

    /// "Send the sounds back to the kit track" (`track.drum_route_clear`).
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

/// What "Route all sounds to tracks" says it did.
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

/// "1 track" / "2 tracks", from the forms the caller wrote.
fn plural(count: usize, one: String, many: String) -> String {
    if count == 1 { one } else { many }
}

/// [`App::sound_output_choices`] without the app: the project and the pairs
/// the instrument has.
///
/// The tracks it offers are the ones a sound may be sent to, by the one rule
/// the edit applies (`midi::sends::can_send_to`, review L1): a MIDI track
/// that plays its instrument, not one sent to a hardware port, not a group,
/// a return or an audio track.
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

/// Whether the instrument on `track` can be told which sound goes to which
/// output: a built-in one (its pads, B5) can; a hosted plugin routes inside
/// itself (RT A).
fn told_internally(project: &Project, track: Id) -> bool {
    project
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.sound_source())
        .is_some_and(|device| !device.is_vst3())
}

/// The sounds sounding on each kit track at one position, kept while the
/// project and the position hold (M8 D4 review M4). A frame asks a header per
/// track; without this, every header read every kit's clips again, so a
/// project of 200 tracks cost 200 reads of each kit for one frame's lights.
#[derive(Debug, Default)]
pub(crate) struct Sounding {
    /// The document revision the readings are for.
    revision: u64,
    /// Where the transport was, or `None` when it was stopped.
    at: Option<Ticks>,
    /// The input notes of each kit track's sounds sounding there.
    held: HashMap<Id, Vec<u8>>,
    /// How many times a kit's notes were read (the test that 200 headers cost
    /// one read a kit).
    #[cfg(test)]
    translations: usize,
}

impl Sounding {
    /// The input notes of `kit`'s sounds sounding at `at`, read once for the
    /// position and the project's state.
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

    /// How many kits were read since the app started (the test of M4).
    #[cfg(test)]
    fn translations(&self) -> usize {
        self.translations
    }
}

/// [`App::sounds_on_track`] without the app: `at` is where the transport is,
/// or `None` while it is stopped. A kit's notes are read through `sounding`,
/// so asking for many tracks in one frame reads each kit once.
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
        // The sounds this track plays, in the map's order; nothing is read
        // for a kit that plays none of them here.
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

/// Whether a sound's route makes `track` a track made for sounds (M8 D4):
/// one the kit's pairs or sends play on. The header of such a track shows
/// the icon of its first sound in place of the colour chip.
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

    /// A kit track with a plugin instrument and a map, and a second MIDI
    /// track: the kit, the other track and the map.
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

    /// Gives the kit track a one-bar clip with the first sound at beat `at`
    /// and the second at beat `second` (clip time).
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
        // Review L1: the column offers what `route_drum_sound` takes, by the
        // same rule (`can_send_to`). A track sent to a hardware port, an
        // audio track and the kit itself are not offered.
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
        // The edit refuses what the column does not offer.
        let input = doc.drum_map(_map).map(|_| 36).unwrap();
        assert!(
            doc.route_drum_sound(kit, _map, input, Route::Track(port))
                .is_none()
        );
    }

    #[test]
    fn a_hit_under_the_playhead_lights_its_sound_on_a_pairs_track_and_a_sent_one() {
        // Review M5: `sounding` is true where a hit is under the playhead, on
        // a pair's track and on a track a sound is sent to, and false where
        // there is no hit.
        let (mut doc, kit, other, map) = kit();
        let kick = doc.drum_map(map).unwrap().sounds[0].input;
        // The kick is the first sound: pair 1's track. The rim is the second:
        // sent to the other track.
        let made = doc
            .route_all_drum_sounds(kit, map, 1, false)
            .expect("a pair to give");
        let pair_track = made.tracks[0];
        let rim = doc.drum_map(map).unwrap().sounds[1].input;
        doc.route_drum_sound(kit, map, rim, Route::Track(other))
            .expect("the rim goes to the other track");
        clip(&mut doc, kit, map, 0, Some(2));

        let mut cache = Sounding::default();
        // At beat 0 the kick sounds (pair 1's track) and the rim does not.
        let on_pair = sounds_on(&doc, &mut cache, pair_track, Some(Ticks::ZERO));
        assert_eq!(on_pair.len(), 1);
        assert_eq!(on_pair[0].input, kick);
        assert!(on_pair[0].sounding, "the kick's hit is under the playhead");
        let on_other = sounds_on(&doc, &mut cache, other, Some(Ticks::ZERO));
        assert_eq!(on_other.len(), 1);
        assert_eq!(on_other[0].input, rim);
        assert!(!on_other[0].sounding, "the rim's hit is not");
        // At beat 2 it is the other way round.
        let mut cache = Sounding::default();
        let on_pair = sounds_on(&doc, &mut cache, pair_track, Some(q(2)));
        assert!(!on_pair[0].sounding, "the kick's hit is over");
        let on_other = sounds_on(&doc, &mut cache, other, Some(q(2)));
        assert!(on_other[0].sounding, "the rim's is under the playhead");
        // Between the hits, neither lights.
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
        // Stopped: nothing lights, and the same rule answers the map editor.
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
        // Review M4: a frame asks a header per track; the reading is per kit,
        // position and revision, not per header.
        let (mut doc, kit, _other, map) = kit();
        let made = doc
            .route_all_drum_sounds(kit, map, 2, false)
            .expect("two pairs");
        clip(&mut doc, kit, map, 0, Some(2));
        // Two headers that play the kit's sounds, and 198 that play nothing:
        // the frame asks them all.
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
        // Another frame at the same position reads nothing new.
        for track in &tracks {
            sounds_on(&doc, &mut cache, *track, Some(Ticks::ZERO));
        }
        assert_eq!(cache.translations(), 1);
        // The playhead moves: one read again, for all of them.
        for track in &tracks {
            sounds_on(&doc, &mut cache, *track, Some(q(1)));
        }
        assert_eq!(cache.translations(), 2);
        // An edit re-reads it once, for every header that asks.
        doc.set_sound_mute(map, 36, true);
        for _ in 0..2 {
            sounds_on(&doc, &mut cache, tracks[0], Some(q(1)));
        }
        assert_eq!(cache.translations(), 3);
        // A header that plays none of the kit's sounds reads nothing.
        let quiet = *tracks.last().unwrap();
        sounds_on(&doc, &mut cache, quiet, Some(q(1)));
        assert_eq!(cache.translations(), 3);
    }

    #[test]
    fn an_outputs_card_track_stays_a_track_made_for_nothing_until_a_sound_uses_it() {
        // A pair's track the card made is not "made for sounds" until a sound
        // is routed to it: the header shows its colour chip, not an icon.
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
