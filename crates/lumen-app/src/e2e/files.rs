//! Files: importing audio, saving and opening, autosave and recovery.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};

use super::{Lumen, wav};
use crate::files;

/// The clips of every track, as (track name, clip count).
fn clips(lumen: &Lumen) -> Vec<(String, usize)> {
    lumen
        .app()
        .document
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
    // Its audio is ready for the engine, with a waveform.
    let media = lumen.app().document.project().media[0].id.0;
    let item = lumen
        .app()
        .lanes
        .library
        .get(media)
        .expect("in the library");
    assert!(item.peaks.is_some(), "a waveform was built");

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.transport.state().playing);
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
    assert!(lumen.app().document.is_dirty());

    // Save as the Save dialog would, once a path is chosen.
    let path = files::project_file_for(&lumen.dir().join("Song"));
    lumen.app_mut().start_save(path.clone());
    lumen.wait_for("the save", |app| {
        app.saving.is_none() && !app.document.is_dirty()
    });
    assert!(path.exists(), "{}", path.display());
    let saved = lumen.app().document.project().clone();

    // A fresh Lumen opens it: the same project, its audio loaded.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.open = Some(path.clone()));
    again.wait_for("the project to open", |app| {
        app.opening.is_none() && !app.lanes.is_loading()
    });
    again.frames(5);
    let opened = again.app().document.project();
    assert_eq!(opened.tracks, saved.tracks);
    assert_eq!(opened.media.len(), saved.media.len());
    assert!(!again.app().document.is_dirty());
    assert!(
        again.app().file_notice.is_none(),
        "{:?}",
        again.app().file_notice
    );
}

#[test]
fn work_survives_a_crash_through_the_autosave() {
    let mut lumen = Lumen::start_with(
        lumen_project::testing::TempDir::new("e2e-recover"),
        |launch| launch.autosave_after = Some(Duration::from_millis(50)),
    );
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    let names = lumen.track_names();
    let autosaves = lumen.dir().join("autosave");
    lumen.wait_for("an autosave", |_| {
        std::fs::read_dir(&autosaves).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == files::EXTENSION))
        })
    });

    // Lumen stops without closing normally; the next start offers the work.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |_| {});
    assert!(again.track_names().is_empty());
    again.click("Recover");
    again.wait_for("the recovered project", |app| {
        app.opening.is_none() && app.document.project().tracks.len() == 2
    });
    assert_eq!(again.track_names(), names);
    assert!(
        again.app().document.is_dirty(),
        "recovered work is unsaved until saved"
    );
}
