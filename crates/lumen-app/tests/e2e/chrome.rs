//! M4.5 slice 3, the interface audit's chrome: the panel toggles keep a
//! button's look and say whether they are on, the empty mixer's ghost strip
//! adds a track, and the time signature is one field that takes "7/8".

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;

use super::Lumen;

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
    let meter = lumen.app().project().time_signature;
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
    let meter = lumen.app().project().time_signature;
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
