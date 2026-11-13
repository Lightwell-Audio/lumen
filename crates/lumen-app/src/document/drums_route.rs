//! Per-sound routing on the document (M8 slice D4, §6.4's "the instrument or
//! a specific instrument output"): where each of a drum map's sounds is
//! heard — the kit track itself, one of its instrument's output pairs, or
//! another instrument track.
//!
//! Picking an **output pair** makes (or reuses) the audio track that plays
//! that pair of the instrument, named after the sound and placed right under
//! the kit track: the same output tracks M6 makes from a plugin card
//! (`document::outputs`), made here for a sound. Picking **another track**
//! changes only the route: that track's instrument then plays the sound's
//! notes (`midi::sends`).
//!
//! Every edit is one undo step, whatever it makes: routing a whole kit, its
//! group included, is one step. Nothing here touches a note.

use std::sync::Arc;

use lumen_project::groups;
use lumen_project::model::{Id, PluginOutput, Route, Track, TrackKind};

use super::outputs::is_unused;
use super::{Document, track_color};

/// The most output pairs a sound may name (the project's own rule, which
/// `validate` applies: pair 1 to 31).
const MOST_BUSES: u32 = 31;

/// What a routing edit leaves: the track each sound of the map plays on, in
/// the map's order, the group the tracks were put in, and how many sounds
/// were left on the kit track because the instrument had no output pair left
/// for them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routed {
    /// One entry for each sound of the map, in its order: the track the
    /// sound is heard on. The kit track for a sound on Main, the pair's
    /// track for one on an output, and the track it is sent to.
    pub tracks: Vec<Id>,
    /// The output tracks this edit made, in pair order.
    pub made: Vec<Id>,
    /// The new group holding the kit track and the tracks made for its
    /// sounds, when the edit was asked to group them.
    pub group: Option<Id>,
    /// Sounds with no output pair left for them: they stay on the kit track.
    pub on_main: usize,
}

impl Document {
    /// Sets where one sound plays: the kit track, one of `track`'s
    /// instrument's output pairs (making or reusing the track that plays it),
    /// or another instrument track. One undo step; `None` when nothing would
    /// change, or when the route cannot work — another track that is not an
    /// instrument track, the track itself, or an output pair out of range.
    pub fn route_drum_sound(
        &mut self,
        track: Id,
        map: Id,
        input: u8,
        route: Route,
    ) -> Option<Routed> {
        let sound = self.drum_map(map)?.sound(input)?;
        if sound.route == route || !self.route_fits(track, map, route) {
            return None;
        }
        Some(self.apply_routes(track, map, &[(input, route)], false, "Route the drum sound"))
    }

    /// "Route all sounds to tracks" (RT A): gives the map's sounds a pair of
    /// `track`'s instrument each, in the map's order — the first sound pair
    /// 1, the next pair 2, and so on for the `pairs` the instrument has — and
    /// makes (or reuses) the track that plays each pair. A sound past the
    /// pairs stays on the kit track, and `Routed::on_main` counts them; a
    /// sound already sent to another track is left where it is. With
    /// `group`, the kit track and the tracks made for it go into a new group
    /// in the same undo step. `None` when `pairs` is 0, when the track reads
    /// no map, or when nothing changes.
    pub fn route_all_drum_sounds(
        &mut self,
        track: Id,
        map: Id,
        pairs: u32,
        group: bool,
    ) -> Option<Routed> {
        if pairs == 0 || !self.is_midi_track(track) {
            return None;
        }
        let map_held = self.drum_map(map)?.clone();
        let mut routes: Vec<(u8, Route)> = Vec::new();
        let mut next = 1;
        for sound in &map_held.sounds {
            if sound.route.sent_to().is_some() {
                continue;
            }
            let route = if next <= pairs {
                let bus = next.min(MOST_BUSES);
                next += 1;
                Route::Output(bus)
            } else {
                Route::Main
            };
            if sound.route != route {
                routes.push((sound.input, route));
            }
        }
        if routes.is_empty() {
            return None;
        }
        Some(self.apply_routes(track, map, &routes, group, "Route the sounds to tracks"))
    }

