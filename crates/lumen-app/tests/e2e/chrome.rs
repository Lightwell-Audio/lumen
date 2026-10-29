//! M4.5 slice 3, the interface audit's chrome: the panel toggles keep a
//! button's look and say whether they are on, the empty mixer's ghost strip
//! adds a track, and the time signature is one field that takes "7/8".

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::Lumen;

#[test]
fn the_copy_imported_audio_switch_saves_without_another_setting_change() {
    let dir = lumen_project::testing::TempDir::new("copy-imported-setting");
    let file = dir.path().join("settings.json");
    let mut lumen = Lumen::start_with(dir, |launch| launch.with_settings_file(&file));
    lumen.click("Lumen");
    lumen.click("More ways to save");
    lumen.click("Copy imported audio into the project");
    lumen.wait_for("copy setting saved", |_| {
        std::fs::read_to_string(&file).is_ok_and(|text| text.contains("\"copy_imported\": false"))
    });
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.with_settings_file(&file));
    again.click("Lumen");
    again.click("More ways to save");
    assert!(!again.is_on("Copy imported audio into the project"));
}

#[test]
fn the_panel_toggles_and_the_mixer_tab_say_whether_they_are_on() {
    let mut lumen = Lumen::start("toggles");
    // ("Browser" also names its pane, so the Detail toggle is checked.)
    assert!(lumen.is_on("Detail"));
    lumen.click("Detail");
    assert!(!lumen.is_on("Detail"), "a click switches it off");
    lumen.click("Detail");
    assert!(lumen.is_on("Detail"));
    assert!(lumen.is_on("Mixer tab"), "the open tab");
}

#[test]
fn the_empty_mixers_ghost_strip_adds_a_track() {
    let mut lumen = Lumen::start("ghost-strip");
    assert!(lumen.app().project().tracks.is_empty());
    lumen.click("Add track in the mixer");
    assert_eq!(lumen.app().project().tracks.len(), 1);
    assert!(
        !lumen.shows("Add track in the mixer"),
        "a strip took its place"
    );
}

#[test]
fn the_time_signature_is_one_field_that_takes_a_typed_meter() {
    let mut lumen = Lumen::start("meter");
    lumen
        .harness
        .get_by_label_contains("Time signature: 4/4")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("7/8".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    let meter = lumen.app().project().meter[0].meter;
    assert_eq!((meter.numerator, meter.denominator), (7, 8));
    assert!(lumen.shows("Time signature: 7/8"));
    // Not a meter Lumen has: nothing changes.
    lumen
        .harness
        .get_by_label_contains("Time signature: 7/8")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("7/7".into()));
    lumen.key(Modifiers::NONE, Key::Enter);
    let meter = lumen.app().project().meter[0].meter;
    assert_eq!((meter.numerator, meter.denominator), (7, 8));
}

#[test]
fn the_browser_and_the_devices_tab_stop_before_they_stop_being_useful() {
    let mut lumen = Lumen::start("panel-limits");
    // Tall enough that the window itself is not what limits the panel.
    lumen.harness.set_size(eframe::egui::vec2(1280.0, 900.0));
    lumen.frames(10);
    let (browser, devices) = lumen.app().panel_limits();
    lumen.app_mut().set_browser_size(40.0);
    // The Devices tab needs a track to show.
    lumen.click("Add track in the mixer");
    lumen.frames(30);
    // The browser has a Devices tab too: the detail panel's comes last.
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(10);
    lumen.app_mut().set_detail_size(40.0);
    lumen.frames(10);
    assert!(
        lumen.app().browser_size() >= browser,
        "the browser keeps {browser} pt: {}",
        lumen.app().browser_size()
    );
    assert!(
        lumen.app().detail_size() >= devices,
        "the Devices tab keeps room for a whole card: {} < {devices}",
        lumen.app().detail_size()
    );
}

/// The detail panel's Devices tab, on a new MIDI track (it has the synth).
fn devices_of_a_new_midi_track(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    lumen.harness.set_size(eframe::egui::vec2(1280.0, 900.0));
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    lumen.frames(30);
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(10);
    lumen
}

#[test]
fn a_short_devices_tab_turns_the_cards_compact_and_a_tall_one_turns_them_back() {
    let mut lumen = devices_of_a_new_midi_track("compact-chain");
    lumen.app_mut().set_detail_size(400.0);
    lumen.frames(10);
    assert!(!lumen.app().chain_is_compact(), "room for a whole card");

    lumen.app_mut().set_detail_size(40.0);
    lumen.frames(10);
    assert!(lumen.app().chain_is_compact());
    let (_, least) = lumen.app().panel_limits();
    assert!(
        lumen.app().detail_size() >= least,
        "the panel keeps room for a compact card: {} < {least}",
        lumen.app().detail_size()
    );
    // A compact card is far shorter than a whole one: the panel can be
    // small.
    assert!(least < 240.0, "the least height is {least}");

    lumen.app_mut().set_detail_size(400.0);
    lumen.frames(10);
    assert!(!lumen.app().chain_is_compact(), "whole cards again");
}

#[test]
fn the_cpu_readout_keeps_its_width_so_the_audio_chip_never_moves() {
    let mut lumen = Lumen::start("cpu-slot");
    lumen.harness.set_size(eframe::egui::vec2(1600.0, 900.0));
    lumen.frames(5);
    let mut lefts = vec![];
    for (peak, text) in [
        (0.03, "CPU 3%"),
        (0.42, "CPU 42%"),
        (1.0, "CPU 100%"),
        (4.5, "CPU 450%"),
    ] {
        lumen.app_mut().show_cpu_load(peak);
        lumen.frames(3);
        assert!(lumen.shows(text), "{text} is shown");
        lefts.push(lumen.harness.get_by_label_contains("Audio: ").rect().left());
    }
    assert!(
        lefts.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01),
        "the audio chip stays put: {lefts:?}"
    );
}
