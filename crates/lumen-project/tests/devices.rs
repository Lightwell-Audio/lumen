//! Format v17 (M6 slice 2): what a saved device holds. A VST3 device keeps
//! its component and controller state, its version and its last reported
//! timing; a plugin says whether it is an instrument or an effect. Old files
//! mean what they always meant, and a damaged file is refused with a reason.

use std::fs;
use std::path::PathBuf;

use lumen_project::model::{
    Color, Device, DeviceKind, INFINITE_TAIL, Id, MAX_DEVICE_STATE, PluginRef, Project, Reported,
    Track, TrackKind,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};
use serde_json::{Value, json};

const CLASS_A: &str = "ABCDEF0123456789ABCDEF0123456789";
const CLASS_B: &str = "0123456789ABCDEF0123456789ABCDEF";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn instrument(id: u64) -> Device {
    Device::vst3(
        Id(id),
        CLASS_A,
        "Glass Pad",
        "Vendor A",
        DeviceKind::Instrument,
    )
}

fn effect(id: u64) -> Device {
    Device::vst3(
        Id(id),
        CLASS_B,
        "Room Reverb",
        "Vendor B",
        DeviceKind::Effect,
    )
}

fn track(id: u64, kind: TrackKind, devices: Vec<Device>) -> Track {
    let mut track = Track::new(Id(id), "Track", kind, Color(0x4F81BD));
    track.devices = devices;
    track
}

/// The model the v17 golden file must always load as. Never change it: when
/// the format moves on, this test proves old projects still open.
fn golden_v17_expected() -> Project {
    let mut synth = instrument(1001);
    synth.state = vec![1, 2, 3, 4];
    synth.controller_state = vec![9, 8, 7];
    synth.plugin_version = "1.2.3".into();
    synth.reported = Some(Reported {
        latency: 0,
        tail: 144_000,
        rate: 48_000,
    });
    let mut reverb = effect(1002);
    reverb.bypass = true;
    reverb.state = vec![5, 6];
    reverb.reported = Some(Reported {
        latency: 128,
        tail: INFINITE_TAIL,
        rate: 48_000,
    });
    let keys = track(1000, TrackKind::Midi, vec![synth, reverb]);
    let vox = track(1010, TrackKind::Audio, vec![effect(1011)]);
    project_of("Golden v17", vec![keys, vox])
}

#[test]
fn golden_v17_file_loads_forever() {
    let path = fixture("v17/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v17_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v17 golden file must load");
    assert_eq!(project, golden_v17_expected());
}

#[test]
fn a_v16_project_loads_with_its_devices_as_they_were() {
    let project = load(&fixture("v16/golden.lumen")).expect("v16 golden file loads");
    let devices: Vec<&Device> = project
        .tracks
        .iter()
        .flat_map(|track| &track.devices)
        .collect();
    assert!(!devices.is_empty(), "the v16 golden project has devices");
    for device in devices {
        assert!(device.controller_state.is_empty());
        assert!(device.plugin_version.is_empty());
        assert!(device.reported.is_none());
        if let PluginRef::Vst3 { kind, .. } = &device.plugin {
            assert_eq!(*kind, DeviceKind::Effect, "an old plugin means an effect");
        }
    }
}

#[test]
fn a_device_with_nothing_new_is_written_as_before() {
    let project = project_of(
        "Plain",
        vec![track(1000, TrackKind::Audio, vec![effect(1001)])],
    );
    let document: Value = serde_json::from_slice(&to_bytes(&project).unwrap()).unwrap();
    let device = &document["project"]["tracks"][0]["devices"][0];
    for field in ["controller_state", "plugin_version", "reported"] {
        assert!(
            device.get(field).is_none(),
            "{field} is written only when set"
        );
    }
    assert!(
        device["plugin"].get("kind").is_none(),
        "an effect writes no kind"
    );
}

#[test]
fn new_device_data_survives_a_round_trip() {
    let project = golden_v17_expected();
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    assert!(text.contains("\"kind\": \"instrument\""));
    assert!(text.contains("controller_state"));
    assert!(text.contains("\"plugin_version\": \"1.2.3\""));
    assert_eq!(from_bytes(text.as_bytes()).unwrap(), project);
}

