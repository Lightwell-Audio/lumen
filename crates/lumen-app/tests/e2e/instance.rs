//! One Lumen per settings file (M9 R8, ADR-0117): what the window does with
//! a project a later start hands over, and the guard that keeps a project
//! from replacing a recording or an export. Requests are injected as a
//! later start sends them; the real endpoint is covered by the smoke tests.

use std::path::{Path, PathBuf};

use eframe::egui::{Key, Modifiers};
use lumen_app::App;
use lumen_project::model::{Color, Id, Track, TrackKind};
use lumen_project::testing::{TempDir, project_of};

use super::{Lumen, wav};

const VOX: Id = Id(21);

/// Saves a project with one track named `track` at `dir/<name>.lumen`.
fn song(dir: &Path, name: &str, track: &str) -> PathBuf {
    let project = project_of(
        name,
        vec![Track::new(VOX, track, TrackKind::Audio, Color(0x4F81BD))],
    );
    let path = dir.join(format!("{name}.lumen"));
    lumen_project::save(&project, &path, &Default::default()).expect("save");
    path
}

fn shows_track(app: &App, name: &str) -> bool {
    app.project().tracks.iter().any(|t| t.name == name)
}

/// Waits for the window's answer to a hand-over.
fn answer(lumen: &mut Lumen, sent: &lumen_app::testing::HandOver) -> String {
    let got = std::cell::RefCell::new(None);
    lumen.run_until("the answer", |_| {
        let mut got = got.borrow_mut();
        if got.is_none() {
            *got = sent.reply();
        }
        got.is_some()
    });
    got.into_inner().unwrap()
}

#[test]
fn a_handed_over_project_opens_when_nothing_is_unsaved() {
    let mut lumen = Lumen::start("instance-open");
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
    assert!(!lumen.app().save_prompt_open());
}

#[test]
fn unsaved_changes_are_asked_about_first() {
    let mut lumen = Lumen::start("instance-prompt");
    let take = wav(&lumen.dir().join("kick.wav"), 0.5);
    lumen.drop_files(&[take]);
    lumen.wait_for("the import", |app| !app.is_busy() && app.is_dirty());
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.wait_for("the save question", |app| app.save_prompt_open());
    lumen.click("Don't save");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
}

#[test]
fn the_project_already_open_only_comes_to_the_front() {
    let dir = TempDir::new("e2e-instance-same");
    let path = song(dir.path(), "Night Drive", "Keys");
    let opening = path.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(opening));
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
    let sent = lumen.app_mut().hand_over(Some(path));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.frames(10);
    assert!(!lumen.app().save_prompt_open());
    assert!(!lumen.app().is_dirty());

    let shown = lumen.app_mut().hand_over(None);
    assert_eq!(answer(&mut lumen, &shown), "accepted");
}

#[test]
fn a_file_that_is_not_a_project_is_refused_with_the_reason() {
    let mut lumen = Lumen::start("instance-refuse");
    let take = wav(&lumen.dir().join("kick.wav"), 0.5);
    let sent = lumen.app_mut().hand_over(Some(take));
    let reply = answer(&mut lumen, &sent);
    assert!(reply.starts_with("rejected: kick.wav is not a"), "{reply}");
    let gone = lumen.dir().join("Gone.lumen");
    let sent = lumen.app_mut().hand_over(Some(gone));
    assert!(answer(&mut lumen, &sent).contains("was not found"));
    assert!(
        lumen.app().project().tracks.is_empty(),
        "nothing was imported"
    );
}

#[test]
fn a_dropped_project_opens_and_nothing_else_in_the_drop_is_added() {
    let mut lumen = Lumen::start("instance-drop");
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let take = wav(&lumen.dir().join("kick.wav"), 0.5);
    lumen.drop_files(&[take, other]);
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
    lumen.frames(5);
    assert_eq!(
        lumen.app().project().tracks.len(),
        1,
        "kick.wav was not added"
    );
    assert!(lumen.app().project().media.is_empty());
    assert_eq!(
        lumen.app().note(),
        Some(
            "Opened Night Drive. Only the project opens: the 1 other file in the drop was not added."
        )
    );
}