    /// Sends every sound of `map` back to the kit track, in one undo step,
    /// taking away the output tracks that are left with nothing in them.
    /// `None` when every sound is on the kit track already.
    pub fn clear_drum_routes(&mut self, track: Id, map: Id) -> Option<Routed> {
        let routes: Vec<(u8, Route)> = self
            .drum_map(map)?
            .sounds
            .iter()
            .filter(|sound| !sound.route.is_main())
            .map(|sound| (sound.input, Route::Main))
            .collect();
        if routes.is_empty() {
            return None;
        }
        Some(self.apply_routes(
            track,
            map,
            &routes,
            false,
            "Send the sounds to the kit track",
        ))
    }

    /// The track that plays output pair `bus` of `track`'s instrument, when
    /// one was made.
    pub fn sound_output_track(&self, track: Id, bus: u32) -> Option<Id> {
        let device = self.output_device(track)?;
        self.output_tracks(device)
            .into_iter()
            .find(|(b, _)| *b == bus)
            .map(|(_, id)| id)
    }

    /// The track a sound of `map` is heard on now: the kit track, the pair's
    /// track, or the track it is sent to. For a track header's icon (M8 D4).
    pub fn sound_playing_track(&self, track: Id, map: Id, input: u8) -> Option<Id> {
        let route = self.drum_map(map)?.sound(input)?.route;
        Some(match route {
            Route::Main => track,
            Route::Output(bus) => self.sound_output_track(track, bus).unwrap_or(track),
            Route::Track(to) => to,
        })
    }

    /// Whether `route` can be given to a sound of `map` on `track`.
    fn route_fits(&self, track: Id, map: Id, route: Route) -> bool {
        match route {
            Route::Main => self.drum_map(map).is_some(),
            Route::Output(bus) => (1..=MOST_BUSES).contains(&bus) && self.is_midi_track(track),
            Route::Track(to) => crate::midi::sends::can_send_to(self.project(), track, to),
        }
    }

