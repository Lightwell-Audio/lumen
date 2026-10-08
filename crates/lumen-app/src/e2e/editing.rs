//! Tracks: adding, muting, renaming, moving, deleting, undo and redo.

use eframe::egui::{Event, Key, Modifiers};

use super::Lumen;

#[test]
fn tracks_are_added_from_the_menu_and_the_keyboard() {
    let mut lumen = Lumen::start("add-tracks");
    assert!(lumen.track_names().is_empty(), "a new project is empty");
    assert!(lumen.shows("Empty track area: double-click to add an audio track"));

    lumen.click("Add track");
    // Menu items are named with their shortcut, as a screen reader says them.
    lumen.click("Add audio track Ctrl+T");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    let kinds: Vec<_> = lumen
        .app()
        .document
        .project()
        .tracks
        .iter()
        .map(|t| t.kind)
        .collect();
    use lumen_project::model::TrackKind::{Audio, Midi};
    assert_eq!(kinds, [Audio, Audio, Midi]);
    // Each has a header with its controls.
    for name in lumen.track_names() {
        assert!(lumen.shows(&format!("Mute {name}")), "{name}");
        assert!(lumen.shows(&format!("Arm {name}")), "{name}");
    }
}

#[test]
fn mute_solo_and_arm_toggle_and_undo_as_one_step_each() {
    let mut lumen = Lumen::start("toggles");
    lumen.key(Modifiers::COMMAND, Key::T);
    let name = lumen.track_names()[0].clone();
    let mute = format!("Mute {name}");

    lumen.click(&mute);
    assert!(lumen.is_on(&mute));
    assert!(lumen.app().document.project().tracks[0].mute);
    lumen.click(&format!("Solo {name}"));
    assert!(lumen.app().document.project().tracks[0].solo);
    lumen.click(&format!("Arm {name}"));
    assert!(lumen.app().document.project().tracks[0].armed);

    // Undo steps back through arm, solo and mute; redo brings mute back.
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::Z);
    }
    let track = &lumen.app().document.project().tracks[0];
    assert!(!track.armed && !track.solo && !track.mute);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(lumen.app().document.project().tracks[0].mute);
    assert!(lumen.is_on(&mute));
}

#[test]
fn the_keyboard_alone_selects_and_moves_tracks() {
    let mut lumen = Lumen::start("keyboard");
    for _ in 0..3 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    let names = lumen.track_names();
    let ids: Vec<_> = lumen
        .app()
        .document
        .project()
        .tracks
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(lumen.app().arrangement.selected, [ids[2]], "the newest");

    // Up selects the one above; Shift+Up extends the selection.
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert_eq!(lumen.app().arrangement.selected, [ids[1]]);
    lumen.key(Modifiers::SHIFT, Key::ArrowUp);
    let selected = &lumen.app().arrangement.selected;
    assert!(selected.len() == 2 && selected.contains(&ids[0]) && selected.contains(&ids[1]));

    // Alt+Down moves both below the third.
    lumen.key(Modifiers::ALT, Key::ArrowDown);
    assert_eq!(
        lumen.track_names(),
        [names[2].as_str(), names[0].as_str(), names[1].as_str()]
    );
}

#[test]
fn tracks_are_renamed_moved_and_deleted() {
    let mut lumen = Lumen::start("rename-move");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    let before = lumen.track_names();

    // F2 renames the selected track (the one just added): type, Enter.
    lumen.key(Modifiers::NONE, Key::F2);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("Drums".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.track_names(), [before[0].as_str(), "Drums"]);

    // Alt+Up moves it above the first; undo puts it back.
    lumen.key(Modifiers::ALT, Key::ArrowUp);
    assert_eq!(lumen.track_names(), ["Drums", before[0].as_str()]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.track_names(), [before[0].as_str(), "Drums"]);

    // Delete removes the selection; undo restores it.
    lumen.click("Drums: double-click to rename");
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(lumen.track_names(), [before[0].as_str()]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.track_names(), [before[0].as_str(), "Drums"]);
}
