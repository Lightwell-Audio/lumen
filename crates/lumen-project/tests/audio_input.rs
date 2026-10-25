//! Format v30: an audio track may name the device inputs it records from.
//! Earlier formats route no input.

use std::fs;
use std::path::PathBuf;

use lumen_project::model::{AudioInput, Color, Id, Project, Track, TrackKind};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn golden_v30_expected() -> Project {
    let mut vocal = Track::new(Id(3_000), "Vocal", TrackKind::Audio, Color(0x4F81BD));
    vocal.input = Some(AudioInput::mono(2));
    vocal.armed = true;
    let mut room = Track::new(Id(3_001), "Room", TrackKind::Audio, Color(0xC0504D));
    room.input = Some(AudioInput::pair(4));
    let bare = Track::new(Id(3_002), "Bare", TrackKind::Audio, Color(0x9BBB59));
    project_of("Golden v30", vec![vocal, room, bare])
}

#[test]
fn golden_v30_loads_and_round_trips() {
    let path = fixture("v30/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v30_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v30 golden file must load");
    assert_eq!(project, golden_v30_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn older_projects_route_no_input() {
    let legacy = load(&fixture("v29/golden.lumen")).expect("v29 file loads");
    assert!(legacy.tracks.iter().all(|t| t.input.is_none()));
}

#[test]
fn an_unrouted_track_writes_no_input() {
    let project = project_of(
        "Plain",
        vec![Track::new(Id(1), "A", TrackKind::Audio, Color(0))],
    );
    let text = String::from_utf8(to_bytes(&project).unwrap()).unwrap();
    assert!(!text.contains("\"input\""), "{text}");
}

#[test]
fn only_an_audio_track_has_an_input_and_it_is_a_device_channel() {
    let mut project = golden_v30_expected();
    assert!(validate(&project).is_ok());
    let midi = Track {
        input: Some(AudioInput::mono(0)),
        ..Track::new(Id(3_010), "Keys", TrackKind::Midi, Color(0))
    };
    project.tracks.push(midi.into());
    assert!(validate(&project).unwrap_err().contains("audio track"));
    project.tracks.pop();
    std::sync::Arc::make_mut(&mut project.tracks[0]).input = Some(AudioInput::pair(255));
    assert!(validate(&project).unwrap_err().contains("device channel"));
}

#[test]
fn inputs_overlap_when_they_share_a_channel() {
    let (mono, pair) = (AudioInput::mono(3), AudioInput::pair(2));
    assert!(mono.overlaps(&pair) && pair.overlaps(&mono));
    assert!(!AudioInput::mono(1).overlaps(&pair));
    assert!(!AudioInput::pair(0).overlaps(&pair));
    assert_eq!(pair.channels(), 2..4);
}