    /// The device whose output pair a sound of `track` can name: the hosted
    /// plugin instrument on it. A built-in instrument (the synth, and the
    /// sampler of B5) has no output tracks made by this module: the sampler
    /// is told its pads' pairs instead (`midi::play::pad_outputs`).
    fn output_device(&self, track: Id) -> Option<Id> {
        self.project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.sound_source())
            .filter(|device| device.is_vst3())
            .map(|device| device.id)
    }

    /// One undo step that sets `routes` on `map`'s sounds and makes, drops
    /// and groups the tracks that follow from them.
    fn apply_routes(
        &mut self,
        track: Id,
        map: Id,
        routes: &[(u8, Route)],
        group: bool,
        label: &str,
    ) -> Routed {
        let Some(before) = self.drum_map(map).cloned() else {
            return Routed::default();
        };
        let device = self.output_device(track);
        // The route each sound has after this edit.
        let route_of = |input: u8| -> Route {
            routes.iter().find(|(i, _)| *i == input).map_or_else(
                || {
                    before
                        .sounds
                        .iter()
                        .find(|s| s.input == input)
                        .map_or(Route::Main, |s| s.route)
                },
                |&(_, route)| route,
            )
        };
        // The output pairs still needed, with the first sound that plays on
        // each: the track is named after it.
        let mut needed: Vec<(u32, String)> = Vec::new();
        for sound in &before.sounds {
            if let Route::Output(bus) = route_of(sound.input)
                && !needed.iter().any(|(b, _)| *b == bus)
            {
                needed.push((bus, sound.name.clone()));
            }
        }
        needed.sort_unstable_by_key(|(bus, _)| *bus);
        let existing = device.map(|d| self.output_tracks(d)).unwrap_or_default();
        let make: Vec<(Id, u32, String)> = needed
            .iter()
            .filter(|(bus, _)| !existing.iter().any(|(b, _)| b == bus))
            .map(|(bus, name)| (self.fresh_id(), *bus, name.clone()))
            .collect();
        let drop: Vec<Id> = existing
            .iter()
            .filter(|(bus, _)| !needed.iter().any(|(b, _)| b == bus))
            .filter(|(_, id)| {
                self.project()
                    .tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .is_some_and(|t| is_unused(t))
            })
            .map(|(_, id)| *id)
            .collect();
        // Where each sound is heard after the edit, in the map's order.
        let mut heard: Vec<Id> = Vec::new();
        let mut on_main = 0;
        for sound in &before.sounds {
            match route_of(sound.input) {
                Route::Main => {
                    heard.push(track);
                    on_main += 1;
                }
                Route::Output(bus) => {
                    let made = make
                        .iter()
                        .find(|(_, b, _)| *b == bus)
                        .map(|(id, _, _)| *id)
                        .or_else(|| {
                            existing
                                .iter()
                                .find(|(b, id)| *b == bus && !drop.contains(id))
                                .map(|(_, id)| *id)
                        });
                    heard.push(made.unwrap_or(track));
                }
                Route::Track(to) => heard.push(to),
            }
        }
        let make_group = group && !make.is_empty() && self.can_group(track);
        let group_id = self.fresh_id();
        let group_name = before.name.clone();
        let color = track_color(self.project().new_track_color, self.project().tracks.len());
        let routes = routes.to_vec();
        let made: Vec<(Id, u32, String)> = make;
        let made_ids: Vec<Id> = made.iter().map(|(id, _, _)| *id).collect();
        self.history.apply(label, None, move |project| {
            for (input, route) in &routes {
                if let Some(sound) = project
                    .drum_maps
                    .iter_mut()
                    .find(|m| m.id == map)
                    .and_then(|m| m.sounds.iter_mut().find(|s| s.input == *input))
                {
                    sound.route = *route;
                }
            }
            // A pair no sound plays on any more loses its track when nothing
            // is in it; one holding work stays, as the outputs menu's Keep
            // does. A group that held only the tracks this edit takes away
            // goes with them, so sending a kit back leaves no empty group.
            let held: Vec<Id> = drop
                .iter()
                .filter_map(|id| {
                    project
                        .tracks
                        .iter()
                        .find(|t| t.id == *id)
                        .and_then(|t| t.parent)
                })
                .collect();
            project.tracks.retain(|t| !drop.contains(&t.id));
            super::mixer::forget_routes_to(project, &drop);
            let empty: Vec<Id> = held
                .into_iter()
                .filter(|group| {
                    project
                        .tracks
                        .iter()
                        .find(|t| t.id == *group)
                        .is_some_and(|t| t.kind == TrackKind::Group && is_unused(t))
                })
                .filter(|group| !project.tracks.iter().any(|t| t.parent == Some(*group)))
                .collect();
            project.tracks.retain(|t| !empty.contains(&t.id));
            let Some(at) = project.tracks.iter().position(|t| t.id == track) else {
                return;
            };
            let parent = project.tracks[at].parent;
            let kit_color = project.tracks[at].color;
            for (id, bus, name) in &made {
                // Right under the kit track, among the tracks made before it
                // in pair order, as an output track is.
                let mut index = project
                    .tracks
                    .iter()
                    .position(|t| t.id == track)
                    .unwrap_or(project.tracks.len())
                    + 1;
                while project.tracks.get(index).is_some_and(|t| {
                    t.plugin_output
                        .is_some_and(|o| Some(o.device) == device && o.bus < *bus)
                }) {
                    index += 1;
                }
                project.tracks.insert(
                    index,
                    Arc::new(Track {
                        parent,
                        plugin_output: device.map(|device| PluginOutput { device, bus: *bus }),
                        ..Track::new(*id, name.clone(), TrackKind::Audio, kit_color)
                    }),
                );
            }
            if make_group {
                // The tracks the sounds were given, not the kit track: they
                // are what a group of a multi-output instrument holds, and
                // the kit's own output stays where it was.
                let members: Vec<Id> = made.iter().map(|(id, _, _)| *id).collect();
                for slot in &mut project.tracks {
                    if members.contains(&slot.id) {
                        Arc::make_mut(slot).parent = Some(group_id);
                    }
                }
                let group = Arc::new(Track {
                    parent,
                    ..Track::new(group_id, group_name.clone(), TrackKind::Group, color)
                });
                let at = project
                    .tracks
                    .iter()
                    .position(|t| t.id == track)
                    .unwrap_or(project.tracks.len());
                project.tracks.insert(at, group);
            }
            groups::normalize(&mut project.tracks);
        });
        Routed {
            tracks: heard,
            made: made_ids,
            group: make_group.then_some(group_id),
            on_main,
        }
    }

    /// Whether the kit track can take a new group around it: groups nest three
    /// deep at most.
    fn can_group(&self, track: Id) -> bool {
        let tracks = &self.project().tracks;
        let inside = tracks
            .iter()
            .find(|t| t.id == track)
            .and_then(|t| t.parent)
            .map_or(0, |parent| {
                let mut depth = 1;
                let mut at = Some(parent);
                while let Some(id) = at {
                    at = tracks.iter().find(|t| t.id == id).and_then(|t| t.parent);
                    depth += 1;
                    if depth > groups::MAX_NESTING {
                        break;
                    }
                }
                depth
            });
        inside < groups::MAX_NESTING
    }
}

