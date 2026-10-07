//! P-FORMAT pass criteria: round trips, golden fixtures, refusal of newer
//! files, robustness against damaged input, crash-safe saving.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use lumen_core::time::{PPQ, TempoPoint, Ticks};
use lumen_project::migrate::CURRENT_VERSION;
use lumen_project::model::{
    Clip, ClipContent, Color, Device, Id, Media, Note, PluginRef, Project, TimeSignature, Track,
    TrackKind,
};
use lumen_project::model::{TransportState, ViewState};
use lumen_project::testing::{TempDir, small_project, synthetic};
use lumen_project::{
    LoadError, MediaLocation, SaveError, SaveOptions, from_bytes, load, locate_media,
    media_path_for, save, to_bytes,
};
use serde_json::Value;

/// The model the v1 golden file must always load as. Never change either:
/// when the format moves on, this test proves old projects still open.
fn golden_v1_expected() -> Project {
    let track = |id, name: &str, kind, color| Track {
        id: Id(id),
        name: name.into(),
        kind,
        color: Color(color),
        parent: None,
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        armed: false,
        devices: vec![],
        clips: vec![],
    };
    let mut kick = track(2, "Kick", TrackKind::Audio, 0xC0504D);
    kick.parent = Some(Id(1));
    kick.gain_db = -3.0;
    kick.devices = vec![Device {
        id: Id(0x10),
        plugin: PluginRef::Builtin {
            name: "Utility".into(),
        },
        bypass: false,
        state: vec![0, 1, 2, 3],
    }];
    kick.clips = vec![Arc::new(Clip {
        id: Id(0x20),
        name: "Kick 1".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(4),
        muted: false,
        content: ClipContent::Audio {
            media: Id(0x30),
            offset: 0,
            gain_db: 0.0,
        },
    })];
    let mut bass = track(3, "Bass", TrackKind::Midi, 0x4F81BD);
    bass.gain_db = -6.0;
    bass.pan = -0.25;
    bass.devices = vec![Device {
        id: Id(0x11),
        plugin: PluginRef::Vst3 {
            class_id: "56535455484E36747972656C6C6E3600".into(),
            name: "TyrellN6".into(),
            vendor: "u-he".into(),
        },
        bypass: true,
        state: vec![],
    }];
    bass.clips = vec![Arc::new(Clip {
        id: Id(0x21),
        name: "Bassline".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(4),
        muted: true,
        content: ClipContent::Midi {
            notes: vec![
                Note(Ticks::ZERO, Ticks(PPQ / 4), 36, 100),
                Note(Ticks(PPQ / 3), Ticks(PPQ / 4), 43, 90),
            ],
        },
    })];
    Project {
        name: "Golden v1".into(),
        sample_rate: 48_000,
        tempo: vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
            },
            TempoPoint {
                at: Ticks::quarters(16),
                bpm: 90.0,
            },
        ],
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tracks: vec![
            Arc::new(track(1, "Drums", TrackKind::Group, 0xC0504D)),
            Arc::new(kick),
            Arc::new(bass),
            Arc::new(track(4, "Master", TrackKind::Master, 0x808080)),
        ],
        media: vec![Media {
            id: Id(0x30),
            path: "Samples/kick.wav".into(),
            original_path: None,
            frames: 48_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 384_044,
        }],
        // Not in v1 files: the v1 -> v2 migration writes the defaults.
        transport: Default::default(),
        view: Default::default(),
    }
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The model the v2 golden file must always load as: v1 plus a saved loop
/// region and view.
fn golden_v2_expected() -> Project {
    Project {
        name: "Golden v2".into(),
        transport: TransportState {
            loop_start: Ticks::quarters(8),
            loop_end: Ticks::quarters(24),
            loop_enabled: true,
        },
        view: ViewState {
            left_beat: 4.0,
            px_per_beat: 32.0,
            browser_open: false,
            detail_open: true,
            browser_size: 240.0,
            detail_size: 180.0,
            ..ViewState::default()
        },
        ..golden_v1_expected()
    }
}

/// The model the v3 golden file must always load as: v2 plus an armed
/// track, a vertical scroll and a resized track.
fn golden_v3_expected() -> Project {
    let mut project = Project {
        name: "Golden v3".into(),
        ..golden_v2_expected()
    };
    Arc::make_mut(&mut project.tracks[1]).armed = true;
    project.view.top = 48.0;
    project.view.track_heights.insert(Id(2), 96.0);
    project
}

#[test]
fn golden_v3_file_loads_forever() {
    let path = fixture("v3/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 3 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v3_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v3 golden file must load");
    assert_eq!(project, golden_v3_expected());
}

#[test]
fn golden_v2_file_loads_forever() {
    let path = fixture("v2/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 2 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v2_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v2 golden file must load");
    assert_eq!(project, golden_v2_expected());
}

#[test]
fn golden_v1_file_loads_forever() {
    let project = load(&fixture("v1/golden.lumen")).expect("v1 golden file must load");
    assert_eq!(project, golden_v1_expected());
}

#[test]
fn round_trips_are_exact() {
    for seed in 0..25 {
        let project = synthetic(
            seed,
            3 + seed as usize % 9,
            1 + seed as usize % 5,
            seed as usize % 13,
            37,
        );
        let bytes = to_bytes(&project).unwrap();
        assert_eq!(from_bytes(&bytes).unwrap(), project, "seed {seed}");
    }
    let dir = TempDir::new("round-trip");
    let path = dir.path().join("Song.lumen");
    save(&golden_v1_expected(), &path, &SaveOptions::default()).unwrap();
    assert_eq!(load(&path).unwrap(), golden_v1_expected());
}

