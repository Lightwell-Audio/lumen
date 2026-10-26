//! Save a copy and Collect and save (M9 slice R4, ADR-0105).

use std::path::Path;

use eframe::egui::{Event, Key, Modifiers};
use lumen_app::testing::{PROJECT_EXTENSION, project_file};

use super::{Lumen, wav};

/// Runs a command by name through the palette, as a user without a shortcut
/// does.
pub(super) fn command(lumen: &mut Lumen, name: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(!lumen.app().palette_open());
}

/// A project with one clip of `kick.wav`, a file outside any project folder.
pub(super) fn with_outside_audio(name: &str) -> (Lumen, std::path::PathBuf) {
    let mut lumen = Lumen::start(name);
    std::fs::create_dir_all(lumen.dir().join("Downloads")).unwrap();
    let kick = wav(&lumen.dir().join("Downloads").join("kick.wav"), 1.0);
    lumen.drop_files(std::slice::from_ref(&kick));
    lumen.wait_for("the file to load", |app| {
        !app.is_busy() && app.project().tracks.len() == 1
    });
    (lumen, kick)
}

/// Whether the open project's file is `path`. The worker names it by its
/// resolved path, which differs from the temp folder's spelling (Windows
/// short names, macOS's /private/var): the same file is what counts.
fn is_at(lumen: &Lumen, path: &Path) -> bool {
    let resolved = |p: &Path| std::fs::canonicalize(p).ok();
    lumen
        .app()
        .project_path()
        .is_some_and(|open| resolved(open).is_some() && resolved(open) == resolved(path))
}

fn imported(project_file: &Path) -> std::path::PathBuf {
    project_file
        .parent()
        .unwrap()
        .join("Samples")
        .join("Imported")
        .join("kick.wav")
}

#[test]
fn a_copy_holds_all_the_audio_and_leaves_the_open_project_as_it_was() {
    let (mut lumen, kick) = with_outside_audio("copy-whole");
    // Copying into projects is off: a copy takes the audio anyway.
    lumen.app_mut().set_copy_imported(false);
    assert!(lumen.app().is_dirty());
    assert_eq!(lumen.app().project_path(), None);

    let copy = project_file(lumen.dir(), "Song copy");
    lumen.app_mut().answer_next_dialog(vec![copy.clone()]);
    command(&mut lumen, "Save a copy");
    lumen.wait_for("the copy", |app| !app.is_busy());

    assert!(copy.is_file(), "{}", copy.display());
    assert!(imported(&copy).is_file(), "the audio is in the copy");
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Saved a copy as Song copy")),
        "{:?}",
        lumen.app().note()
    );
    // The open project has not moved, been saved, or been remembered.
    assert_eq!(lumen.app().project_path(), None);
    assert!(lumen.app().is_dirty());
    assert!(lumen.app().recent_projects().is_empty());
    assert_eq!(lumen.app().undo_label(), Some("Import kick"), "no edit");
    assert!(kick.is_file(), "the original is untouched");

    // The copy stands alone: a fresh Lumen opens it with the originals gone.
    std::fs::remove_file(&kick).unwrap();
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(copy.clone()));
    again.wait_for("the copy to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().file_notice(), None, "no audio is missing");
    assert_eq!(again.app().project().tracks.len(), 1);
}

#[test]
fn a_copy_cannot_be_written_over_the_open_projects_own_file() {
    let (mut lumen, _kick) = with_outside_audio("copy-over");
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let before = std::fs::read(&path).unwrap();

    lumen.app_mut().answer_next_dialog(vec![path.clone()]);
    command(&mut lumen, "Save a copy");
    lumen.wait_for("the refusal", |app| !app.is_busy());
    lumen.frames(3);

    let notice = lumen.app().file_notice().expect("the refusal is shown");
    assert!(
        notice.starts_with("Save a copy failed.") && notice.contains("own file"),
        "{notice}"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is as it was"
    );
    assert!(is_at(&lumen, &path), "{:?}", lumen.app().project_path());
}

#[test]
fn collect_and_save_gathers_outside_audio_into_the_project_and_saves() {
    let (mut lumen, kick) = with_outside_audio("collect");
    lumen.app_mut().set_copy_imported(false);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    assert!(
        !imported(&path).exists(),
        "with copying off, a save leaves the audio where it was"
    );

    command(&mut lumen, "Collect and save");
    lumen.wait_for("the collect", |app| !app.is_busy());
    lumen.frames(3);

    assert!(imported(&path).is_file(), "the audio is in the project");
    assert!(!lumen.app().is_dirty());
    assert!(is_at(&lumen, &path), "{:?}", lumen.app().project_path());
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Collected 1 audio file")),
        "{:?}",
        lumen.app().note()
    );

    // The project holds what it plays: reopened without the original.
    std::fs::remove_file(&kick).unwrap();
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(5);
    assert_eq!(again.app().file_notice(), None, "no audio is missing");
}

#[test]
fn collecting_a_project_that_was_never_saved_asks_where_first() {
    let (mut lumen, _kick) = with_outside_audio("collect-new");
    lumen.app_mut().set_copy_imported(false);
    let path = project_file(lumen.dir(), "First");
    lumen.app_mut().answer_next_dialog(vec![path.clone()]);
    command(&mut lumen, "Collect and save");
    lumen.wait_for("the collect", |app| !app.is_busy());

    assert!(path.is_file() && imported(&path).is_file());
    assert!(is_at(&lumen, &path), "{:?}", lumen.app().project_path());
    assert!(!lumen.app().is_dirty());
    assert_eq!(lumen.app().recent_projects().len(), 1, "now remembered");
}

#[test]
fn cancelling_the_dialog_copies_nothing() {
    let (mut lumen, _kick) = with_outside_audio("copy-cancel");
    command(&mut lumen, "Save a copy");
    lumen.frames(5);
    assert!(!lumen.app().is_busy());
    assert_eq!(lumen.app().note(), None);
    assert!(lumen.app().is_dirty());
    let folders = std::fs::read_dir(lumen.dir())
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == PROJECT_EXTENSION))
        .count();
    assert_eq!(folders, 0, "no project file was written");
}

#[test]
fn a_missing_audio_file_is_listed_and_the_rest_is_copied() {
    let (mut lumen, kick) = with_outside_audio("copy-missing");
    let snare = wav(&lumen.dir().join("Downloads").join("snare.wav"), 1.0);
    lumen.drop_files(std::slice::from_ref(&snare));
    lumen.wait_for("both files", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    std::fs::remove_file(&snare).unwrap();

    let copy = project_file(lumen.dir(), "Song copy");
    lumen.app_mut().answer_next_dialog(vec![copy.clone()]);
    command(&mut lumen, "Save a copy");
    lumen.wait_for("the copy", |app| !app.is_busy());
    lumen.frames(3);

    assert!(imported(&copy).is_file(), "the file that exists is copied");
    let notice = lumen.app().file_notice().expect("the missing file is told");
    assert!(
        notice.contains("could not be found") && notice.contains("snare.wav"),
        "{notice}"
    );
    assert!(kick.is_file());
}
