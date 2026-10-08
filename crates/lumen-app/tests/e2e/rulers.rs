//! The rulers (layout A): the time ruler's format, and dragging the bar
//! ruler to zoom and scroll without moving the playhead.

use eframe::egui::vec2;
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::Lumen;

const RULER: &str = "Ruler: click to move the playhead";

#[test]
fn the_time_format_changes_the_position_display() {
    let mut lumen = Lumen::start("time-format");
    assert!(lumen.shows("0:00.000"), "minutes and seconds by default");
    lumen.click("Time format: Minutes and seconds");
    lumen.click("Timecode, 25 fps");
    assert!(lumen.shows("Time format: Timecode, 25 fps"));
    assert!(lumen.shows("00:00:00:00"));
    lumen.click("Time format: Timecode, 25 fps");
    lumen.click("Samples");
    assert!(lumen.shows("Time format: Samples"));
}

#[test]
fn dragging_the_ruler_zooms_and_scrolls_but_does_not_locate() {
    let mut lumen = Lumen::start("ruler-drag");
    let before = lumen.app().view();
    let at = lumen.harness.get_by_label(RULER).rect().center();

    // Down zooms in, holding the grabbed beat under the pointer.
    lumen.harness.hover_at(at);
    lumen.harness.drag_at(at);
    lumen.frames(2);
    for step in 1..=10 {
        lumen.harness.hover_at(at + vec2(0.0, 10.0 * step as f32));
        lumen.frames(1);
    }
    lumen.harness.drop_at(at + vec2(0.0, 100.0));
    lumen.frames(3);
    let after = lumen.app().view();
    assert!(after.zoom > before.zoom * 2.0, "{before:?} -> {after:?}");
    assert_eq!(lumen.app().playhead(), Ticks::ZERO, "a drag is not a click");

    // Up zooms back out.
    lumen.harness.hover_at(at);
    lumen.harness.drag_at(at);
    lumen.frames(2);
    for step in 1..=10 {
        lumen.harness.hover_at(at - vec2(0.0, 10.0 * step as f32));
        lumen.frames(1);
    }
    lumen.harness.drop_at(at - vec2(0.0, 100.0));
    lumen.frames(3);
    assert!(lumen.app().view().zoom < after.zoom / 2.0);
}