#[test]
fn output_is_readable_json_with_one_note_per_line() {
    let text = String::from_utf8(to_bytes(&golden_v1_expected()).unwrap()).unwrap();
    assert!(
        text.starts_with(&format!(
            "{{\n  \"format\": \"lumen-project\",\n  \"version\": {CURRENT_VERSION},"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("[{}, {}, 43, 90]", PPQ / 3, PPQ / 4)),
        "{text}"
    );
}

#[test]
fn newer_versions_are_refused_with_a_clear_message() {
    let mut document: Value =
        serde_json::from_slice(&to_bytes(&small_project(1)).unwrap()).unwrap();
    document["version"] = Value::from(999);
    let error = from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::TooNew {
                version: 999,
                supported: CURRENT_VERSION
            }
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("newer version of Lumen"));
}

#[test]
fn other_files_are_not_projects() {
    for text in [
        r#"{}"#,
        r#"[]"#,
        r#"{"format":"something-else","version":1}"#,
        r#"{"format":"lumen-project"}"#,
    ] {
        assert!(
            matches!(from_bytes(text.as_bytes()), Err(LoadError::NotAProject)),
            "{text}"
        );
    }
}

#[test]
fn every_truncation_is_an_error_not_a_panic() {
    let bytes = to_bytes(&small_project(7)).unwrap();
    // The final byte is a newline; everything shorter than that is incomplete.
    for cut in 0..bytes.len() - 1 {
        let result = from_bytes(&bytes[..cut]);
        assert!(
            result.is_err(),
            "truncation at {cut} of {} loaded",
            bytes.len()
        );
    }
}

#[test]
fn random_damage_never_panics() {
    let original = to_bytes(&small_project(11)).unwrap();
    let mut state = 0x853C_49E6_748F_EA9B_u64;
    let mut rejected = 0;
    for _ in 0..5_000 {
        let mut damaged = original.clone();
        for _ in 0..1 + state % 4 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = (state % damaged.len() as u64) as usize;
            damaged[at] = (state >> 32) as u8;
        }
        if from_bytes(&damaged).is_err() {
            rejected += 1;
        }
    }
    assert!(
        rejected > 4_000,
        "most random damage should be detected, got {rejected}"
    );
}

#[test]
fn inconsistent_projects_are_rejected_precisely() {
    let mut bad_pitch = golden_v1_expected();
    if let ClipContent::Midi { notes } =
        &mut Arc::make_mut(&mut Arc::make_mut(&mut bad_pitch.tracks[2]).clips[0]).content
    {
        notes[0].2 = 200;
    }
    let mut cycle = golden_v1_expected();
    Arc::make_mut(&mut cycle.tracks[0]).parent = Some(Id(2)); // group inside its own child
    Arc::make_mut(&mut cycle.tracks[1]).kind = TrackKind::Group;
    let mut missing_media = golden_v1_expected();
    missing_media.media.clear();

    for (project, expected) in [
        (bad_pitch, "out of range"),
        (cycle, "cycle"),
        (missing_media, "missing media"),
    ] {
        // Saving refuses, so such a file is never written...
        let Err(SaveError::Invalid(message)) = to_bytes(&project) else {
            panic!("saved an invalid project")
        };
        assert!(message.contains(expected), "{message}");
        // ...and a hand-edited one is rejected on load.
        let mut document: Value =
            serde_json::from_slice(&to_bytes(&golden_v1_expected()).unwrap()).unwrap();
        document["project"] = serde_json::to_value(&project).unwrap();
        let error = from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap_err();
        assert!(
            matches!(error, LoadError::Invalid(ref m) if m.contains(expected)),
            "{error:?}"
        );
    }
}

#[test]
fn a_project_that_cannot_round_trip_is_never_written() {
    let dir = TempDir::new("nan");
    let path = dir.path().join("Song.lumen");
    save(&golden_v1_expected(), &path, &SaveOptions::default()).unwrap();
    let before = fs::read(&path).unwrap();
    let mut broken = golden_v1_expected();
    Arc::make_mut(&mut broken.tracks[1]).gain_db = f32::NAN; // JSON would turn this into null
    assert!(matches!(
        save(&broken, &path, &SaveOptions::default()),
        Err(SaveError::Invalid(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn media_inside_the_project_moves_with_it() {
    let original = TempDir::new("media-a");
    let samples = original.path().join("Samples");
    fs::create_dir_all(&samples).unwrap();
    fs::write(samples.join("kick.wav"), b"RIFF").unwrap();
    let stored = media_path_for(original.path(), &samples.join("kick.wav"));
    assert_eq!(stored, "Samples/kick.wav");

    let media = Media {
        id: Id(1),
        path: stored,
        original_path: None,
        frames: 0,
        channels: 1,
        sample_rate: 48_000,
        size_bytes: 4,
    };
    let moved = TempDir::new("media-b");
    fs::create_dir_all(moved.path().join("Samples")).unwrap();
    fs::copy(
        samples.join("kick.wav"),
        moved.path().join("Samples").join("kick.wav"),
    )
    .unwrap();
    assert!(matches!(
        locate_media(moved.path(), &media),
        MediaLocation::Found(_)
    ));

    let empty = TempDir::new("media-c");
    assert!(matches!(
        locate_media(empty.path(), &media),
        MediaLocation::Missing { .. }
    ));
}
