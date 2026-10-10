//! The loop brace and punch: set by dragging on the strip (snapped), moved,
//! turned on and off, undone, and played by the engine.

use eframe::egui::{Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::Lumen;

const BRACE: &str = "Loop brace: drag to set the loop, click to turn it on or off";
const BAR: i64 = Ticks::quarters(4).0;

/// Drags with the left button from `from` to `to`, in small steps.
fn drag(lumen: &mut Lumen, from: Pos2, to: Pos2) {
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(to);
    lumen.frames(3);
}

/// The x of `quarters` on screen: the brace band starts where beat 0 is
/// (the view starts at the song's start).
fn x_of(lumen: &Lumen, band_left: f32, quarters: f64) -> f32 {
    band_left + (quarters * lumen.app().view().zoom) as f32
}

#[test]
fn a_loop_is_drawn_on_the_strip_moved_and_undone() {
    let mut lumen = Lumen::start("loop-draw");
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    let band = lumen.harness.get_by_label(BRACE).rect();
    let y = band.center().y;

    // A new loop from bar 6 to bar 8 (drawn right to left, a little off the lines).
    let from = Pos2::new(x_of(&lumen, band.left(), 28.3), y);
    let to = Pos2::new(x_of(&lumen, band.left(), 20.2), y);
    drag(&mut lumen, from, to);
    let t = lumen.app().project().transport;
    assert_eq!((t.loop_start.0, t.loop_end.0), (5 * BAR, 7 * BAR));
    assert!(t.loop_enabled, "a new loop plays");
    assert_eq!(
        lumen.app().engine_loop(),
        (Ticks(5 * BAR), Ticks(7 * BAR), true)
    );

    // Move it a bar to the right: one undo step for the whole drag.
    let inside = Pos2::new(x_of(&lumen, band.left(), 24.0), y);
    let bar_width = (4.0 * lumen.app().view().zoom) as f32;
    drag(&mut lumen, inside, inside + vec2(bar_width, 0.0));
    let t = lumen.app().project().transport;
    assert_eq!((t.loop_start.0, t.loop_end.0), (6 * BAR, 8 * BAR));
    lumen.key(Modifiers::COMMAND, eframe::egui::Key::Z);
    let t = lumen.app().project().transport;
    assert_eq!(
        (t.loop_start.0, t.loop_end.0),
        (5 * BAR, 7 * BAR),
        "undo the move"
    );
    lumen.key(Modifiers::COMMAND, eframe::egui::Key::Z);
    assert_eq!(
        lumen.app().project().transport.loop_end,
        Ticks(4 * BAR),
        "undo the new loop"
    );
    assert!(!lumen.app().engine_loop().2, "the engine follows undo");
}

#[test]
fn the_brace_and_the_button_turn_looping_on_and_off() {
    let mut lumen = Lumen::start("loop-toggle");
    assert!(!lumen.is_on("Loop"));
    lumen.click("Loop");
    assert!(lumen.is_on("Loop"));
    assert!(lumen.app().engine_loop().2);

    // A click on the brace (the default loop, bars 1 to 4) turns it off.
    let band = lumen.harness.get_by_label(BRACE).rect();
    let inside = Pos2::new(x_of(&lumen, band.left(), 2.0), band.center().y);
    lumen.harness.hover_at(inside);
    lumen.harness.drag_at(inside);
    lumen.frames(1);
    lumen.harness.drop_at(inside);
    lumen.frames(3);
    assert!(!lumen.is_on("Loop"));
    assert!(!lumen.app().project().transport.loop_enabled);
}

#[test]
fn punch_in_and_out_are_set_from_the_loop_menu() {
    let mut lumen = Lumen::start("punch");
    lumen
        .harness
        .get_by_label("Loop")
        .click_button(PointerButton::Secondary);
    lumen.frames(3);
    lumen.click("Punch in at the loop start");
    assert!(lumen.app().project().transport.punch_in);
    lumen
        .harness
        .get_by_label("Loop")
        .click_button(PointerButton::Secondary);
    lumen.frames(3);
    lumen.click("Punch out at the loop end");
    let t = lumen.app().project().transport;
    assert!(t.punch_in && t.punch_out);
    lumen.key(Modifiers::COMMAND, eframe::egui::Key::Z);
    assert!(!lumen.app().project().transport.punch_out, "undoable");
}

/// MT-APP-13 run 1 (owner): nothing said whether a drag would move or
/// resize the loop, and the edges were hard to catch.
#[test]
fn hovering_the_brace_says_what_a_drag_will_do() {
    let mut lumen = Lumen::start("loop-hints");
    let band = lumen.harness.get_by_label(BRACE).rect();
    let y = band.center().y;
    let hover = |lumen: &mut Lumen, x: f32| {
        lumen.harness.hover_at(Pos2::new(x, y));
        lumen.frames(2);
        lumen.info().split(':').next().map(str::to_owned)
    };
    // The default loop is bars 1 to 4 (quarters 0 to 16).
    let start = x_of(&lumen, band.left(), 0.0);
    let end = x_of(&lumen, band.left(), 16.0);
    let (middle, after) = (
        x_of(&lumen, band.left(), 8.0),
        x_of(&lumen, band.left(), 20.0),
    );
    assert_eq!(
        hover(&mut lumen, start + 7.0).as_deref(),
        Some("Loop start")
    );
    assert_eq!(hover(&mut lumen, end - 7.0).as_deref(), Some("Loop end"));
    assert_eq!(hover(&mut lumen, middle).as_deref(), Some("Loop"));
    assert_eq!(hover(&mut lumen, after).as_deref(), Some("Loop band"));
    assert!(lumen.info().contains("drag to draw a loop"));

    // 7 points inside the start edge still resizes.
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    let from = Pos2::new(start + 7.0, y);
    let to = Pos2::new(x_of(&lumen, band.left(), 4.2), y);
    drag(&mut lumen, from, to);
    let t = lumen.app().project().transport;
    assert_eq!((t.loop_start.0, t.loop_end.0), (BAR, 4 * BAR));
}
