//! The scrub strip: a click jumps (snapped); a drag plays the grid step
//! under the pointer and leaves the playhead there; Alt scrubs like tape,
//! unsnapped; while playing, a drag jumps playback.

use eframe::egui::{Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::{Lumen, keys};

const STRIP: &str = "Scrub strip: click to jump, drag to listen";
const BAR: i64 = Ticks::quarters(4).0;

/// The point on the strip at `quarters` (the view starts at the song's start).
fn at(lumen: &Lumen, quarters: f64) -> Pos2 {
    let strip = lumen.harness.get_by_label(STRIP).rect();
    Pos2::new(
        strip.left() + (quarters * lumen.app().view().zoom) as f32,
        strip.center().y,
    )
}

/// Drags on the strip from `from` to `to` (quarters), holding `modifiers`,
/// and checks `during` before letting go.
fn scrub(lumen: &mut Lumen, from: f64, to: f64, modifiers: Modifiers, during: impl Fn(&Lumen)) {
    let (a, b) = (at(lumen, from), at(lumen, to));
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(modifiers));
    lumen.harness.hover_at(a);
    lumen.harness.drag_at(a);
    lumen.frames(2);
    for step in 1..=8 {
        lumen.harness.hover_at(a + (b - a) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.frames(2);
    during(lumen);
    lumen.harness.drop_at(b);
    lumen.frames(3);
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(1);
}

#[test]
fn a_click_on_the_strip_jumps_there_snapped() {
    let mut lumen = Lumen::start("scrub-click");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    let p = at(&lumen, 8.4);
    lumen.harness.hover_at(p);
    lumen.harness.drag_at(p);
    lumen.frames(1);
    lumen.harness.drop_at(p);
    lumen.wait_for("the jump", |app| app.playhead() == Ticks(2 * BAR));
}

#[test]
fn dragging_plays_the_step_under_the_pointer_and_leaves_the_playhead_there() {
    let mut lumen = Lumen::start("scrub-loop");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    // Into bar 4 (quarters 12 to 16): the step that plays starts at its
    // start, not the nearest line.
    scrub(&mut lumen, 4.0, 15.2, Modifiers::NONE, |lumen| {
        assert!(!lumen.app().is_playing(), "scrubbing is not playing");
    });
    lumen.wait_for("the playhead at the step", |app| {
        app.playhead() == Ticks(3 * BAR)
    });
    assert!(!lumen.app().is_playing());
}

#[test]
fn alt_scrubs_like_tape_without_snapping() {
    let mut lumen = Lumen::start("scrub-tape");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    scrub(&mut lumen, 4.0, 15.2, Modifiers::ALT, |_| {});
    // Wherever the hand let go, not on a bar line.
    lumen.wait_for("the playhead under the pointer", |app| {
        let t = app.playhead().0;
        t > 3 * BAR && t < 4 * BAR
    });
}

#[test]
fn while_playing_a_drag_jumps_playback() {
    let mut lumen = Lumen::start("scrub-playing");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    lumen.key(Modifiers::NONE, eframe::egui::Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    scrub(&mut lumen, 4.0, 16.3, Modifiers::NONE, |_| {});
    lumen.wait_for("playback from bar 5", |app| {
        let t = app.playhead().0;
        app.is_playing() && (4 * BAR..5 * BAR).contains(&t)
    });
}

/// MT-APP-13 run 1 (owner): the scrub strip gave no hint of what a drag
/// would do.
#[test]
fn the_strip_hints_what_a_drag_will_do() {
    let mut lumen = Lumen::start("scrub-hints");
    let p = at(&lumen, 5.3);
    lumen.harness.hover_at(p);
    lumen.frames(2);
    assert!(
        lumen.info().contains("drag to listen to each step"),
        "{}",
        lumen.info()
    );
    assert_eq!(lumen.lit(), None, "nothing is lit until a key is held");
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::ALT));
    lumen.frames(2);
    assert_eq!(lumen.lit(), Some(keys("Alt+drag to scrub like tape")));
    scrub(&mut lumen, 5.3, 9.0, Modifiers::ALT, |lumen| {
        let hint = lumen.app().hint().unwrap_or_default();
        assert!(hint.starts_with("tape ") && hint.ends_with('×'), "{hint}");
    });
}