#[cfg(test)]
mod tests {
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::{DeviceKind, PluginRef, TrackKind};

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

    /// A kit track with a plugin instrument and the 808-style map, and a
    /// second MIDI track. Returns the document, the kit, the other track, the
    /// map and the instrument's device.
    fn kit() -> (Document, Id, Id, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let kit = doc.add_track(TrackKind::Midi, None);
        let device = doc.add_device(kit, plugin(), Slot::End).unwrap();
        let other = doc.add_track(TrackKind::Midi, None);
        let map = doc
            .attach_drum_map_preset(kit, Preset::EIGHT_OH_EIGHT)
            .expect("a MIDI track takes a map");
        (doc, kit, other, map, device)
    }

    fn names(doc: &Document) -> Vec<String> {
        doc.project()
            .tracks
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    fn sound(doc: &Document, map: Id, index: usize) -> (u8, String, Route) {
        let sound = &doc.drum_map(map).unwrap().sounds[index];
        (sound.input, sound.name.clone(), sound.route)
    }

    fn route_of(doc: &Document, map: Id, input: u8) -> Route {
        doc.drum_map(map)
            .unwrap()
            .sound(input)
            .expect("the sound")
            .route
    }

    #[test]
    fn a_sound_on_an_output_makes_its_track_under_the_kit_in_one_step() {
        let (mut doc, kit, _other, map, device) = kit();
        let (input, name, _) = sound(&doc, map, 0);
        let before = names(&doc);
        let done = doc
            .route_drum_sound(kit, map, input, Route::Output(2))
            .expect("a pair of the instrument");
        assert_eq!(done.made.len(), 1, "one track made");
        assert_eq!(done.tracks.len(), doc.drum_map(map).unwrap().sounds.len());
        assert_eq!(route_of(&doc, map, input), Route::Output(2));
        // Named after the sound, right under the kit, playing pair 2.
        let made = done.made[0];
        assert_eq!(
            names(&doc),
            vec![before[0].clone(), name.clone(), before[1].clone()],
            "under the kit track"
        );
        let track = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == made)
            .expect("the track");
        assert_eq!(track.kind, TrackKind::Audio);
        assert_eq!(track.plugin_output, Some(PluginOutput { device, bus: 2 }));
        assert_eq!(track.parent, None);
        assert_eq!(
            doc.sound_output_track(kit, 2),
            Some(made),
            "the pair's track is found again"
        );
        assert_eq!(
            doc.sound_playing_track(kit, map, input),
            Some(made),
            "and the sound plays on it"
        );
        assert_eq!(
            doc.history().steps().0.last(),
            Some(&"Route the drum sound")
        );
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        // One step takes the route and the track back together.
        doc.undo();
        assert_eq!(route_of(&doc, map, input), Route::Main);
        assert_eq!(names(&doc), before);
    }

