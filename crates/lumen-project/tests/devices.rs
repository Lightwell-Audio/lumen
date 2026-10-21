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

// Format v18 (M6 slice 3): the view remembers which device cards are folded.

/// The model the v18 golden file must always load as. Never change it.
fn golden_v18_expected() -> Project {
    let mut project = golden_v17_expected();
    project.name = "Golden v18".into();
    project.view.folded_devices = [Id(1002), Id(1011)].into_iter().collect();
    project
}

#[test]
fn golden_v18_file_loads_forever() {
    let path = fixture("v18/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v18_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v18 golden file must load");
    assert_eq!(project, golden_v18_expected());
}

#[test]
fn older_projects_have_no_folded_cards_and_none_are_written_when_there_are_none() {
    for old in ["v16/golden.lumen", "v17/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(project.view.folded_devices.is_empty(), "{old}");
    }
    let document: Value =
        serde_json::from_slice(&to_bytes(&golden_v17_expected()).unwrap()).unwrap();
    assert!(
        document["project"]["view"].get("folded_devices").is_none(),
        "written only when some are folded"
    );
    let document: Value =
        serde_json::from_slice(&to_bytes(&golden_v18_expected()).unwrap()).unwrap();
    assert_eq!(
        document["project"]["view"]["folded_devices"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

// Format v19 (M6 slice 4): a device holds parameter values over its saved
// state, and the view remembers which parameters a card's strip shows.

/// The model the v19 golden file must always load as. Never change it.
fn golden_v19_expected() -> Project {
    let mut project = golden_v18_expected();
    project.name = "Golden v19".into();
    for track in &mut project.tracks {
        let track = std::sync::Arc::make_mut(track);
        if let Some(device) = track.devices.iter_mut().find(|d| d.id == Id(1002)) {
            device.params = [(0, 0.25), (7, 1.0), (4_000_000_000, 0.0)]
                .into_iter()
                .collect();
        }
    }
    project.view.shown_params = [(Id(1002), vec![7, 0]), (Id(1011), vec![3])]
        .into_iter()
        .collect();
    project
}

#[test]
fn golden_v19_file_loads_forever() {
    let path = fixture("v19/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v19_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v19 golden file must load");
    assert_eq!(project, golden_v19_expected());
}

#[test]
fn older_projects_hold_no_parameter_values_and_none_are_written_when_there_are_none() {
    for old in ["v16/golden.lumen", "v17/golden.lumen", "v18/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(project.view.shown_params.is_empty(), "{old}");
        assert!(
            project
                .tracks
                .iter()
                .flat_map(|t| &t.devices)
                .all(|d| d.params.is_empty()),
            "{old}"
        );
    }
    let text = String::from_utf8(to_bytes(&golden_v18_expected()).unwrap()).unwrap();
    assert!(!text.contains("shown_params") && !text.contains("\"params\""));
    let text = String::from_utf8(to_bytes(&golden_v19_expected()).unwrap()).unwrap();
    assert!(text.contains("shown_params") && text.contains("\"params\""));
}

#[test]
fn parameter_values_that_cannot_be_a_plugins_are_refused_with_a_reason() {
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v19_expected();
        edit(&mut project);
        // Saving refuses it; and so would loading a file that said so.
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let why = refused(&|p| {
        let track = std::sync::Arc::make_mut(&mut p.tracks[0]);
        track.devices[1].params.insert(9, 1.5);
    });
    assert!(why.contains("parameter values"), "{why}");
    let why = refused(&|p| {
        let track = std::sync::Arc::make_mut(&mut p.tracks[0]);
        track.devices[1].params.insert(9, f64::NAN);
    });
    assert!(why.contains("parameter values"), "{why}");
    // A built-in device holds none of this.
    let why = refused(&|p| {
        let track = std::sync::Arc::make_mut(&mut p.tracks[0]);
        track.devices[0].plugin = PluginRef::Builtin {
            name: "synth".into(),
        };
        track.devices[0].params.insert(1, 0.5);
    });
    assert!(why.contains("plugin-only"), "{why}");
    let why = refused(&|p| {
        p.view.shown_params.insert(Id(1002), (0..300).collect());
    });
    assert!(why.contains("strip"), "{why}");
    // And the view cannot name devices without end.
    let why = refused(&|p| {
        p.view.folded_devices = (0..70_000).map(Id).collect();
    });
    assert!(why.contains("too many devices"), "{why}");
    let why = refused(&|p| {
        p.view.shown_params = (0..70_000).map(|i| (Id(i), vec![0])).collect();
    });
    assert!(why.contains("too many devices"), "{why}");
}

// Format v20 (M6 slice 6): an effect takes a sidechain from another track.

/// The model the v20 golden file must always load as. Never change it.
fn golden_v20_expected() -> Project {
    use lumen_project::model::{Sidechain, SidechainTap};
    let mut project = golden_v19_expected();
    project.name = "Golden v20".into();
    for track in &mut project.tracks {
        let track = std::sync::Arc::make_mut(track);
        if let Some(device) = track.devices.iter_mut().find(|d| d.id == Id(1011)) {
            device.sidechain = Some(Sidechain {
                from: Id(1000),
                tap: SidechainTap::PreFx,
            });
        }
    }
    project
}

#[test]
fn golden_v20_file_loads_forever() {
    let path = fixture("v20/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v20_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v20 golden file must load");
    assert_eq!(project, golden_v20_expected());
}

#[test]
fn older_projects_have_no_sidechains_and_none_are_written_when_there_are_none() {
    for old in [
        "v16/golden.lumen",
        "v17/golden.lumen",
        "v18/golden.lumen",
        "v19/golden.lumen",
    ] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(
            project
                .tracks
                .iter()
                .flat_map(|t| &t.devices)
                .all(|d| d.sidechain.is_none()),
            "{old}"
        );
    }
    let text = String::from_utf8(to_bytes(&golden_v19_expected()).unwrap()).unwrap();
    assert!(!text.contains("sidechain"));
    let text = String::from_utf8(to_bytes(&golden_v20_expected()).unwrap()).unwrap();
    assert!(text.contains("\"sidechain\"") && text.contains("pre_fx"));
}

#[test]
fn a_sidechain_that_cannot_work_is_refused_with_a_reason() {
    use lumen_project::model::{Sidechain, SidechainTap};
    let key = |from: u64| Sidechain {
        from: Id(from),
        tap: SidechainTap::PostFx,
    };
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v20_expected();
        edit(&mut project);
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[1]).devices[0].sidechain = Some(key(1010));
    });
    assert!(why.contains("its own track"), "{why}");
    // A source that was deleted is kept, so undoing the delete restores it.
    let mut gone = golden_v20_expected();
    std::sync::Arc::make_mut(&mut gone.tracks[1]).devices[0].sidechain = Some(key(77));
    assert!(to_bytes(&gone).is_ok());
    // Keys on one track from the other, and back: the routing would feed back.
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[0]).devices[1].sidechain = Some(key(1010));
    });
    assert!(why.contains("feeds back"), "{why}");
    // An instrument takes none.
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[0]).devices[0].sidechain = Some(key(1010));
    });
    assert!(why.contains("instrument takes no sidechain"), "{why}");
}

