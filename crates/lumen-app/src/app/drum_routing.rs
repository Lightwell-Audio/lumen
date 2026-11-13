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

use lumen_core::time::Ticks;
use lumen_project::model::{DrumIcon, Id, Project, Route, TrackKind};

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
    /// stopped, so nothing lights — and a muted sound never lights.
    pub fn sounds_on_track(&self, track: Id) -> Vec<SoundOnTrack> {
        let at = self.transport.state().playing.then(|| self.playhead());
        sounds_on(&self.document, track, at)
    }

    /// The input notes of `track`'s own sounds sounding at the playhead: what
    /// the map editor lights, wherever the sound is routed to be heard.
    pub fn sounds_sounding(&self, track: Id) -> Vec<u8> {
        let Some(held) = self.project().tracks.iter().find(|t| t.id == track) else {
            return Vec::new();
        };
        sounding_inputs(self.project(), held, self.playhead())
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
        match self.document.route_all_drum_sounds(track, map, pairs, true) {
            Some(done) => self.say(routed_words(&done)),
            None => self.say(
                "The sounds are where they should be: the instrument has no pair to give them"
                    .to_owned(),
            ),
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
pub(crate) fn choices(doc: &Document, track: Id, pairs: &[(u32, String)]) -> Vec<SoundOutput> {
    let project = doc.project();
    let mut out = vec![SoundOutput::Main];
    for (bus, name) in pairs {
        out.push(SoundOutput::Pair {
            bus: *bus,
            name: name.clone(),
            told: told_internally(project, track),
        });
    }
    for other in &project.tracks {
        if other.id == track || other.kind != TrackKind::Midi || other.sound_source().is_none() {
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

/// [`App::sounds_on_track`] without the app: `at` is where the transport
/// is, or `None` while it is stopped.
pub(crate) fn sounds_on(doc: &Document, track: Id, at: Option<Ticks>) -> Vec<SoundOnTrack> {
    let project = doc.project();
    let mut out: Vec<SoundOnTrack> = Vec::new();
    for kit in &project.tracks {
        if kit.kind != TrackKind::Midi || kit.id == track {
            continue;
        }
        let Some(map) = project.drum_map_of(kit) else {
            continue;
        };
        let held: Vec<u8> = at.map_or_else(Vec::new, |at| sounding_inputs(project, kit, at));
        for sound in &map.sounds {
            let Some(plays_on) = doc.sound_playing_track(kit.id, map.id, sound.input) else {
                continue;
            };
            if plays_on != track {
                continue;
            }
            out.push(SoundOnTrack {
                map: map.id,
                input: sound.input,
                name: sound.name.clone(),
                icon: sound.icon,
                sounding: held.contains(&sound.input),
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
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::{DeviceKind, PluginRef};

    use super::*;
    use crate::document::Slot;

    fn plugin() -> PluginRef {
        PluginRef::Vst3 {
            class_id: "00112233445566778899AABBCCDDEEFF".into(),
            name: "Kit".into(),
            vendor: "Vendor A".into(),
            kind: DeviceKind::Instrument,
        }
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
    fn a_sound_on_its_pairs_track_lights_its_header_while_it_sounds() {
        let (mut doc, kit, _other, map) = kit();
        let bus = 2;
        let made = doc
            .route_all_drum_sounds(kit, map, 3, false)
            .expect("three pairs");
        assert_eq!(made.tracks.len(), doc.drum_map(map).unwrap().sounds.len());
        let second = made.tracks[1];
        assert_eq!(
            doc.sound_playing_track(kit, map, doc.drum_map(map).unwrap().sounds[1].input),
            Some(second)
        );
        let sounds = sounds_on(&doc, second, Some(Ticks::ZERO));
        assert_eq!(
            sounds.len(),
            1,
            "one sound plays on the second pair's track"
        );
        assert_eq!(sounds[0].map, map);
        assert_eq!(sounds[0].input, doc.drum_map(map).unwrap().sounds[1].input);
        assert!(!sounds[0].sounding, "no clip is under the playhead");
        assert!(
            sounds_on(&doc, second, None).iter().all(|s| !s.sounding),
            "stopped: nothing lights"
        );
        assert!(
            sounds_on(&doc, kit, Some(Ticks::ZERO)).is_empty(),
            "the kit track is not a track made for a sound"
        );
        assert!(made_for_sounds(&doc.project().clone(), second));
        let _ = bus;
    }

    #[test]
    fn a_sound_sent_to_another_track_is_that_tracks_icon() {
        let (mut doc, kit, other, map) = kit();
        let input = doc.drum_map(map).unwrap().sounds[2].input;
        doc.route_drum_sound(kit, map, input, Route::Track(other))
            .expect("an instrument track takes a sound");
        let sounds = sounds_on(&doc, other, None);
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].input, input);
        assert!(made_for_sounds(&doc.project().clone(), other));
        assert!(!made_for_sounds(&doc.project().clone(), kit));
        let _ = Route::Main;
    }
}
