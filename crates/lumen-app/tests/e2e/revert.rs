//! Revert to saved (M9 slice R4, ADR-0105).

use eframe::egui::{Event, Key, Modifiers};
use lumen_app::testing::project_file;

use super::Lumen;
use super::copies::{command, with_outside_audio};

/// A saved project of one track with a second track added since: unsaved.
fn saved_then_changed(name: &str) -> (Lumen, std::path::PathBuf) {
    let (mut lumen, _kick) = with_outside_audio(name);
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("the new track", |app| app.project().tracks.len() == 2);
    assert!(lumen.app().is_dirty());
    (lumen, path)
}

#[test]
fn reverting_reads_the_saved_project_back_and_keeps_what_it_dropped() {
    let (mut lumen, path) = saved_then_changed("revert-whole");

    command(&mut lumen, "Revert to saved");
    lumen.frames(3);
    assert!(lumen.app().revert_prompt_open(), "it asks first");
    assert_eq!(
        lumen.app().project().tracks.len(),
        2,
        "and has changed nothing"
    );
    lumen.click("Revert");
    lumen.wait_for("the revert", |app| {
        !app.is_busy() && !app.is_dirty() && app.reverted_changes().is_some()
    });

    assert_eq!(lumen.app().project().tracks.len(), 1, "as it was saved");
    assert!(!lumen.app().revert_prompt_open());
    assert_eq!(lumen.app().undo_label(), None, "a fresh open: no history");
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Reverted to the saved Song")),
        "{:?}",
        lumen.app().note()
    );
    // The unsaved version is in the project's Backups/Reverted folder.
    let kept = lumen.app().reverted_changes().unwrap().to_path_buf();
    assert!(kept.is_file(), "{}", kept.display());
    // (Compared as resolved paths: the temp folder has a short Windows name.)
    let folder = path.parent().unwrap().join("Backups").join("Reverted");
    assert_eq!(
        std::fs::canonicalize(kept.parent().unwrap()).unwrap(),
        std::fs::canonicalize(folder).unwrap(),
    );

    // The message offers it: opening it brings the dropped track back.
    lumen.click("Open them");
    lumen.wait_for("the kept version", |app| {
        !app.is_busy() && app.project().tracks.len() == 2
    });
    assert_eq!(lumen.app().reverted_changes(), None, "the message is done");
}

#[test]
fn cancelling_the_prompt_leaves_everything_as_it_is() {
    let (mut lumen, path) = saved_then_changed("revert-cancel");
    command(&mut lumen, "Revert to saved");
    lumen.frames(3);
    assert!(lumen.app().revert_prompt_open());
    lumen.click("Cancel");
    lumen.frames(5);
    assert!(!lumen.app().revert_prompt_open());
    assert!(lumen.app().is_dirty());
    assert_eq!(lumen.app().project().tracks.len(), 2);
    assert!(!path.parent().unwrap().join("Backups").exists());
    assert_eq!(lumen.app().reverted_changes(), None);
}

#[test]
fn escape_closes_the_prompt_without_reverting() {
    let (mut lumen, _path) = saved_then_changed("revert-escape");
    command(&mut lumen, "Revert to saved");
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    assert!(!lumen.app().revert_prompt_open());
    assert_eq!(lumen.app().project().tracks.len(), 2);
}

#[test]
fn a_project_with_nothing_to_revert_says_so_and_asks_nothing() {
    // Never saved: there is no file to read back.
    let (mut lumen, _kick) = with_outside_audio("revert-unsaved");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Revert to saved".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(!lumen.app().revert_prompt_open());
    assert!(
        lumen.app().palette_open(),
        "the palette stays open and says why"
    );
    lumen.key(Modifiers::NONE, Key::Escape);

    // Saved and unchanged: nothing to drop.
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path);
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Revert to saved".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(!lumen.app().revert_prompt_open());
    assert!(lumen.app().palette_open());
}
