//! The synth's sound as values (format v36, M8 slice B4).
//!
//! Before v36 a synth device's sound was the *name* of one of five presets,
//! saved as text in its `state`. v36 gives every synth device the values the
//! sound is made of, worked out from that name, and a file of it is frozen
//! as a golden. The values are literals in the migration on purpose: a
//! project written before v36 opens sounding as it always did, whatever the
//! synth's defaults or its presets become.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{
    Clip, ClipContent, Color, Device, DeviceKind, Id, Note, PluginRef, Project, Track, TrackKind,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, to_bytes};

fn q(n: i64) -> Ticks {
    Ticks::quarters(n)
}

/// `project` as version 35 wrote it: the same model, one version back. A
/// synth device made by [`Device::synth`] writes no values, so the file is
/// exactly what that build saved.
fn as_v35(project: &Project) -> Vec<u8> {
    let text = String::from_utf8(to_bytes(project).unwrap()).unwrap();
    text.replace("\"version\": 36", "\"version\": 35")
        .into_bytes()
}

fn midi_track(devices: Vec<Device>) -> Track {
    let mut track = Track::new(Id(1), "Keys".to_owned(), TrackKind::Midi, Color(0));
    track.devices = devices;
    track
}

fn synth(id: Id, preset: &str) -> Device {
    Device::synth(id, preset)
}

/// The model the v36 golden file must always load as: a MIDI track whose
/// synth holds Poly's values, with a plugin beside it holding a name and its
/// own parameters. Never change it.
fn golden_v36_expected() -> Project {
    let mut synth = Device::new(
        Id(0x3601),
        PluginRef::Builtin {
            name: "synth".into(),
        },
        false,
        b"poly".to_vec(),
    );
    synth.params = [
        (1u32, 0.666_666_687_f64),
        (6, 0.666_666_687),
        (13, 0.492_032_528),
        (34, 0.159_999_996),
    ]
    .into_iter()
    .collect();
    let mut plugin = Device::vst3(
        Id(0x3602),
        "00112233445566778899AABBCCDDEEFF",
        "Warm Synth",
        "Zed",
        DeviceKind::Instrument,
    );
    plugin.params = BTreeMap::from([(3u32, 0.5_f64)]);
    let clip = Clip {
        id: Id(0x3603),
        name: "Part".into(),
        start: Ticks::ZERO,
        length: q(4),
        muted: false,
        color: None,
        locked: false,
        crossfade: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        warp: None,
        content: ClipContent::Midi {
            notes: vec![
                Note::new(q(0), q(1), 60, 100),
                Note::new(q(2), q(1), 67, 90),
            ],
            events: Vec::new(),
        },
    };
    let mut keys = midi_track(vec![synth]);
    keys.clips = vec![Arc::new(clip)];
    // A plugin instrument of its own, on its own track: a track holds one.
    let mut pad = Track::new(Id(0x3605), "Pad".to_owned(), TrackKind::Midi, Color(0));
    pad.devices = vec![plugin];
    project_of("Golden v36", vec![keys, pad])
}

#[test]
fn golden_v36_loads_and_round_trips() {
    let project = common::golden("v36", golden_v36_expected);
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

/// The five preset names a v35 project may hold become the values that sound
/// the same, and a name this build does not know becomes the default sound,
/// as it always played.
#[test]
fn a_preset_name_becomes_its_values() {
    let mut poly = None;
    for preset in ["poly", "keys", "bass", "pluck", "pad"] {
        let project = project_of("Old", vec![midi_track(vec![synth(Id(2), preset)])]);
        let loaded = from_bytes(&as_v35(&project)).expect("a v35 file loads");
        let device = &loaded.tracks[0].devices[0];
        assert_eq!(device.params.len(), 34, "{preset}");
        assert_eq!(
            device.preset(),
            Some(preset),
            "the name is kept as the record"
        );
        assert!(
            device.params.values().all(|v| (0.0..=1.0).contains(v)),
            "{preset}"
        );
        // The whole file round-trips.
        assert_eq!(from_bytes(&to_bytes(&loaded).unwrap()).unwrap(), loaded);
        if preset == "poly" {
            poly = Some(device.params.clone());
        }
    }
    // A name from a later version plays the default sound, as it always did.
    let future = project_of(
        "Old",
        vec![midi_track(vec![synth(Id(2), "from-the-future")])],
    );
    let loaded = from_bytes(&as_v35(&future)).expect("it loads");
    assert_eq!(loaded.tracks[0].devices[0].params, poly.unwrap());
}

/// A v35 project opens sounding as it did: its synth holds the values its
/// preset name meant, and a file already at v36 keeps what it holds.
#[test]
fn an_older_project_opens_with_its_synth_s_values() {
    let project = project_of("Old", vec![midi_track(vec![synth(Id(2), "pad")])]);
    let old = from_bytes(&as_v35(&project)).expect("v35 loads");
    let device = old.tracks[0]
        .devices
        .iter()
        .find(|d| d.is_synth())
        .expect("the synth");
    assert_eq!(device.params.len(), 34, "the synth has its values");
    // Pad's corner, as that preset set it (a 1.6 kHz low-pass): the values are
    // frozen in the migration, so this is what it always opens as.
    assert!((device.params[&13] - 0.429_227_144).abs() < 1e-9);
    // A value the project already holds is never replaced.
    let mut project = old.clone();
    let mut tracks: Vec<Arc<Track>> = Vec::new();
    for track in &project.tracks {
        let mut track = (**track).clone();
        if let Some(device) = track.devices.iter_mut().find(|d| d.is_synth()) {
            device.params.insert(13, 0.25);
        }
        tracks.push(Arc::new(track));
    }
    project.tracks = tracks;
    let again = from_bytes(&to_bytes(&project).unwrap()).unwrap();
    let device = again
        .tracks
        .iter()
        .flat_map(|t| &t.devices)
        .find(|d| d.is_synth())
        .expect("the synth");
    assert_eq!(device.params.get(&13), Some(&0.25));
}

/// A synth with no readable name is still given the default sound, and a
/// project with no synth is untouched.
#[test]
fn a_synth_with_no_name_gets_the_default_sound() {
    let project = project_of(
        "Old",
        vec![midi_track(vec![Device::new(
            Id(2),
            PluginRef::Builtin {
                name: "synth".into(),
            },
            false,
            Vec::new(),
        )])],
    );
    let mut bytes = as_v35(&project);
    // The file as that build wrote a synth with no name at all.
    let text = String::from_utf8(std::mem::take(&mut bytes))
        .unwrap()
        .replace(",\n            \"state\": \"\"", "");
    let loaded = from_bytes(text.as_bytes()).expect("it loads");
    assert_eq!(loaded.tracks[0].devices[0].params.len(), 34);
    // A project with a plugin alone is untouched by the migration.
    let plugin_only = project_of(
        "Old",
        vec![midi_track(vec![Device::vst3(
            Id(3),
            "00112233445566778899AABBCCDDEEFF",
            "Warm Synth",
            "Zed",
            DeviceKind::Instrument,
        )])],
    );
    let loaded = from_bytes(&as_v35(&plugin_only)).unwrap();
    assert!(loaded.tracks[0].devices[0].params.is_empty());
}
