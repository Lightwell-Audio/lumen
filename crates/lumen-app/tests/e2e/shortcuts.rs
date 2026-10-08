//! Changing shortcuts (M3 slice 7c): the Keyboard shortcuts window (record
//! a chord, clashes reassigned or cancelled, reset), rebinding from the
//! palette with Ctrl+Enter, and saved shortcuts used at start.

use eframe::egui::{Event, Key, Modifiers};

use super::Lumen;
use super::properties::with_two_clips;

/// Opens the Keyboard shortcuts window from the palette and searches its
/// list for `query`.
fn open_and_search(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(Event::Text("keyboard shortcuts".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(lumen.app().shortcuts_open());
    lumen.click("Search shortcuts");
    lumen.harness.event(Event::Text(query.into()));
    lumen.frames(2);
}

#[test]
fn a_shortcut_recorded_in_the_window_runs_and_is_kept() {
    let (mut lumen, _, _) = with_two_clips("shortcut-record");
    open_and_search(&mut lumen, "fit song");
    lumen.click("Change Fit song to width shortcut W");
    lumen.key(Modifiers::ALT, Key::W);
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["Alt+W"]);
    assert_eq!(
        lumen.app().saved_shortcuts()["view.fit_width"],
        ["Alt+W"],
        "kept for the settings file"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().shortcuts_open(), "Esc closes the window");

    // Zoomed in, the new chord fits the song again; W alone no longer does.
    let fitted = lumen.app().view().zoom;
    lumen.key(Modifiers::NONE, Key::Plus);
    lumen.frames(30);
    let zoomed = lumen.app().view().zoom;
    assert!(zoomed > fitted);
    lumen.key(Modifiers::NONE, Key::W);
    lumen.frames(30);
    assert_eq!(lumen.app().view().zoom, zoomed, "W is free now");
    lumen.key(Modifiers::ALT, Key::W);
    lumen.frames(30);
    // Fitting a short song can zoom in or out: it changes the view.
    assert_ne!(lumen.app().view().zoom, zoomed, "Alt+W fits");
}

#[test]
fn a_clash_is_reassigned_or_cancelled_and_reset_restores() {
    let mut lumen = Lumen::start("shortcut-clash");
    open_and_search(&mut lumen, "fit song");
    lumen.click("Change Fit song to width shortcut W");
    // Esc while recording cancels the recording, not the window.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.app().shortcuts_open());
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["W"]);

    lumen.click("Change Fit song to width shortcut W");
    lumen.key(Modifiers::COMMAND, Key::D);
    assert!(lumen.shows("Ctrl+D is used by Duplicate."));
    assert_eq!(lumen.app().shortcuts_of("Duplicate"), ["Ctrl+D"], "not yet");
    lumen.click("Reassign");
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["Ctrl+D"]);
    assert!(lumen.app().shortcuts_of("Duplicate").is_empty());

    // Reset all puts both back, and nothing is left to save.
    lumen.click("Reset all");
    assert_eq!(lumen.app().shortcuts_of("Duplicate"), ["Ctrl+D"]);
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["W"]);
    assert!(lumen.app().saved_shortcuts().is_empty());
}

#[test]
fn ctrl_enter_in_the_palette_records_new_keys() {
    let mut lumen = Lumen::start("shortcut-palette");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("fit song".into()));
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::Enter);
    assert!(lumen.shows("Press the new keys for Fit song to width \u{B7} Esc cancels"));
    lumen.key(Modifiers::SHIFT, Key::W);
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["Shift+W"]);
    assert!(lumen.app().palette_open(), "the palette stays open");
}

#[test]
fn saved_shortcuts_are_used_from_the_start() {
    let dir = lumen_project::testing::TempDir::new("e2e-shortcut-saved");
    let lumen = Lumen::start_with(dir, |launch| {
        launch.with_saved_shortcut("view.fit_width", &["Alt+W"])
    });
    assert_eq!(lumen.app().shortcuts_of("Fit song to width"), ["Alt+W"]);
    assert_eq!(lumen.app().shortcuts_of("Play / Stop"), ["Space"]);
}