    #[test]
    fn a_second_sound_on_the_same_pair_reuses_its_track() {
        let (mut doc, kit, _, map, device) = kit();
        let (first, _, _) = sound(&doc, map, 0);
        let (second, _, _) = sound(&doc, map, 1);
        let one = doc
            .route_drum_sound(kit, map, first, Route::Output(1))
            .unwrap();
        let two = doc
            .route_drum_sound(kit, map, second, Route::Output(1))
            .unwrap();
        assert!(two.made.is_empty(), "the pair has a track already");
        assert_eq!(two.tracks[1], one.tracks[0], "both play on it");
        assert_eq!(
            doc.project().tracks.len(),
            3,
            "the kit, its pair's track, the other"
        );
        assert_eq!(doc.output_tracks(device).len(), 1);
        assert_eq!(doc.undo().as_deref(), Some("Route the drum sound"));
        assert_eq!(route_of(&doc, map, second), Route::Main);
        assert_eq!(
            doc.output_tracks(device).len(),
            1,
            "the first keeps its track"
        );
    }

    #[test]
    fn another_track_takes_the_sound_without_making_a_track() {
        let (mut doc, kit, other, map, _) = kit();
        let (input, _, _) = sound(&doc, map, 2);
        let tracks = doc.project().tracks.len();
        let done = doc
            .route_drum_sound(kit, map, input, Route::Track(other))
            .expect("an instrument track takes it");
        assert!(done.made.is_empty());
        assert_eq!(doc.project().tracks.len(), tracks);
        assert_eq!(route_of(&doc, map, input), Route::Track(other));
        assert_eq!(done.tracks[2], other);
        assert_eq!(doc.undo().as_deref(), Some("Route the drum sound"));
        assert_eq!(route_of(&doc, map, input), Route::Main);
    }

