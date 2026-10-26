//! The Project settings window (M9 slice R4, ADR-0105): its controls change
//! the project in one undo step each, and what they set is saved with it.

use eframe::egui::{Event, Key, Modifiers};
use lumen_app::testing::project_file;
use lumen_core::pan::PanLaw;
use lumen_project::model::Color;

use super::Lumen;
use super::copies::command;

/// The window, opened by its command as a person without a shortcut would.
fn open_window() -> Lumen {
    let mut lumen = Lumen::start("project-settings");
    show(&mut lumen);
    lumen
}

fn show(lumen: &mut Lumen) {
    command(lumen, "Project settings");
    lumen.run_until("the window", |l| l.shows("Notes"));
    // The scroll area measures its content on the first frames.
    lumen.frames(5);
    assert!(lumen.app().project_settings_open());
}

#[test]
fn the_window_opens_from_the_palette_and_closes_with_escape() {
    let mut lumen = open_window();
    for part in ["Audio", "Mixing", "New tracks", "Notes"] {
        assert!(lumen.shows(part), "{part} is a section");
    }
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    assert!(!lumen.app().project_settings_open());
}

#[test]
fn the_sample_rate_is_one_undo_step() {
    let mut lumen = open_window();
    assert_eq!(lumen.app().project().sample_rate, 48_000);
    lumen.click("Sample rate: 48.0 kHz");
    lumen.run_until("the list", |l| l.shows("44.1 kHz"));
    lumen.click("44.1 kHz");
    lumen.wait_for("the rate", |app| app.project().sample_rate == 44_100);
    assert_eq!(lumen.app().undo_label(), Some("Change project rate"));
    assert!(lumen.shows("Sample rate: 44.1 kHz"), "the window shows it");

    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the undo", |app| app.project().sample_rate == 48_000);
}

#[test]
fn the_pan_law_and_the_crossfade_switch_change_the_project() {
    let mut lumen = open_window();
    assert_eq!(lumen.app().project().pan_law, PanLaw::ConstantPower);
    lumen.click("Pan law: \u{2212}3 dB, constant power");
    lumen.run_until("the list", |l| l.shows("\u{2212}6 dB, linear"));
    lumen.click("\u{2212}6 dB, linear");
    lumen.wait_for("the law", |app| app.project().pan_law == PanLaw::Linear);
    assert_eq!(lumen.app().undo_label(), Some("Change pan law"));

    assert!(lumen.app().project().auto_crossfade);
    lumen.click("Automatic crossfades");
    lumen.wait_for("the switch", |app| !app.project().auto_crossfade);
    lumen.click("Automatic crossfades");
    lumen.wait_for("the switch back", |app| app.project().auto_crossfade);
}

#[test]
fn new_tracks_take_the_chosen_colour_until_the_palette_is_chosen_again() {
    let mut lumen = open_window();
    assert_eq!(lumen.app().project().new_track_color, None);
    lumen.click("New track colour 3");
    lumen.wait_for("the colour", |app| {
        app.project().new_track_color == Some(Color(0x5E8C6A))
    });
    assert_eq!(lumen.app().undo_label(), Some("Change new track colour"));

    // The window stays open while a track is made from the keyboard.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("the new track", |app| app.project().tracks.len() == 1);
    assert_eq!(lumen.app().project().tracks[0].color, Color(0x5E8C6A));

    show(&mut lumen);
    lumen.click("Colours in turn");
    lumen.wait_for("the palette", |app| app.project().new_track_color.is_none());
}

#[test]
fn typing_notes_is_one_undo_step_and_the_notes_are_saved_with_the_project() {
    let mut lumen = open_window();
    lumen.click("Project notes");
    lumen.frames(2);
    lumen.harness.event(Event::Text("Verse in".into()));
    lumen.frames(3);
    lumen.harness.event(Event::Text(" D minor".into()));
    lumen.frames(3);
    assert_eq!(lumen.app().project().notes, "Verse in D minor");

    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    assert!(!lumen.app().project_settings_open());
    assert_eq!(lumen.app().undo_label(), Some("Edit project notes"));

    // Saved with the project, and read back from the file.
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen_project::load(&path).unwrap();
    assert_eq!(saved.notes, "Verse in D minor");

    // One undo takes all the typing back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the undo", |app| app.project().notes.is_empty());
}

#[test]
fn what_the_window_sets_survives_save_and_reopen() {
    let mut lumen = open_window();
    lumen.click("Sample rate: 48.0 kHz");
    lumen.run_until("the list", |l| l.shows("44.1 kHz"));
    lumen.click("44.1 kHz");
    lumen.click("New track colour 2");
    lumen.click("Project notes");
    lumen.frames(2);
    lumen.harness.event(Event::Text("Sketch".into()));
    lumen.frames(3);
    lumen.wait_for("the settings", |app| {
        app.project().sample_rate == 44_100
            && app.project().notes == "Sketch"
            && app.project().new_track_color.is_some()
    });
    let path = project_file(lumen.dir(), "Song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    lumen.app_mut().open_project_file(path);
    lumen.wait_for("the reopened project", |app| {
        !app.is_busy() && app.project().notes == "Sketch"
    });
    let project = lumen.app().project();
    assert_eq!(project.sample_rate, 44_100);
    assert_eq!(project.new_track_color, Some(Color(0x7A6A9E)));

    // The window shows the reopened project's values, not the old ones.
    show(&mut lumen);
    assert!(lumen.shows("Sample rate: 44.1 kHz"));
}