/// Starts on the input test device with Vox, an armed-able audio track,
/// in a saved project.
fn recording_ready(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-instance-{name}"));
    let path = song(dir.path(), "Take", "Vox");
    let mut lumen = Lumen::start_with(dir, move |l| l.with_input_device().opening(path));
    lumen.wait_for("the project", |app| shows_track(app, "Vox"));
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.frames(30);
    lumen.click("Arm Vox");
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the recording", |app| app.recording() && app.is_playing());
    // A take stopped before its first input block has nothing to place, so
    // wait for frames on disk: a slow runner can take longer than a frame.
    let vox = lumen.app().project().tracks[0].id;
    lumen.wait_for("the take's first frames", move |app| {
        app.audio_take_frames(vox).is_some_and(|f| f > 0)
    });
    lumen
}

#[test]
fn while_recording_the_project_waits_and_cancel_keeps_the_take() {
    let mut lumen = recording_ready("cancel");
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.frames(5);
    assert_eq!(
        lumen.app().holding_words().as_deref(),
        Some("Night Drive.lumen opens when recording stops. Nothing is replaced before then.")
    );
    assert!(lumen.app().recording(), "recording goes on");
    lumen.click("Cancel");
    lumen.frames(5);
    assert!(!lumen.app().is_holding());
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the take", |app| {
        !app.recording() && !app.audio_takes_placing() && app.project().tracks[0].clips.len() == 1
    });
    lumen.frames(10);
    assert!(shows_track(lumen.app(), "Vox"), "nothing else opened");
}

#[test]
fn stop_and_open_places_the_take_then_asks_and_save_keeps_it() {
    let mut lumen = recording_ready("stop-open");
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.run_until("the notice", |l| l.shows("Stop and open"));
    lumen.click("Stop and open");
    // The take is placed before anything opens; it is an unsaved change.
    lumen.wait_for("the save question", |app| app.save_prompt_open());
    assert!(!lumen.app().recording() && !lumen.app().audio_takes_placing());
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1, "the take");
    let take_song = lumen.dir().join("Take.lumen");
    lumen.click("Save");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
    let saved = lumen_project::load(&take_song).expect("the saved take");
    assert_eq!(saved.tracks[0].clips.len(), 1, "Save kept the take");
}

#[test]
fn a_recording_started_behind_the_save_question_holds_dont_save_and_asks_again() {
    let dir = TempDir::new("e2e-instance-behind");
    let path = song(dir.path(), "Take", "Vox");
    let mut lumen = Lumen::start_with(dir, move |l| l.with_input_device().opening(path));
    lumen.wait_for("the project", |app| shows_track(app, "Vox"));
    lumen.wait_for("the device's inputs", |app| app.open_inputs() == 2);
    lumen.frames(30);
    lumen.click("Arm Vox"); // An unsaved change.
    assert!(lumen.app().is_dirty());
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.wait_for("the save question", |app| app.save_prompt_open());
    lumen.key(Modifiers::NONE, Key::F9);
    lumen.wait_for("the recording", |app| app.recording());
    // The take is the change asked about again; one stopped before its
    // first input block has nothing to place (as in `recording_ready`).
    let vox = lumen.app().project().tracks[0].id;
    lumen.wait_for("the take's first frames", move |app| {
        app.audio_take_frames(vox).is_some_and(|f| f > 0)
    });
    lumen.click("Don't save");
    lumen.frames(5);
    assert!(
        lumen.app().is_holding(),
        "Don't save waits for the recording"
    );
    assert!(shows_track(lumen.app(), "Vox"));
    lumen.key(Modifiers::NONE, Key::Space);
    // The take is a change Don't save did not cover: asked again.
    lumen.wait_for("the question again", |app| {
        !app.is_holding() && app.save_prompt_open()
    });
    lumen.click("Don't save");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
}

