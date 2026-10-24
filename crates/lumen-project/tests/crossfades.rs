//! Format v28: an audio clip may mark its incoming fade as a crossfade and
//! projects may opt into automatic crossfades. Earlier formats preserve the
//! old playback by migrating with automatic crossfades disabled.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, Color, Id, Media, Project, Track, TrackKind};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn golden_v28_expected() -> Project {
    let mut track = Track::new(Id(2_800), "Crossfade", TrackKind::Audio, Color(0x4F81BD));
    let clip = Clip {
        id: Id(2_801),
        name: "Incoming".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(4),
        muted: false,
        color: None,
        locked: false,
        fade_in: lumen_core::fade::Fade {
            length: Ticks(2_352),
            curve: lumen_core::curve::Curve::LINEAR,
            offset: Ticks::ZERO,
            span: Ticks::ZERO,
        },
        crossfade: true,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Ticks::ZERO,
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        content: ClipContent::Audio {
            media: Id(2_802),
            offset: 0,
            gain_db: 0.0,
        },
    };
    track.clips = vec![Arc::new(clip)];
    let mut project = project_of("Golden v28", vec![track]);
    project.auto_crossfade = true;
    project.media = vec![Media {
        id: Id(2_802),
        path: "Samples/join.wav".into(),
        original_path: None,
        frames: 48_000,
        channels: 1,
        sample_rate: 48_000,
        size_bytes: 96_000,
    }];
    project
}

#[test]
fn golden_v28_loads_and_round_trips() {
    let path = fixture("v28/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v28_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v28 golden file must load");
    assert_eq!(project, golden_v28_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn v27_migration_preserves_playback_and_new_projects_opt_in() {
    let legacy = load(&fixture("v27/golden.lumen")).expect("v27 file loads");
    assert!(!legacy.auto_crossfade);
    assert!(
        legacy
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .all(|clip| !clip.crossfade)
    );
    assert!(project_of("New", Vec::new()).auto_crossfade);
}

#[test]
fn a_midi_clip_cannot_request_a_crossfade() {
    let mut track = Track::new(Id(2_800), "Keys", TrackKind::Midi, Color(0x4F81BD));
    track.clips = vec![Arc::new(Clip {
        id: Id(2_801),
        name: "MIDI".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(4),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: true,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Ticks::ZERO,
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        content: ClipContent::midi(),
    })];
    let project = project_of("Invalid", vec![track]);
    assert!(to_bytes(&project).unwrap_err().to_string().contains("MIDI"));
}
