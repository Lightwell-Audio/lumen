use lumen_project::model::{self, Id, TrackKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Send {
    pub from: Id,
    pub to: Id,
}

pub(crate) fn can_send_to(project: &model::Project, from: Id, to: Id) -> bool {
    if from == to {
        return false;
    }
    project.tracks.iter().find(|t| t.id == to).is_some_and(|t| {
        t.kind == TrackKind::Midi && t.midi_to.is_none() && t.sound_source().is_some()
    })
}

pub(crate) fn sends(project: &model::Project) -> Vec<Send> {
    let mut out: Vec<Send> = Vec::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Midi {
            continue;
        }
        let Some(map) = project.drum_map_of(track) else {
            continue;
        };
        for sound in &map.sounds {
            let Some(to) = sound.route.sent_to() else {
                continue;
            };
            if !can_send_to(project, track.id, to) {
                continue;
            }
            let send = Send { from: track.id, to };
            if !out.contains(&send) {
                out.push(send);
            }
        }
    }
    out
}

pub(crate) fn playable_map(
    project: &model::Project,
    track: &model::Track,
) -> Option<model::DrumMap> {
    let map = project.drum_map_of(track)?;
    let broken = map.sounds.iter().any(|sound| {
        sound
            .route
            .sent_to()
            .is_some_and(|to| !can_send_to(project, track.id, to))
    });
    if !broken {
        return None;
    }
    let mut held = map.clone();
    for sound in &mut held.sounds {
        if sound
            .route
            .sent_to()
            .is_some_and(|to| !can_send_to(project, track.id, to))
        {
            sound.route = model::Route::Main;
        }
    }
    Some(held)
}

#[cfg(test)]
pub(crate) fn kept_inputs(map: &model::DrumMap) -> Vec<u8> {
    map.sounds
        .iter()
        .filter(|sound| sound.route.sent_to().is_none())
        .map(|sound| sound.input)
        .collect()
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{DeviceKind, PluginRef, Route};

    use super::*;
    use crate::document::{Document, Slot};

    fn plugin() -> PluginRef {
        PluginRef::Vst3 {
            class_id: "00112233445566778899AABBCCDDEEFF".into(),
            name: "Kit".into(),
            vendor: "Vendor A".into(),
            kind: DeviceKind::Instrument,
        }
    }

    fn pair() -> (Document, Id, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let kit = doc.add_track(TrackKind::Midi, None);
        doc.add_device(kit, plugin(), Slot::End).unwrap();
        let other = doc.add_track(TrackKind::Midi, None);
        let map = doc
            .attach_drum_map_preset(kit, lumen_project::drums::presets::Preset::EIGHT_OH_EIGHT)
            .expect("a MIDI track takes a map");
        (doc, kit, other, map)
    }

    #[test]
    fn a_sound_sent_to_another_track_is_one_send_however_many_sounds_go_there() {
        let (mut doc, kit, other, map) = pair();
        assert!(sends(doc.project()).is_empty(), "every sound is at home");
        let inputs: Vec<u8> = doc
            .drum_map(map)
            .unwrap()
            .sounds
            .iter()
            .map(|s| s.input)
            .take(2)
            .collect();
        doc.set_sound_route(map, inputs[0], Route::Track(other));
        assert_eq!(
            sends(doc.project()),
            [Send {
                from: kit,
                to: other
            }]
        );
        doc.set_sound_route(map, inputs[1], Route::Track(other));
        assert_eq!(sends(doc.project()).len(), 1, "one read of the clips");
        assert_eq!(
            kept_inputs(doc.drum_map(map).unwrap()).len(),
            doc.drum_map(map).unwrap().sounds.len() - 2,
            "the sounds on the kit track"
        );
    }

    #[test]
    fn a_pair_stays_on_its_track_and_a_gone_or_wrong_target_is_skipped() {
        let (mut doc, kit, other, map) = pair();
        let first = doc.drum_map(map).unwrap().sounds[0].input;
        doc.set_sound_route(map, first, Route::Output(2));
        assert!(sends(doc.project()).is_empty(), "the instrument routes it");
        assert_eq!(
            kept_inputs(doc.drum_map(map).unwrap()).len(),
            doc.drum_map(map).unwrap().sounds.len(),
            "its notes stay the track's"
        );
        let audio = doc.add_track(TrackKind::Audio, None);
        let second = doc.drum_map(map).unwrap().sounds[1].input;
        assert!(
            doc.route_drum_sound(kit, map, second, Route::Track(audio))
                .is_none()
        );
        assert!(doc.set_sound_route(map, second, Route::Track(audio)));
        assert!(!can_send_to(doc.project(), kit, audio));
        assert!(sends(doc.project()).is_empty());
        let sender = doc.project().tracks.iter().find(|t| t.id == kit).unwrap();
        let held = playable_map(doc.project(), sender).expect("a route with no track to play it");
        assert_eq!(held.sounds[1].route, Route::Main);
        let mut project = doc.project().clone();
        let slot = project.tracks.iter_mut().find(|t| t.id == other).unwrap();
        std::sync::Arc::make_mut(slot).midi_to = Some(model::MidiTo {
            port: "Synth".into(),
            channel: 0,
        });
        assert!(!can_send_to(&project, kit, other));
        assert!(!can_send_to(doc.project(), other, other), "never itself");
    }
}