#[test]
fn while_exporting_the_project_waits_until_the_export_ends() {
    let mut lumen = Lumen::start("instance-export");
    let take = wav(&lumen.dir().join("take.wav"), 5.0);
    lumen.drop_files(&[take]);
    lumen.wait_for("the import", |a| {
        !a.is_busy() && !a.project().tracks.is_empty()
    });
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    lumen.frames(5);
    lumen.click("Render mode: Offline");
    lumen.click("Real-time");
    let output = lumen.dir().join("mix.wav");
    lumen.app_mut().answer_next_dialog(vec![output]);
    lumen.click("Export audio…");
    lumen.wait_for("the export", |a| a.audio_export_busy());
    let other = song(lumen.dir(), "Night Drive", "Keys");
    let sent = lumen.app_mut().hand_over(Some(other));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.frames(5);
    let words = lumen.app().holding_words().expect("it waits");
    assert!(words.ends_with("opens when the export is done. Nothing is replaced before then."));
    assert!(
        !lumen.shows("Stop and open"),
        "an export has no Stop and open"
    );
    lumen.run_until("cancel control", |l| l.shows("Cancel export"));
    lumen.click("Cancel export");
    // The import is unsaved: once the export has stopped, the question.
    lumen.wait_for("the save question", |app| {
        !app.audio_export_busy() && !app.is_holding() && app.save_prompt_open()
    });
    lumen.click("Don't save");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
}

// ── Opening .lumen files (FA A) ───────────────────────────────────────────

const SWITCH: &str = "Open .lumen files with Lumen";

fn files_page(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    lumen.app_mut().fake_association(None, None, None);
    lumen.open_preferences("Files");
    lumen.run_until("the switch", |l| l.shows(SWITCH));
    lumen
}

#[cfg(windows)]
#[test]
fn the_files_switch_registers_this_copy_and_removes_it() {
    let mut lumen = files_page("instance-assoc");
    assert_eq!(lumen.app().association(), "off");
    lumen.click(SWITCH);
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "this");
    assert!(lumen.is_on(SWITCH));
    assert!(lumen.shows("Double-click a project in File Explorer to open it here."));
    lumen.click(SWITCH);
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "off");
    assert!(!lumen.app().wants_association());

    // The palette's command does the same.
    super::copies::command(&mut lumen, "Open .lumen files");
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "this");
    assert!(
        lumen
            .app()
            .note()
            .unwrap()
            .starts_with("Projects now open with")
    );
}

#[cfg(windows)]
#[test]
fn another_copy_is_named_and_use_this_copy_takes_it_over() {
    let mut lumen = files_page("instance-assoc-other");
    let other = lumen.dir().join("Old").join("lumen.exe");
    lumen.app_mut().fake_association(Some(&other), None, None);
    lumen.frames(3);
    assert!(!lumen.is_on(SWITCH), "another copy is not this one");
    assert!(lumen.app().association().starts_with("other: "));
    lumen.click("Use this copy");
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "this");
}

#[cfg(windows)]
#[test]
fn the_files_row_draws_the_status_a_worker_read() {
    let mut lumen = files_page("instance-assoc-cache");
    assert_eq!(lumen.app().association(), "off");
    // Changed in Windows while the page shows: drawing keeps the answer the
    // worker brought, and never reads the registry itself (sweep M4b).
    let other = lumen.dir().join("Old").join("lumen.exe");
    lumen.app_mut().change_association_hive(Some(&other));
    lumen.frames(10);
    assert_eq!(
        lumen.app().association(),
        "off",
        "the row did not look again"
    );
    assert!(!lumen.is_on(SWITCH));
    // A window opening asks again: the next answer shows.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.run_until("Preferences closed", |l| !l.shows(SWITCH));
    lumen.open_preferences("Files");
    lumen.run_until("the worker's new answer", |l| {
        l.app().association().starts_with("other: ")
    });
    assert!(lumen.shows("Use this copy"));
}

#[cfg(windows)]
#[test]
fn the_persons_own_choice_links_to_default_apps_and_a_failed_write_is_shown() {
    use egui_kittest::kittest::Queryable;
    let mut lumen = files_page("instance-assoc-choice");
    lumen
        .app_mut()
        .fake_association(None, Some("Other.Thing"), None);
    lumen.frames(3);
    assert!(lumen.shows("Open Default apps"));
    lumen.click("Open Default apps");
    assert_eq!(lumen.app().default_apps_asked(), 1);

    lumen
        .app_mut()
        .fake_association(None, None, Some("access is denied"));
    lumen.click(SWITCH);
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "off");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Windows did not take the change: access is denied")
            .is_some()
    );
}

#[test]
fn the_files_row_is_found_by_search() {
    let mut lumen = files_page("instance-assoc-search");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.open_preferences("Mixing");
    lumen.click("Search preferences");
    lumen
        .harness
        .event(eframe::egui::Event::Text("double-click".into()));
    lumen.frames(3);
    lumen.click("Open .lumen files   Files");
    assert_eq!(lumen.app().marked_preference(), Some("files.associate"));
    #[cfg(not(windows))]
    assert!(lumen.shows("On this system, project files are set up when the app is installed."));
}