// Format v21 (M6 slice 6): an audio track plays an output of a plugin
// instrument.

/// The model the v21 golden file must always load as. Never change it.
fn golden_v21_expected() -> Project {
    use lumen_project::model::PluginOutput;
    let mut project = golden_v20_expected();
    project.name = "Golden v21".into();
    let mut kick = track(1020, TrackKind::Audio, vec![]);
    kick.name = "Kick out".into();
    kick.plugin_output = Some(PluginOutput {
        device: Id(1001),
        bus: 1,
    });
    let mut snare = track(1030, TrackKind::Audio, vec![]);
    snare.name = "Snare out".into();
    snare.plugin_output = Some(PluginOutput {
        device: Id(1001),
        bus: 3,
    });
    project.tracks.push(std::sync::Arc::new(kick));
    project.tracks.push(std::sync::Arc::new(snare));
    project
}

#[test]
fn golden_v21_file_loads_forever() {
    let path = fixture("v21/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v21_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v21 golden file must load");
    assert_eq!(project, golden_v21_expected());
}

#[test]
fn older_projects_play_no_plugin_outputs_and_none_are_written_when_there_are_none() {
    for old in ["v16/golden.lumen", "v19/golden.lumen", "v20/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(
            project.tracks.iter().all(|t| t.plugin_output.is_none()),
            "{old}"
        );
    }
    let text = String::from_utf8(to_bytes(&golden_v20_expected()).unwrap()).unwrap();
    assert!(!text.contains("plugin_output"));
    let text = String::from_utf8(to_bytes(&golden_v21_expected()).unwrap()).unwrap();
    assert!(text.contains("plugin_output"));
}