    #[test]
    fn route_all_gives_each_sound_a_pair_and_groups_the_tracks_in_one_step() {
        let (mut doc, kit, _, map, device) = kit();
        let sounds = doc.drum_map(map).unwrap().sounds.len();
        let pairs = 3;
        let before = names(&doc);
        let done = doc
            .route_all_drum_sounds(kit, map, pairs, true)
            .expect("three pairs to give");
        assert_eq!(done.made.len(), pairs as usize);
        assert_eq!(done.on_main, sounds - pairs as usize);
        assert_eq!(done.tracks.len(), sounds);
        assert_eq!(
            doc.history().steps().0.last(),
            Some(&"Route the sounds to tracks"),
            "one step"
        );
        // The first sounds are on pairs 1..3, the rest stay on the kit.
        for index in 0..sounds {
            let expect = if index < pairs as usize {
                Route::Output(index as u32 + 1)
            } else {
                Route::Main
            };
            assert_eq!(route_of(&doc, map, sound(&doc, map, index).0), expect);
        }
        assert_eq!(doc.output_tracks(device).len(), pairs as usize);
        // The three tracks the pairs made are in a group named after the map.
        let group = done.group.expect("grouped");
        let inside: Vec<String> = doc
            .project()
            .tracks
            .iter()
            .filter(|t| t.parent == Some(group))
            .map(|t| t.name.clone())
            .collect();
        assert_eq!(inside.len(), pairs as usize);
        assert!(inside.contains(&"Kick".to_owned()), "{inside:?}");
        assert_eq!(
            doc.project()
                .tracks
                .iter()
                .find(|t| t.id == group)
                .map(|t| t.name.as_str()),
            Some("808 style")
        );
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        // One undo takes every track and every route away.
        doc.undo();
        assert_eq!(
            doc.project()
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Group)
                .count(),
            0
        );
        assert_eq!(doc.output_tracks(device).len(), 0);
        assert_eq!(route_of(&doc, map, sound(&doc, map, 0).0), Route::Main);
        assert_eq!(names(&doc), before);
    }

    #[test]
    fn routing_a_whole_kit_can_be_sent_back_and_empty_tracks_go() {
        let (mut doc, kit, _, map, device) = kit();
        doc.route_all_drum_sounds(kit, map, 4, false)
            .expect("four pairs");
        assert_eq!(doc.output_tracks(device).len(), 4);
        let done = doc.clear_drum_routes(kit, map).expect("some are routed");
        assert_eq!(done.made.len(), 0);
        assert_eq!(done.on_main, doc.drum_map(map).unwrap().sounds.len());
        assert_eq!(
            doc.history().steps().0.last(),
            Some(&"Send the sounds to the kit track")
        );
        assert!(
            doc.output_tracks(device).is_empty(),
            "empty tracks are gone"
        );
        assert!(doc.clear_drum_routes(kit, map).is_none(), "nothing to send");
    }

    #[test]
    fn an_output_track_that_holds_work_stays_when_its_sound_leaves() {
        use std::sync::Arc;

        use lumen_project::model::{Clip, ClipContent, Color};
        let (mut doc, kit, _, map, device) = kit();
        let (input, _, _) = sound(&doc, map, 0);
        let done = doc
            .route_drum_sound(kit, map, input, Route::Output(1))
            .unwrap();
        let track = done.made[0];
        let clip = doc.fresh_id();
        doc.apply("Put a take there", None, |project| {
            let slot = project.tracks.iter_mut().find(|t| t.id == track).unwrap();
            Arc::make_mut(slot).clips.push(Arc::new(Clip {
                id: clip,
                name: "Take".into(),
                start: lumen_core::time::Ticks::ZERO,
                length: lumen_core::time::Ticks::quarters(4),
                muted: false,
                color: None::<Color>,
                locked: false,
                fade_in: Default::default(),
                crossfade: false,
                fade_out: Default::default(),
                loop_length: None,
                loop_phase: Default::default(),
                audio_phase_seconds: 0.0,
                reversed: false,
                inverted: false,
                envelopes: Default::default(),
                takes: None,
                warp: None,
                content: ClipContent::Midi {
                    notes: Vec::new(),
                    events: Vec::new(),
                },
            }));
        });
        doc.clear_drum_routes(kit, map).unwrap();
        assert!(
            doc.project().tracks.iter().any(|t| t.id == track),
            "a track with a take is kept"
        );
        assert_eq!(doc.output_tracks(device).len(), 1, "it just plays nothing");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_route_that_cannot_work_changes_nothing_and_takes_no_step() {
        let (mut doc, kit, other, map, _) = kit();
        doc.seal();
        let (input, _, _) = sound(&doc, map, 0);
        let steps = doc.history().steps().0.len();
        // Itself.
        assert!(
            doc.route_drum_sound(kit, map, input, Route::Track(kit))
                .is_none()
        );
        // An audio track cannot take notes.
        let audio = doc.add_track(TrackKind::Audio, None);
        doc.seal();
        let steps = steps.max(doc.history().steps().0.len());
        assert!(
            doc.route_drum_sound(kit, map, input, Route::Track(audio))
                .is_none()
        );
        // A track sent to hardware plays no instrument.
        let mut project = doc.project().clone();
        let slot = project.tracks.iter_mut().find(|t| t.id == other).unwrap();
        Arc::make_mut(slot).midi_to = Some(lumen_project::model::MidiTo {
            port: "Synth".into(),
            channel: 0,
        });
        assert!(!crate::midi::sends::can_send_to(&project, kit, other));
        // Out of range pairs.
        assert!(
            doc.route_drum_sound(kit, map, input, Route::Output(0))
                .is_none()
        );
        assert!(
            doc.route_drum_sound(kit, map, input, Route::Output(32))
                .is_none()
        );
        // The route it already has.
        assert!(doc.route_drum_sound(kit, map, input, Route::Main).is_none());
        // No map, no track.
        assert!(
            doc.route_drum_sound(kit, Id(9_999), input, Route::Output(1))
                .is_none()
        );
        assert_eq!(doc.history().steps().0.len(), steps, "no step was taken");
        assert_eq!(route_of(&doc, map, input), Route::Main);
        // No pairs to give.
        assert!(doc.route_all_drum_sounds(kit, map, 0, true).is_none());
    }

    #[test]
    fn a_group_at_the_deepest_level_is_not_made_but_the_tracks_are() {
        let (mut doc, kit, _, map, _) = kit();
        // Three groups around the kit track already.
        for _ in 0..groups::MAX_NESTING {
            let group = doc.group_tracks(&[kit]).unwrap();
            assert_eq!(
                doc.project()
                    .tracks
                    .iter()
                    .filter(|t| t.id == group)
                    .count(),
                1
            );
        }
        let done = doc
            .route_all_drum_sounds(kit, map, 2, true)
            .expect("two pairs");
        assert_eq!(done.made.len(), 2, "the tracks are made");
        assert_eq!(done.group, None, "but no group goes deeper");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }
}