#[test]
fn an_edit_made_while_a_file_is_read_is_asked_about() {
    let dir = TempDir::new("e2e-instance-edit-while-read");
    let path = song(dir.path(), "Take", "Vox");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| {
        shows_track(app, "Vox") && !app.is_dirty()
    });
    let other = song(lumen.dir(), "Night Drive", "Keys");
    // Nothing is unsaved when the file is asked for; the edit comes before
    // it arrives, so the choice did not cover it.
    lumen.app_mut().open_project_file(other);
    lumen
        .app_mut()
        .set_audio_input(VOX, lumen_project::model::AudioInput::mono(1));
    lumen.wait_for("the save question", |app| app.save_prompt_open());
    assert!(shows_track(lumen.app(), "Vox"), "nothing replaced yet");
    lumen.click("Don't save");
    lumen.wait_for("the project", |app| shows_track(app, "Keys"));
}

#[test]
fn revert_to_saved_is_refused_while_recording() {
    let mut lumen = recording_ready("revert");
    assert!(lumen.app().is_dirty(), "arming is an unsaved change");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Revert to saved".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(!lumen.app().revert_prompt_open());
    assert!(lumen.app().recording(), "recording goes on");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the take", |app| {
        !app.recording() && !app.audio_takes_placing()
    });
}

#[test]
fn recover_waits_while_the_open_project_has_unsaved_changes() {
    let mut lumen = Lumen::start_with(TempDir::new("e2e-instance-recover"), |launch| {
        launch.autosaving_after(std::time::Duration::from_millis(50))
    });
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("an autosave", |app| app.autosave_finished());
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch);
    again.key(Modifiers::COMMAND, Key::T);
    again.key(Modifiers::COMMAND, Key::T);
    again.run_until("the recovery banner", |lumen| lumen.shows("Recover"));
    assert!(
        again.shows("Recovering replaces the open project: save your changes to Untitled first.")
    );
    again.click("Recover");
    again.frames(10);
    assert_eq!(
        again.app().project().tracks.len(),
        2,
        "nothing was recovered"
    );
    assert!(again.shows("Recover"), "the offer stays");
}

#[test]
fn a_project_handed_over_while_another_is_read_opens_after_it() {
    let mut lumen = Lumen::start("instance-second-open");
    let first = song(lumen.dir(), "Alpha", "Bass");
    let second = song(lumen.dir(), "Night Drive", "Keys");
    // The first file is being read when the second is handed over: the
    // second waits behind it instead of being dropped.
    lumen.app_mut().open_project_file(first);
    let sent = lumen.app_mut().hand_over(Some(second));
    assert_eq!(answer(&mut lumen, &sent), "accepted");
    lumen.wait_for("the later project", |app| {
        shows_track(app, "Keys") && !app.is_holding() && !app.is_busy()
    });
    lumen.frames(30);
    assert!(shows_track(lumen.app(), "Keys"), "the later one stays open");
}

#[cfg(windows)]
#[test]
fn a_normal_start_registers_this_copy_only_when_none_is_and_it_is_wanted() {
    let mut lumen = files_page("instance-assoc-start");
    // Nothing registered, the setting on as it is by default: registered.
    assert!(lumen.app().wants_association());
    lumen.app_mut().associate_as_a_normal_start();
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "this");
    assert!(lumen.is_on(SWITCH));

    // Another copy owns it: left alone.
    let other = lumen.dir().join("Old").join("lumen.exe");
    lumen.app_mut().fake_association(Some(&other), None, None);
    lumen.app_mut().associate_as_a_normal_start();
    lumen.frames(3);
    assert!(lumen.app().association().starts_with("other: "));

    // Turned off by the person: never turned on again by a start.
    lumen.app_mut().fake_association(None, None, None);
    lumen.app_mut().associate_as_a_normal_start();
    lumen.frames(3);
    lumen.click(SWITCH);
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "off");
    assert!(!lumen.app().wants_association());
    lumen.app_mut().associate_as_a_normal_start();
    lumen.frames(3);
    assert_eq!(lumen.app().association(), "off");
}