#[test]
fn a_plugin_output_that_cannot_be_played_is_refused_with_a_reason() {
    use lumen_project::model::PluginOutput;
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v21_expected();
        edit(&mut project);
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let play = |device, bus| {
        Some(PluginOutput {
            device: Id(device),
            bus,
        })
    };
    let why =
        refused(&|p| std::sync::Arc::make_mut(&mut p.tracks[0]).plugin_output = play(1001, 1));
    assert!(why.contains("only an audio track"), "{why}");
    let why =
        refused(&|p| std::sync::Arc::make_mut(&mut p.tracks[2]).plugin_output = play(1001, 0));
    assert!(why.contains("bus 1 to"), "{why}");
    let why =
        refused(&|p| std::sync::Arc::make_mut(&mut p.tracks[3]).plugin_output = play(1001, 1));
    assert!(why.contains("same plugin output"), "{why}");
    // A device that is no longer there is kept: the track says so.
    let mut gone = golden_v21_expected();
    std::sync::Arc::make_mut(&mut gone.tracks[2]).plugin_output = play(9_999, 1);
    assert!(to_bytes(&gone).is_ok());
}

// Format v22 (M7 slice A0): a track holds automation lanes.

/// The model the v22 golden file must always load as. Never change it.
fn golden_v22_expected() -> Project {
    use lumen_core::automation::{Lane, Point, Shape};
    use lumen_core::curve::Curve;
    use lumen_core::time::Ticks;
    use lumen_project::automation::{AutomationLane, Target};
    use lumen_project::model::{Level, Send};
    let lane = |points: Vec<Point>| Lane::new(points).expect("a lane in time order");
    let at = Ticks::quarters;
    let mut project = golden_v21_expected();
    project.name = "Golden v22".into();
    let reverb = track(1040, TrackKind::Return, vec![]);
    project.tracks.push(std::sync::Arc::new(reverb));
    for track in &mut project.tracks {
        let track = std::sync::Arc::make_mut(track);
        match track.id {
            Id(1000) => {
                track.automation = vec![
                    AutomationLane::new(
                        Target::Volume,
                        lane(vec![
                            Point::new(at(0), -100.0),
                            Point::new(at(4), 0.0).with_shape(Shape::Curve(Curve::S_CURVE)),
                            Point::new(at(8), -6.0).with_shape(Shape::Step),
                            Point::new(at(12), 3.0),
                        ]),
                    ),
                    AutomationLane {
                        target: Target::Mute,
                        lane: lane(vec![
                            Point::new(at(16), 0.0).with_shape(Shape::Step),
                            Point::new(at(17), 1.0).with_shape(Shape::Step),
                        ]),
                        enabled: false,
                    },
                ];
            }
            Id(1010) => {
                track.sends.push(Send {
                    to: Id(1040),
                    level: Level(-12.0),
                    pre_fader: false,
                });
                track.automation = vec![
                    AutomationLane::new(
                        Target::Pan,
                        lane(vec![Point::new(at(0), -1.0), Point::new(at(8), 1.0)]),
                    ),
                    AutomationLane::new(
                        Target::Send { to: Id(1040) },
                        lane(vec![Point::new(at(2), -30.0), Point::new(at(6), 0.0)]),
                    ),
                    AutomationLane::new(
                        Target::Device {
                            device: Id(1011),
                            param: 7,
                        },
                        lane(vec![Point::new(at(0), 0.0), Point::new(at(4), 0.75)]),
                    ),
                ];
            }
            _ => {}
        }
    }
    project
}

#[test]
fn golden_v22_file_loads_forever() {
    let path = fixture("v22/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v22_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v22 golden file must load");
    assert_eq!(project, golden_v22_expected());
}

#[test]
fn older_projects_have_no_lanes_and_none_are_written_when_there_are_none() {
    for old in ["v16/golden.lumen", "v20/golden.lumen", "v21/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(
            lumen_project::routing::all_tracks(&project).all(|t| t.automation.is_empty()),
            "{old}"
        );
    }
    let text = String::from_utf8(to_bytes(&golden_v21_expected()).unwrap()).unwrap();
    assert!(!text.contains("automation"));
    let text = String::from_utf8(to_bytes(&golden_v22_expected()).unwrap()).unwrap();
    assert!(text.contains("\"automation\"") && text.contains("\"enabled\": false"));
}

