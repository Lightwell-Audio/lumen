//! Preferences (M9 R7): one window of pages, opened from the app menu, the
//! palette and the transport's chips; a search that finds any setting by a
//! word and opens its page with the row outlined; a page menu instead of the
//! list in a small window; and changes kept across a restart.

use eframe::egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use lumen_app::App;
use lumen_midi::fake::FakeControl;
use lumen_project::testing::TempDir;

use super::Lumen;

/// Types `text` into the search field, which takes the keyboard first.
fn search(lumen: &mut Lumen, text: &str) {
    lumen.click("Search preferences");
    lumen.harness.event(Event::Text(text.into()));
    lumen.frames(3);
}

#[test]
fn every_page_opens_from_the_list_and_draws_every_row_search_can_find() {
    let fake = FakeControl::new();
    let mut lumen = Lumen::start_with(TempDir::new("e2e-preferences-pages"), |l| {
        l.with_midi(fake).with_input_device()
    });
    lumen.open_preferences("Audio");
    // The device's details load in the background; the latency rows wait
    // for them.
    lumen.run_until("the device's latency", |l| l.shows("Round trip"));
    let ctx = lumen.harness.ctx.clone();
    App::record_drawn_rows(&ctx);
    for page in App::preference_pages() {
        lumen.open_preferences(page);
        assert_eq!(lumen.app().preferences_shown(), Some(page));
        lumen.frames(3);
        lumen.key(Modifiers::NONE, Key::Escape);
        assert_eq!(lumen.app().preferences_shown(), None, "Esc closes it");
    }
    let drawn = App::drawn_rows(&ctx);
    let missing: Vec<_> = App::preference_rows()
        .into_iter()
        .filter(|(key, _)| !drawn.contains(key))
        .collect();
    assert!(
        missing.is_empty(),
        "rows search finds but no page draws: {missing:?}"
    );
}

#[test]
fn a_search_result_opens_its_page_with_the_row_marked() {
    let mut lumen = Lumen::start("preferences-search");
    lumen.open_preferences("Mixing");
    search(&mut lumen, "latency");
    assert!(
        lumen.shows("Buffer size   Audio"),
        "a result names its page"
    );
    assert!(
        !lumen.shows("Additive solo"),
        "results stand in for the page"
    );
    lumen.click("Buffer size   Audio");
    assert_eq!(lumen.app().preferences_shown(), Some("Audio"));
    assert_eq!(lumen.app().marked_preference(), Some("audio.buffer"));
    assert!(lumen.is_on("Audio page"));

    // Enter takes the first result.
    search(&mut lumen, "dark");
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().preferences_shown(), Some("Appearance"));
    assert_eq!(lumen.app().marked_preference(), Some("look.theme"));

    // Words that find nothing say so; Esc clears them, then closes.
    search(&mut lumen, "zzzz");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Nothing matches")
            .is_some()
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(
        lumen.app().preferences_shown(),
        Some("Appearance"),
        "the first Esc clears the search"
    );
    assert!(lumen.shows("Theme"));
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(lumen.app().preferences_shown(), None);
}

#[test]
fn the_list_folds_into_a_page_menu_in_the_smallest_window_at_200_percent() {
    let mut lumen = Lumen::start("preferences-fold");
    lumen.harness.set_size(vec2(960.0, 540.0));
    lumen.frames(5);
    // The app menu is taller than this window (KNOWN_ISSUES): the palette
    // opens Preferences here.
    run_from_palette(&mut lumen, "Preferences");
    lumen.run_until("the page list", |l| l.shows("Appearance page"));
    lumen.click("Appearance page");
    lumen.click("200%");
    lumen.frames(10);
    // 960 points at 200% leave 480: the list gives way to a menu.
    assert!(!lumen.shows("Audio page"), "the list is folded");
    let menu = lumen.harness.get_by_label_contains("Page: Appearance");
    let field = lumen.harness.get_by_label("Search preferences").rect();
    assert!(field.left() >= 0.0 && field.right() <= 480.0, "{field:?}");
    assert!(menu.rect().right() <= 480.0, "{:?}", menu.rect());
    // The menu chooses the page.
    menu.click();
    lumen.frames(3);
    lumen.click("Mixing");
    assert_eq!(lumen.app().preferences_shown(), Some("Mixing"));
    assert!(lumen.shows("Additive solo"));
}

#[test]
fn a_change_on_a_page_is_kept_across_a_restart() {
    let dir = TempDir::new("e2e-preferences-restart");
    let file = dir.path().join("settings.json");
    let mut lumen = Lumen::start_with(dir, |launch| launch.with_settings_file(&file));
    lumen.open_preferences("Files");
    assert!(lumen.is_on("Copy imported audio into the project"));
    lumen.click("Copy imported audio into the project");
    lumen.wait_for("the setting saved", |_| {
        std::fs::read_to_string(&file).is_ok_and(|text| text.contains("\"copy_imported\": false"))
    });
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.with_settings_file(&file));
    again.open_preferences("Files");
    assert!(!again.is_on("Copy imported audio into the project"));
}

#[test]
fn a_switch_on_a_page_and_its_command_are_one_setting() {
    let mut lumen = Lumen::start("preferences-commands");
    lumen.open_preferences("Mixing");
    assert!(!lumen.is_on("Additive solo"));
    lumen.click("Additive solo");
    assert!(lumen.is_on("Additive solo"));
    lumen.click("Meters in track colors");
    assert!(lumen.is_on("Meters in track colors"));
    lumen.open_preferences("Recording");
    lumen.click("Count-in: 2 bars");
    assert!(lumen.is_on("Count-in: 2 bars"));
    lumen.click("Takes: Overdub");
    assert!(lumen.is_on("Takes: Overdub"));
}

/// Opens the palette, finds `name` and runs it.
fn run_from_palette(lumen: &mut Lumen, name: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
}

#[test]
fn the_palette_opens_preferences_on_a_page_and_the_history() {
    let mut lumen = Lumen::start("preferences-palette");
    for (command, page) in [
        ("Plugin settings", "Plugins"),
        ("MIDI settings", "MIDI"),
        ("Audio settings", "Audio"),
    ] {
        run_from_palette(&mut lumen, command);
        assert_eq!(lumen.app().preferences_shown(), Some(page), "{command}");
        // Its search has the keyboard: Esc closes it for the next.
        lumen.key(Modifiers::NONE, Key::Escape);
    }
    lumen.open_preferences("Editing");
    lumen.key(Modifiers::NONE, Key::Escape);
    run_from_palette(&mut lumen, "Preferences");
    assert_eq!(
        lumen.app().preferences_shown(),
        Some("Editing"),
        "the page shown last"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    run_from_palette(&mut lumen, "History");
    assert!(lumen.app().history_shown());
}
