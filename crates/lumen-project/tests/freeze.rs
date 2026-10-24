//! Format v29: a track may be frozen, playing a rendered file in place of its
//! clips and devices. Earlier formats have no frozen track.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit::freeze;
use lumen_project::model::{
    Clip, ClipContent, Color, Frozen, Id, Media, Project, Track, TrackKind,
};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn golden_v29_expected() -> Project {
    let mut track = Track::new(Id(2_900), "Frozen", TrackKind::Audio, Color(0x4F81BD));
    let clip = Clip {
        id: Id(2_901),
        name: "Take".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(8),
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
        content: ClipContent::Audio {
            media: Id(2_902),
            offset: 0,
            gain_db: 0.0,
        },
    };
    track.clips = vec![Arc::new(clip)];
    track.frozen = Some(Frozen {
        id: Id(2_903),
        media: Id(2_904),
        start: Ticks::quarters(4),
        length: Ticks::quarters(10),
        mono: true,
    });
    let mut project = project_of("Golden v29", vec![track]);
    project.media = vec![
        Media {
            id: Id(2_902),
            path: "Samples/take.wav".into(),
            original_path: None,
            frames: 48_000,
            channels: 1,
            sample_rate: 48_000,
            size_bytes: 96_000,
        },
        Media {
            id: Id(2_904),
            path: "Samples/Rendered/frozen.wav".into(),
            original_path: None,
            frames: 240_000,
            channels: 1,
            sample_rate: 48_000,
            size_bytes: 960_000,
        },
    ];
    project
}

#[test]
fn golden_v29_loads_and_round_trips() {
    let path = fixture("v29/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v29_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v29 golden file must load");
    assert_eq!(project, golden_v29_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn older_projects_have_no_frozen_track() {
    let legacy = load(&fixture("v28/golden.lumen")).expect("v28 file loads");
    assert!(legacy.tracks.iter().all(|t| t.frozen.is_none()));
}

#[test]
fn a_frozen_track_names_a_file_the_project_holds() {
    let mut project = golden_v29_expected();
    assert!(validate(&project).is_ok());
    project.media.pop();
    assert!(validate(&project).is_err(), "the frozen file is missing");
}

#[test]
fn the_golden_track_sounds_as_its_file_and_unfreezes_to_its_clip() {
    let mut project = golden_v29_expected();
    let heard = freeze::played(&project);
    let clips = &heard.tracks[0].clips;
    assert_eq!(clips.len(), 1);
    assert!(matches!(
        clips[0].content,
        ClipContent::Audio {
            media: Id(2_904),
            ..
        }
    ));
    assert!(heard.tracks[0].plays_mono(&heard.media));
    freeze::unfreeze(&mut project, &[Id(2_900)]);
    assert!(project.tracks[0].frozen.is_none());
}