#[test]
fn the_track_finds_its_sound_source_apart_from_the_built_in_synth() {
    let vst = track(1000, TrackKind::Midi, vec![instrument(1001), effect(1002)]);
    assert_eq!(vst.sound_source().map(|d| d.id), Some(Id(1001)));
    assert!(
        vst.instrument().is_none(),
        "instrument() is the built-in synth"
    );
    let synth = track(1010, TrackKind::Midi, vec![Device::synth(Id(1011), "poly")]);
    assert_eq!(synth.sound_source().map(|d| d.id), Some(Id(1011)));
    assert_eq!(synth.instrument().map(|d| d.id), Some(Id(1011)));
    let audio = track(1020, TrackKind::Audio, vec![effect(1021)]);
    assert!(audio.sound_source().is_none());
}

#[test]
fn reported_latency_follows_the_sample_rate() {
    let reported = Reported {
        latency: 128,
        tail: 0,
        rate: 48_000,
    };
    assert_eq!(reported.latency_at(48_000), 128);
    assert_eq!(reported.latency_at(96_000), 256);
    assert_eq!(reported.latency_at(44_100), 118, "rounded up, never early");
}

/// Serializes a valid project, lets `damage` break the first track's
/// devices in the JSON, and returns why loading it was refused.
fn refused(project: &Project, damage: impl FnOnce(&mut Vec<Value>)) -> String {
    let mut document: Value = serde_json::from_slice(&to_bytes(project).unwrap()).unwrap();
    let devices = document["project"]["tracks"][0]["devices"]
        .as_array_mut()
        .expect("the first track has devices");
    damage(devices);
    let bytes = serde_json::to_vec(&document).unwrap();
    from_bytes(&bytes)
        .expect_err("a damaged device must be refused")
        .to_string()
}

fn keys() -> Project {
    project_of(
        "Keys",
        vec![track(
            1000,
            TrackKind::Midi,
            vec![instrument(1001), effect(1002)],
        )],
    )
}

#[test]
fn a_track_has_one_instrument_wherever_it_sits() {
    // An effect before the instrument is tolerated on load: an older file may
    // hold one, and every older file opens. Edits refuse it (the Document).
    let mut document: Value = serde_json::from_slice(&to_bytes(&keys()).unwrap()).unwrap();
    document["project"]["tracks"][0]["devices"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    from_bytes(&serde_json::to_vec(&document).unwrap()).expect("order is not checked on load");
    let why = refused(&keys(), |devices| {
        let mut second = devices[0].clone();
        second["id"] = json!("00000000000003f3");
        devices.push(second);
    });
    assert!(why.contains("more than one instrument"), "{why}");
}

#[test]
fn an_instrument_needs_a_midi_track() {
    let project = project_of(
        "Audio",
        vec![track(1000, TrackKind::Audio, vec![effect(1001)])],
    );
    let why = refused(&project, |devices| {
        devices[0]["plugin"]["kind"] = json!("instrument");
    });
    assert!(why.contains("not MIDI"), "{why}");
}

#[test]
fn damaged_device_data_is_refused_with_a_reason() {
    let why = refused(&keys(), |devices| {
        devices[0]["plugin"]["class_id"] = json!("not-a-class-id");
    });
    assert!(why.contains("class id"), "{why}");

    let why = refused(&keys(), |devices| {
        use base64::Engine;
        let big = vec![0u8; MAX_DEVICE_STATE + 1];
        devices[1]["state"] = json!(base64::engine::general_purpose::STANDARD.encode(big));
    });
    assert!(why.contains("too large"), "{why}");

    for rate in [0, 7_999, 384_001] {
        let why = refused(&keys(), |devices| {
            devices[1]["reported"] = json!({"latency": 1, "tail": 0, "rate": rate});
        });
        assert!(why.contains("out of range"), "rate {rate}: {why}");
    }
    let why = refused(&keys(), |devices| {
        devices[1]["reported"] = json!({"latency": 1 << 25, "tail": 0, "rate": 48_000});
    });
    assert!(why.contains("out of range"), "{why}");

    let project = project_of(
        "Synth",
        vec![track(
            1000,
            TrackKind::Midi,
            vec![Device::synth(Id(1001), "poly")],
        )],
    );
    let why = refused(&project, |devices| {
        devices[0]["controller_state"] = json!("AQID");
    });
    assert!(why.contains("plugin-only"), "{why}");
}
