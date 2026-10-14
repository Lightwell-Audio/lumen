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

/// The M3 review's H1: audio comes in from the keyboard alone. Ctrl+T adds
/// a track and selects it; Import audio puts the chosen file on it at the
/// playhead, as one undo step.
#[test]
fn audio_imports_from_the_keyboard_onto_the_selected_track() {
    let mut lumen = Lumen::start("import-keys");
    let kick = wav(&lumen.dir().join("kick.wav"), 1.0);
    lumen.key(Modifiers::COMMAND, Key::T);
    let track = lumen.track_names()[0].clone();

    lumen.app_mut().answer_next_dialog(vec![kick]);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    lumen.wait_for("the file to load", |app| {
        !app.is_busy() && app.project().tracks[0].clips.len() == 1
    });
    assert_eq!(clips(&lumen), [(track, 1)], "on the selected track");
    let clip = &lumen.app().project().tracks[0].clips[0];
    assert_eq!(clip.start, lumen.app().playhead(), "at the playhead");
    assert_eq!(lumen.app().undo_label(), Some("Import kick"));

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clips(&lumen)[0].1, 0, "one undo step");
}

/// With nothing selected, imported files go on new tracks; a cancelled
/// dialog changes nothing. The empty arrangement offers the command too.
#[test]
fn importing_with_nothing_selected_makes_tracks_and_cancelling_does_nothing() {
    let mut lumen = Lumen::start("import-new");
    let kick = wav(&lumen.dir().join("kick.wav"), 1.0);
    let snare = wav(&lumen.dir().join("snare.wav"), 1.0);

    lumen.click("Import audio or MIDI…");
    lumen.frames(3);
    assert!(clips(&lumen).is_empty(), "cancelled: nothing added");

    lumen.app_mut().answer_next_dialog(vec![kick, snare]);
    lumen.click("Import audio or MIDI…");
    lumen.wait_for("the files to load", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    assert_eq!(
        lumen.track_names(),
        ["kick", "snare"],
        "in the order chosen"
    );
    assert!(
        !lumen.shows("Import audio or MIDI…"),
        "the empty state has gone"
    );
}

/// Files dropped together become tracks in the order they were dropped,
/// not the order they finish loading (a short file loads first; the cause
/// of flaky runs, seen in the M3 review).
#[test]
fn files_dropped_together_keep_their_order() {
    let mut lumen = Lumen::start("drop-order");
    let long = wav(&lumen.dir().join("long.wav"), 30.0);
    let short = wav(&lumen.dir().join("short.wav"), 0.1);
    lumen.drop_files(&[long, short]);
    lumen.wait_for("both files", |app| app.project().tracks.len() == 2);
    assert_eq!(lumen.track_names(), ["long", "short"]);
    assert_eq!(lumen.app().undo_label(), Some("Import 2 files"), "one step");
}

#[test]
fn a_project_from_before_the_mixer_opens_with_a_master_and_saves_as_current() {
    // M4 slice 2: format v7 routes tracks. A version 6 project (the golden
    // fixture) opens with its master moved out of the tracks, plays, and
    // saves in the current version.
    let dir = lumen_project::testing::TempDir::new("e2e-v6-to-v7");
    let folder = dir.path().join("Old");
    std::fs::create_dir_all(folder.join("Samples")).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumen-project/tests/fixtures/v6/golden.lumen");
    let path = folder.join(format!("Old.{PROJECT_EXTENSION}"));
    std::fs::copy(fixture, &path).unwrap();
    wav(&folder.join("Samples/kick.wav"), 1.0);

    let mut lumen = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    lumen.wait_for("the project to open", |app| !app.is_busy());
    lumen.frames(5);
    let project = lumen.app().project();
    assert_eq!(project.master.name, "Master");
    assert!(
        project
            .tracks
            .iter()
            .all(|t| t.kind != lumen_project::model::TrackKind::Master),
        "the master is no longer among the tracks"
    );
    assert_eq!(lumen.track_names().len(), project.tracks.len());

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    lumen.key(Modifiers::NONE, Key::Space);

    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = std::fs::read_to_string(&path).unwrap();
    let version = lumen_project::migrate::CURRENT_VERSION;
    assert!(
        saved.contains(&format!("\"version\": {version}")),
        "saved as version {version}"
    );
    assert!(saved.contains("\"master\": {"), "with its master");
}
