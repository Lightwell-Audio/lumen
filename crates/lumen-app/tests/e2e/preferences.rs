//! Preferences (M9 R7): one window of pages, opened from the app menu, the
//! palette and the transport's chips; a search that finds any setting by a
//! word and opens its page with the row outlined; a page menu instead of the
//! list in a small window; and changes kept across a restart.

use eframe::egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use lumen_app::App;
use lumen_midi::Direction;
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
fn enter_on_a_result_with_the_keyboard_opens_that_result() {
    // Round 2 of the review: Enter anywhere took the first result, so a
    // result reached with Tab could not be opened from the keyboard.
    let mut lumen = Lumen::start("preferences-tab");
    lumen.open_preferences("Mixing");
    search(&mut lumen, "latency");
    // Latency is first, then Buffer size: the second takes the keyboard.
    lumen.harness.get_by_label("Buffer size   Audio").focus();
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().preferences_shown(), Some("Audio"));
    assert_eq!(lumen.app().marked_preference(), Some("audio.buffer"));
}

#[test]
fn a_result_below_the_fold_is_scrolled_into_view() {
    // Round 1 of the review: the mark drawn before a result was chosen
    // overwrote the new one's scroll, so a row below the page's fold stayed
    // there while its outline faded.
    let mut lumen = Lumen::start("preferences-scroll");
    lumen.harness.set_size(vec2(1280.0, 720.0));
    lumen.frames(5);
    lumen.open_preferences("Audio");
    let tone = "Test tone (1 kHz, \u{2212}20 dBFS)";
    let bottom = 720.0 - 40.0;
    assert!(
        lumen.harness.get_by_label(tone).rect().top() > bottom,
        "the test tone starts below the fold"
    );
    search(&mut lumen, "test tone");
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().marked_preference(), Some("audio.test_tone"));
    lumen.run_until("the test tone scrolled into view", |l| {
        let rect = l.harness.get_by_label(tone).rect();
        rect.top() > 0.0 && rect.bottom() <= bottom
    });
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
fn the_midi_page_lines_its_switches_up_in_columns() {
    // The owner's run of Part 2: names were centred, and each switch sat
    // wherever its name ended.
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Keys");
    fake.plug(Direction::Output, "A synth with a much longer name");
    fake.plug(Direction::Output, "Keys");
    let mut lumen = Lumen::start_with(TempDir::new("e2e-preferences-midi"), |l| l.with_midi(fake));
    lumen.open_preferences("MIDI");
    lumen.run_until("the ports", |l| l.shows("A synth with a much longer name"));
    let rect = |l: &Lumen, name: &str| l.harness.get_by_label(name).rect();
    let track = [
        rect(&lumen, "Track Keys input"),
        rect(&lumen, "Track A synth with a much longer name output"),
        rect(&lumen, "Track Keys output"),
    ];
    let second = [
        rect(&lumen, "Remote Keys input"),
        rect(&lumen, "Sync A synth with a much longer name output"),
        rect(&lumen, "Sync Keys output"),
    ];
    for column in [track, second] {
        for r in column {
            assert!(
                (r.right() - column[0].right()).abs() < 0.5,
                "a column's switches share their right edge: {column:?}"
            );
        }
    }
    let edge = rect(&lumen, "Play notes while editing").right();
    assert!(
        (second[0].right() - edge).abs() < 0.5,
        "the last column ends where the page's other switches do"
    );
    // A name starts at the same place in every row, whatever its length.
    let keys = lumen.harness.get_all_by_label("Keys").next().unwrap();
    let names = [keys.rect(), rect(&lumen, "A synth with a much longer name")];
    assert!((names[0].left() - names[1].left()).abs() < 0.5, "{names:?}");
}

#[test]
fn a_plugin_folder_can_be_removed_from_the_plugins_page() {
    let dir = TempDir::new("e2e-preferences-folders");
    let folders = ["first", "second"].map(|name| {
        let folder = dir.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    });
    // Plugins on, with empty folders: nothing is found, so no host is run.
    let host = dir.path().join("no-host");
    let cache = dir.path().join("plugin-cache.json");
    let mut lumen = Lumen::start_with(dir, |l| l.with_plugins(Vec::new(), &host, cache));
    lumen.open_preferences("Plugins");
    for folder in &folders {
        lumen.app_mut().answer_next_dialog(vec![folder.clone()]);
        lumen.click("Add plugin folder\u{2026}");
        let shown = folder.display().to_string();
        lumen.run_until("the folder listed", move |l| l.shows(&shown));
    }
    let first = folders[0].display().to_string();
    lumen.click(&format!("Remove {first}"));
    lumen.frames(3);
    assert!(!lumen.shows(&first), "the folder is gone from the page");
    assert!(lumen.shows(&folders[1].display().to_string()));
    assert_eq!(
        lumen.app().plugin_folders(),
        [folders[1].display().to_string()],
        "and from the settings"
    );
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
