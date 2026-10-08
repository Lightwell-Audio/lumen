//! Files: importing audio, saving and opening, autosave and recovery.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_app::testing::{PROJECT_EXTENSION, project_file};

use super::{Lumen, wav};

/// The clips of every track, as (track name, clip count).
fn clips(lumen: &Lumen) -> Vec<(String, usize)> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .map(|t| (t.name.clone(), t.clips.len()))
        .collect()
}

#[test]
fn dropped_audio_becomes_a_track_with_a_clip_and_plays() {
    let mut lumen = Lumen::start("import");
    let file = wav(&lumen.dir().join("kick.wav"), 2.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    assert_eq!(clips(&lumen), [("kick".to_owned(), 1)]);
    let media = lumen.app().project().media[0].id;
    assert!(lumen.app().has_waveform(media), "loaded, with a waveform");

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_project_saves_and_opens_again_unchanged() {
    let mut lumen = Lumen::start("save-open");
    let file = wav(&lumen.dir().join("bass.wav"), 1.0);
    lumen.drop_files(&[file]);
    lumen.key(Modifiers::COMMAND, Key::T);
    let name = lumen.track_names()[0].clone();
    lumen.click(&format!("Mute {name}"));
    assert!(lumen.app().is_dirty());

    // Save as the Save dialog would, once a path is chosen.
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    assert!(path.exists(), "{}", path.display());
    let saved = lumen.app().project().clone();

    // A fresh Lumen opens it: the same project, its audio loaded.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    let opened = again.app().project();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(opened.media.len(), saved.media.len());
    assert!(!again.app().is_dirty());
    assert_eq!(again.app().file_notice(), None);
}

#[test]
fn work_survives_a_crash_through_the_autosave() {
    let mut lumen = Lumen::start_with(
        lumen_project::testing::TempDir::new("e2e-recover"),
        |launch| launch.autosaving_after(Duration::from_millis(50)),
    );
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    let names = lumen.track_names();
    let autosaves = lumen.dir().join("autosave");
    lumen.wait_for("an autosave", |_| {
        std::fs::read_dir(&autosaves).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == PROJECT_EXTENSION))
        })
    });

    // Lumen stops without closing normally; the next start offers the work.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch);
    assert!(again.track_names().is_empty());
    again.click("Recover");
    again.wait_for("the recovered project", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    assert_eq!(again.track_names(), names);
    assert!(
        again.app().is_dirty(),
        "recovered work is unsaved until saved"
    );
}

/// A crash report from an earlier run is offered once (owner, M3 6c).
#[test]
fn a_crash_report_from_last_time_is_offered_once() {
    let dir = lumen_project::testing::TempDir::new("e2e-crash-report");
    let crashes = dir.path().join("crashes");
    std::fs::create_dir_all(&crashes).unwrap();
    std::fs::write(crashes.join("crash-001791468187.txt"), "Lumen crash report").unwrap();
    let mut lumen = Lumen::start_with(dir, |launch| launch);
    assert!(lumen.shows("Dismiss"), "the crash banner");
    lumen.click("Dismiss");
    assert!(!lumen.shows("Dismiss"));
    let dir = lumen.crash();
    let again = Lumen::start_with(dir, |launch| launch);
    assert!(!again.shows("Dismiss"), "shown once");
}
