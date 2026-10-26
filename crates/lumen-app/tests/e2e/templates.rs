//! Save as template and New from template (M9 slice R4, ADR-0105).

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_app::testing::project_file;

use super::Lumen;
use super::copies::{command, with_outside_audio};

/// Types `name` into the window that names a template, as a person would:
/// the name offered is selected, so typing replaces it.
fn type_name(lumen: &mut Lumen, name: &str) {
    lumen.frames(3);
    assert!(
        lumen.app().template_window_open(),
        "the naming window shows"
    );
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
}

/// Saves the open project as the template `name` and waits until it is listed.
fn save_template(lumen: &mut Lumen, name: &str) {
    command(lumen, "Save as template");
    type_name(lumen, name);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the template", |app| {
        !app.is_busy() && !app.template_window_open() && app.templates().iter().any(|t| t == name)
    });
}

#[test]
fn a_project_becomes_a_template_and_a_new_project_starts_from_it_with_its_audio() {
    let (mut lumen, _kick) = with_outside_audio("template-round");
    // Copying into projects is off: a template holds its audio anyway.
    lumen.app_mut().set_copy_imported(false);

    save_template(&mut lumen, "Beat");
    let folder = lumen.app().templates_folder().join("Beat");
    assert!(folder.join("Beat.lumen").is_file());
    assert!(
        folder.join("Samples/Imported/kick.wav").is_file(),
        "the audio is in the template"
    );
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("Saved the template Beat")),
        "{:?}",
        lumen.app().note()
    );
    // The open project is as it was: not saved, not named, not remembered.
    assert_eq!(lumen.app().project_path(), None);
    assert!(lumen.app().is_dirty());
    assert!(lumen.app().recent_projects().is_empty());

    // The open project has unsaved changes: starting another asks first.
    command(&mut lumen, "New from template Beat");
    lumen.click("Don't save");
    lumen.wait_for("the new project", |app| {
        !app.is_busy() && !app.is_dirty() && app.project().tracks.len() == 1
    });
    assert_eq!(lumen.app().project_path(), None, "a new, untitled project");
    assert!(lumen.app().recent_projects().is_empty());
    assert_eq!(lumen.app().undo_label(), None, "nothing done yet");
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|note| note.starts_with("New project from the template Beat")),
        "{:?}",
        lumen.app().note()
    );

    // The first Save copies the template's audio in, whatever the preference
    // says, and leaves the template as it was.
    let mine = project_file(lumen.dir(), "Mine");
    lumen.app_mut().answer_next_dialog(vec![mine.clone()]);
    lumen.key(Modifiers::COMMAND, Key::S);
    lumen.wait_for("the save", |app| {
        !app.is_busy() && app.project_path().is_some()
    });
    assert!(
        mine.parent()
            .unwrap()
            .join("Samples/Imported/kick.wav")
            .is_file(),
        "the audio is in the new project"
    );
    assert!(folder.join("Samples/Imported/kick.wav").is_file());
    assert!(folder.join("Beat.lumen").is_file());
}

#[test]
fn a_name_that_is_a_template_is_replaced_only_by_a_second_enter() {
    let (mut lumen, _kick) = with_outside_audio("template-replace");
    save_template(&mut lumen, "Beat");
    let file = lumen
        .app()
        .templates_folder()
        .join("Beat")
        .join("Beat.lumen");
    let first = std::fs::read(&file).unwrap();

    // Another track, so the replacement is different.
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(5);

    command(&mut lumen, "Save as template");
    type_name(&mut lumen, "Beat");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(
        lumen.app().template_window_open(),
        "the first Enter asks, it does not replace"
    );
    assert!(!lumen.app().is_busy());
    assert_eq!(std::fs::read(&file).unwrap(), first, "nothing written yet");

    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the replacement", |app| {
        !app.is_busy() && !app.template_window_open()
    });
    assert_ne!(std::fs::read(&file).unwrap(), first, "the template is new");
    assert_eq!(
        lumen
            .app()
            .templates()
            .iter()
            .filter(|t| t.as_str() == "Beat")
            .count(),
        1,
        "one template of that name"
    );
}

#[test]
fn escape_leaves_the_naming_window_and_saves_nothing() {
    let (mut lumen, _kick) = with_outside_audio("template-escape");
    command(&mut lumen, "Save as template");
    type_name(&mut lumen, "Beat");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    assert!(!lumen.app().template_window_open());
    assert!(!lumen.app().is_busy());
    assert!(lumen.app().templates().is_empty());
    assert!(
        !lumen.app().templates_folder().join("Beat").exists(),
        "no folder was made"
    );
}

#[test]
fn a_template_is_listed_in_the_palette_by_its_name() {
    let (mut lumen, _kick) = with_outside_audio("template-palette");
    save_template(&mut lumen, "Beat");
    // Typed in part, the template is found by name.
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("template Beat".into()));
    lumen.frames(3);
    // The row is a button, and its text a label inside it.
    assert!(
        lumen
            .harness
            .query_all_by_label("New from template \u{201C}Beat\u{201D}")
            .next()
            .is_some(),
        "the palette lists the template"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
}

#[test]
fn the_templates_folder_is_made_and_shown_on_request() {
    let mut lumen = Lumen::start("template-folder");
    assert!(!lumen.app().templates_folder().exists());
    command(&mut lumen, "Open templates folder");
    lumen.wait_for("the folder", |app| {
        app.templates_folder().is_dir() && !app.revealed_template_folders().is_empty()
    });
    assert_eq!(
        lumen.app().revealed_template_folders(),
        [lumen.app().templates_folder()]
    );
}

#[test]
fn projects_and_templates_have_their_own_folders_in_the_user_folder() {
    // Owner, MT-PROJ-1: both were plain folders in one place and could not be
    // told apart. Now `Lumen/Projects` and `Lumen/Templates` hold them, and
    // the dialogs start in the first.
    let dir = lumen_project::testing::TempDir::new("user-folder");
    let user = dir.path().join("Lumen");
    let mut lumen = Lumen::start_with(dir, |launch| launch.with_user_dir(&user));
    assert_eq!(lumen.app().templates_folder(), user.join("Templates"));
    assert_eq!(
        lumen.app().dialog_folder(),
        Some(user.join("Projects")),
        "an unsaved project's dialogs start in Projects"
    );
    lumen.app_mut().set_copy_imported(false);
    save_template(&mut lumen, "Beat");
    assert!(user.join("Templates/Beat/Beat.lumen").is_file());
}
