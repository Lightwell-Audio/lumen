//! Files: importing audio, saving and opening, autosave and recovery.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::{PROJECT_EXTENSION, project_file};

use super::{Lumen, wav};

/// An autosave `session` left behind, with a project whose one track is
/// `track`, written at `written`.
fn crashed_copy(dir: &Path, session: &str, track: &str, written: u64) -> PathBuf {
    let autosaves = dir.join("autosave");
    std::fs::create_dir_all(&autosaves).unwrap();
    let project = lumen_project::testing::project_of(
        track,
        vec![lumen_project::model::Track::new(
            lumen_project::model::Id(1),
            track,
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0x4F81BD),
        )],
    );
    let copy = autosaves.join(format!("{session}.{PROJECT_EXTENSION}"));
    lumen_project::save(&project, &copy, &Default::default()).expect("save");
    std::fs::write(
        autosaves.join(format!("{session}.json")),
        format!(r#"{{"project":null,"name":"{track}","written":{written}}}"#),
    )
    .unwrap();
    copy
}

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
    // The file can appear before the worker hands its session lock back.
    // A simulated crash drops the app while the worker thread can still
    // run: wait for the current state's completed write so the app owns it.
    lumen.wait_for("an autosave of both tracks", |app| app.autosave_finished());

    // Lumen stops without closing normally; the next start offers the work.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch);
    assert!(again.track_names().is_empty());
    again.recover();
    again.wait_for("the recovered project", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    assert_eq!(again.track_names(), names);
    assert!(
        again.app().is_dirty(),
        "recovered work is unsaved until saved"
    );
}

/// `--recover-latest` (sweep M4a: the folder is scanned on a worker): the
/// newest leftover opens on the first answer, with no click.
#[test]
fn recover_latest_opens_the_newest_leftover_at_start() {
    let dir = lumen_project::testing::TempDir::new("e2e-recover-latest");
    crashed_copy(dir.path(), "older", "Alpha", 100);
    crashed_copy(dir.path(), "newer", "Beta", 300);
    let mut lumen = Lumen::start_with(dir, |launch| launch.recovering_latest());
    lumen.wait_for("the newest leftover to open", |app| {
        !app.is_busy() && app.project().tracks.iter().any(|t| t.name == "Beta")
    });
    lumen.frames(5);
    assert!(
        !lumen.shows("Recover"),
        "it opened at once, so nothing is offered"
    );
}

/// A project named on the command line wins over `--recover-latest`: it
/// opens, and the leftover is offered rather than adopted silently.
#[test]
fn a_named_project_wins_over_recover_latest_and_the_leftover_stays_offered() {
    let dir = lumen_project::testing::TempDir::new("e2e-recover-latest-named");
    let leftover = crashed_copy(dir.path(), "newer", "Beta", 300);
    let named = dir.path().join(format!("Named.{PROJECT_EXTENSION}"));
    let project = lumen_project::testing::project_of(
        "Named",
        vec![lumen_project::model::Track::new(
            lumen_project::model::Id(1),
            "Named",
            lumen_project::model::TrackKind::Audio,
            lumen_project::model::Color(0x4F81BD),
        )],
    );
    lumen_project::save(&project, &named, &Default::default()).expect("save");
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch.opening(named.clone()).recovering_latest()
    });
    lumen.wait_for("the named project", |app| {
        !app.is_busy() && app.project().tracks.iter().any(|t| t.name == "Named")
    });
    lumen.run_until("the recovery banner", |lumen| lumen.shows("Recover"));
    lumen.frames(5);
    assert!(leftover.exists(), "the leftover is kept, not adopted");
    assert!(
        !lumen
            .app()
            .project()
            .tracks
            .iter()
            .any(|t| t.name == "Beta"),
        "the named project is what opened"
    );
}

