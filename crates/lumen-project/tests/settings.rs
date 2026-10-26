//! Format v32 (M9 slice R4): a project may carry notes and the colour new
//! tracks take. Earlier files have neither, and a project without them writes
//! nothing.

use std::fs;
use std::path::PathBuf;

use lumen_project::model::{Color, Id, MOST_NOTES_BYTES, Project, Track, TrackKind};
use lumen_project::testing::project_of;
use lumen_project::{from_bytes, load, to_bytes, validate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The model the v32 golden file must always load as. Never change it.
fn golden_v32_expected() -> Project {
    let drums = Track::new(Id(3_200), "Drums", TrackKind::Midi, Color(0x4F81BD));
    let mut project = project_of("Golden v32", vec![drums]);
    project.sample_rate = 44_100;
    project.notes = "Verse in D minor.\nBridge: drop the bass for four bars.".into();
    project.new_track_color = Some(Color(0xC0504D));
    project
}

#[test]
fn golden_v32_loads_and_round_trips() {
    let path = fixture("v32/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v32_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v32 golden file must load");
    assert_eq!(project, golden_v32_expected());
    assert_eq!(from_bytes(&to_bytes(&project).unwrap()).unwrap(), project);
}

#[test]
fn older_projects_have_no_notes_and_cycle_the_palette() {
    let legacy = load(&fixture("v31/golden.lumen")).expect("v31 file loads");
    assert!(legacy.notes.is_empty());
    assert_eq!(legacy.new_track_color, None);
}

#[test]
fn a_plain_project_writes_neither() {
    let track = Track::new(Id(1), "A", TrackKind::Midi, Color(0));
    let text = String::from_utf8(to_bytes(&project_of("Plain", vec![track])).unwrap()).unwrap();
    assert!(!text.contains("notes"), "{text}");
    assert!(!text.contains("new_track_color"), "{text}");
}

#[test]
fn notes_past_the_limit_are_refused() {
    let mut project = golden_v32_expected();
    project.notes = "x".repeat(MOST_NOTES_BYTES);
    assert!(validate(&project).is_ok());
    project.notes.push('x');
    assert!(validate(&project).is_err());
}