#[test]
fn lanes_that_cannot_play_are_refused_with_a_reason() {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::{AutomationLane, MAX_LANES, Target};
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v22_expected();
        edit(&mut project);
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let one = |target, value, at: i64| {
        AutomationLane::new(
            target,
            Lane::new(vec![Point::new(Ticks(at), value)]).unwrap(),
        )
    };
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[0])
            .automation
            .push(one(Target::Volume, 0.0, 0));
    });
    assert!(why.contains("same"), "{why}");
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[0]).automation[0] = one(Target::Volume, 7.0, 0);
    });
    assert!(why.contains("outside"), "{why}");
    let why = refused(&|p| {
        std::sync::Arc::make_mut(&mut p.tracks[0]).automation[0] = one(Target::Volume, 0.0, -5);
    });
    assert!(why.contains("out of range"), "{why}");
    let why = refused(&|p| {
        let track = std::sync::Arc::make_mut(&mut p.tracks[1]);
        track.automation = (0..=MAX_LANES as u32)
            .map(|param| {
                one(
                    Target::Device {
                        device: Id(1011),
                        param,
                    },
                    0.5,
                    0,
                )
            })
            .collect();
    });
    assert!(why.contains("more than"), "{why}");
    // A lane whose send or device is gone is kept: undoing the deletion
    // brings the target back.
    let mut kept = golden_v22_expected();
    std::sync::Arc::make_mut(&mut kept.tracks[1])
        .automation
        .push(one(Target::Send { to: Id(77) }, 0.0, 0));
    assert!(to_bytes(&kept).is_ok());
}

#[test]
fn a_lane_out_of_time_order_is_a_damaged_file() {
    let text = String::from_utf8(to_bytes(&golden_v22_expected()).unwrap()).unwrap();
    // Move a later point of the fader lane (8 quarter notes) before its
    // neighbours.
    let eight = format!("\"at\": {}", lumen_core::time::PPQ * 8);
    let damaged = text.replacen(&eight, "\"at\": 1", 1);
    assert_ne!(text, damaged);
    let error = from_bytes(damaged.as_bytes()).expect_err("must be refused");
    assert!(error.to_string().contains("not after"), "{error}");
}

// Format v23 (M7 slice A5): the project holds controller mappings.

/// The model the v23 golden file must always load as. Never change it.
fn golden_v23_expected() -> Project {
    use lumen_project::automation::Target;
    use lumen_project::mapping::Mapping;
    let mut project = golden_v22_expected();
    project.name = "Golden v23".into();
    project.mappings = vec![
        Mapping {
            channel: 0,
            number: 7,
            track: Id(1000),
            target: Target::Volume,
        },
        Mapping {
            channel: 9,
            number: 74,
            track: Id(1010),
            target: Target::Device {
                device: Id(1011),
                param: 7,
            },
        },
        Mapping {
            channel: 0,
            number: 10,
            track: Id(1010),
            target: Target::Send { to: Id(1040) },
        },
    ];
    project
}

#[test]
fn golden_v23_file_loads_forever() {
    let path = fixture("v23/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v23_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v23 golden file must load");
    assert_eq!(project, golden_v23_expected());
}

#[test]
fn older_projects_have_no_mappings_and_none_are_written_when_there_are_none() {
    for old in ["v16/golden.lumen", "v21/golden.lumen", "v22/golden.lumen"] {
        let project = load(&fixture(old)).expect("an older golden file loads");
        assert!(project.mappings.is_empty(), "{old}");
    }
    let text = String::from_utf8(to_bytes(&golden_v22_expected()).unwrap()).unwrap();
    assert!(!text.contains("mappings"));
    let text = String::from_utf8(to_bytes(&golden_v23_expected()).unwrap()).unwrap();
    assert!(text.contains("\"mappings\""));
}

#[test]
fn mappings_that_cannot_work_are_refused_with_a_reason() {
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = golden_v23_expected();
        edit(&mut project);
        to_bytes(&project)
            .expect_err("must be refused when saved")
            .to_string()
    };
    let why = refused(&|p| {
        p.mappings[1].channel = 0;
        p.mappings[1].number = 7;
    });
    assert!(why.contains("share a controller"), "{why}");
    let why = refused(&|p| p.mappings[0].number = 130);
    assert!(why.contains("range"), "{why}");
    let why = refused(&|p| {
        p.mappings[1].track = Id(1000);
        p.mappings[1].target = lumen_project::automation::Target::Volume;
    });
    assert!(why.contains("same control"), "{why}");
}