/// Discarding a crash's copy deletes it and offers the next one, newest
/// first. The folder is read on a worker (sweep M4a), so the banner follows
/// the scan's answer rather than the click.
#[test]
fn discarding_a_crash_copy_removes_it_and_offers_the_next_one() {
    let dir = lumen_project::testing::TempDir::new("e2e-recover-discard");
    let autosaves = dir.path().join("autosave");
    std::fs::create_dir_all(&autosaves).unwrap();
    let copy = |session: &str, name: &str, written: u64| {
        let file = autosaves.join(format!("{session}.{PROJECT_EXTENSION}"));
        std::fs::write(&file, b"{}").unwrap();
        let note = format!("{{\"project\": null, \"name\": \"{name}\", \"written\": {written}}}");
        std::fs::write(autosaves.join(format!("{session}.json")), note).unwrap();
        file
    };
    let older = copy("one", "Alpha", 100);
    let newer = copy("two", "Beta", 300);

    let mut lumen = Lumen::start_with(dir, |launch| launch);
    lumen.run_until("the newest leftover", |l| {
        l.harness
            .query_by_label_contains("Unsaved changes to Beta")
            .is_some()
    });
    lumen.click("Discard");
    lumen.run_until("the next leftover", |l| {
        l.harness
            .query_by_label_contains("Unsaved changes to Alpha")
            .is_some()
    });
    assert!(!newer.exists(), "the discarded copy went with its note");
    assert!(!autosaves.join("two.json").exists());
    lumen.click("Discard");
    lumen.run_until("no leftover at all", |l| {
        l.harness
            .query_by_label_contains("did not close normally")
            .is_none()
    });
    lumen_testkit::wait_until("the last copy to go", || !older.exists());
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

/// The empty arrangement's prompt keeps inside the lanes above the master's
/// row. With the detail panel open in a short window little room is left, and
/// the prompt gives up its button rather than run over the master and the time
/// ruler (the owner's report); in a tall window it is whole.
#[test]
fn the_empty_prompt_stays_inside_the_lanes_in_a_short_window() {
    use eframe::egui::vec2;
    use egui_kittest::kittest::Queryable;

    const BUTTON: &str = "Import audio or MIDI…";
    let mut lumen = Lumen::start("empty-short");
    lumen.harness.set_size(vec2(960.0, 900.0));
    lumen.frames(20);
    assert!(lumen.shows(BUTTON), "a tall window shows the button");

    let master_top = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("Master")
            .map(|n| n.rect())
            .filter(|r| r.top() > 60.0)
            .map(|r| r.top())
            .fold(f32::INFINITY, f32::min)
    };
    let mut hidden = 0;
    for height in [700.0, 600.0, 540.0, 480.0, 440.0] {
        lumen.harness.set_size(vec2(960.0, height));
        lumen.frames(20);
        match lumen.harness.query_by_label(BUTTON) {
            Some(button) => {
                let (button, master) = (button.rect(), master_top(&lumen));
                assert!(
                    button.bottom() <= master + 0.5,
                    "at {height} the button {button:?} reaches the master row at {master}"
                );
            }
            None => hidden += 1,
        }
    }
    assert!(hidden > 0, "the smallest window has no room for the button");
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

/// The backup is best effort (sweep M1): with `Backups` taken by a file, the
/// save still commits, says the previous version was not kept, and the saved
/// project reopens.
#[test]
fn a_save_whose_backup_fails_still_saves_and_says_so() {
    let mut lumen = Lumen::start("save-no-backup");
    lumen.key(Modifiers::COMMAND, Key::T);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    // A file where the backups folder would go: every backup fails.
    let folder = path.parent().expect("the project's folder");
    std::fs::write(folder.join("Backups"), b"not a folder").unwrap();
    let name = lumen.track_names()[0].clone();
    lumen.click(&format!("Mute {name}"));
    assert!(lumen.app().is_dirty());
    let saved = lumen.app().project().clone();
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    let notice = lumen.app().file_notice().unwrap_or_default().to_owned();
    assert!(
        notice.starts_with("Saved, but the previous version could not be backed up"),
        "{notice}"
    );
    assert!(
        notice.ends_with("Your project itself was saved."),
        "{notice}"
    );
    assert!(!notice.contains('\u{2026}'), "read in full: {notice}");

    // What was saved reopens as it was.
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().project().tracks[0].mute, saved.tracks[0].mute);
    assert!(!again.app().is_dirty());
}

#[test]
fn enter_in_the_save_question_saves_and_goes_on() {
    let mut lumen = Lumen::start("save-question-enter");
    lumen.key(Modifiers::COMMAND, Key::T);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    // An unsaved change, then a new project: the question asks first.
    let name = lumen.track_names()[0].clone();
    lumen.click(&format!("Mute {name}"));
    assert!(lumen.app().is_dirty());
    super::copies::command(&mut lumen, "New project");
    lumen.frames(2);
    assert!(lumen.shows("Don't save"), "the question");

    // Enter gives Save, its main answer: the change is saved, then the new
    // project starts.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the new project", |app| {
        !app.is_busy() && app.project_path().is_none()
    });
    assert!(!lumen.shows("Don't save"));
    let saved = lumen_project::load(&path).expect("the saved project");
    assert!(saved.tracks[0].mute, "the mute was saved");
}

#[test]
fn enter_for_the_save_question_reaches_no_other_window() {
    // Never saved: Save asks where, and that dialog is cancelled here, so
    // the project stays and only what Enter reached changes.
    let mut lumen = super::clip_commands::audio("save-question-alone");
    assert!(lumen.app().is_dirty());

    // The bounce popover, which Enter starts and closes, is open under the
    // question.
    let clip = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(clip);
    super::copies::command(&mut lumen, "Bounce");
    lumen.frames(3);
    super::copies::command(&mut lumen, "New project");
    lumen.frames(2);
    assert!(lumen.shows("Don't save"), "the question");
    assert!(lumen.shows("Include effects"), "the popover, under it");

    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(!lumen.shows("Don't save"), "Enter answered the question");
    assert!(
        lumen.shows("Include effects") && !lumen.app().bounce_started(),
        "and reached no other window"
    );
    assert!(lumen.app().project_path().is_none() && lumen.app().is_dirty());
}

#[test]
fn a_new_or_opened_project_does_not_play_on_from_the_last() {
    // Open or New while the song plays: the song belonged to the project
    // that closed, so playback stops rather than carrying on over the next.
    let mut lumen = Lumen::start("open-while-playing");
    lumen.key(Modifiers::COMMAND, Key::T);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    super::copies::command(&mut lumen, "New project");
    lumen.wait_for("the new project to stop playback", |app| {
        app.project_path().is_none() && !app.is_playing()
    });

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    lumen.app_mut().open_project_file(path.clone());
    lumen.wait_for("the opened project to stop playback", |app| {
        !app.is_busy() && app.project_path().is_some() && !app.is_playing()
    });
}
