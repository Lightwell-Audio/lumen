//! M4.5 slice 5, motion: what an edit moves glides there, and what undo
//! changes glides back and is washed in the accent for a moment.

use eframe::egui::{Key, Modifiers, pos2};
use egui_kittest::kittest::Queryable;

use super::properties::{rect, with_two_clips};

#[test]
fn undo_glides_a_clip_back_and_washes_it() {
    let (mut lumen, kick, _) = with_two_clips("glide-undo");
    let ctx = lumen.harness.ctx.clone();
    let before = lumen.app().project().tracks[0].clips[0].start;
    // Drag the kick's header two bars on.
    let a = rect(&lumen, kick);
    let from = pos2(a.center().x, a.top() + 6.0);
    lumen.drag(from, from + eframe::egui::vec2(200.0, 0.0), Modifiers::NONE);
    lumen.frames(40);
    let moved = lumen.app().project().tracks[0].clips[0].start;
    assert!(moved > before, "the drag moved it");

    // Undo: the clip is back at once, drawn on its way back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips[0].start, before);
    let drawn = lumen.app().drawn_clip_start(&ctx, kick).expect("drawn");
    assert!(drawn > before && drawn <= moved, "on its way: {drawn:?}");
    assert!(lumen.app().clip_wash(&ctx, kick) > 0.0, "washed");
    lumen.frames(60);
    assert_eq!(
        lumen.app().drawn_clip_start(&ctx, kick),
        Some(before),
        "landed"
    );
    assert_eq!(lumen.app().clip_wash(&ctx, kick), 0.0, "the wash faded");
}

#[test]
fn a_new_track_grows_in_and_a_folded_group_glides_shut() {
    let mut lumen = super::Lumen::start("rows-glide");
    let ctx = lumen.harness.ctx.clone();
    // The content's height past the tracks (the pinned area's room).
    let rest = lumen.app().tracks_height(&ctx);
    let tracks = |lumen: &super::Lumen| lumen.app().tracks_height(&ctx) - rest;
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.frames(30);
    let one = tracks(&lumen);
    // A second track: it starts from no height and grows to its own.
    lumen.key(Modifiers::COMMAND, Key::T);
    let growing = tracks(&lumen);
    assert!(
        growing > one && growing < 2.0 * one - 1.0,
        "growing: {growing} of {one}"
    );
    lumen.frames(30);
    let grown = tracks(&lumen);
    assert!((grown - 2.0 * one).abs() < 0.5, "grown: {grown} of {one}");

    // Grouped and folded: the group's track glides shut, then goes.
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.key(Modifiers::COMMAND, Key::G);
    lumen.frames(30);
    lumen.click("Fold group Group 1");
    assert!(lumen.shows("Mute Audio 1"), "still closing");
    lumen.frames(30);
    assert!(!lumen.shows("Mute Audio 1"), "shut");
}

#[test]
fn the_tracks_scroll_bar_shows_while_scrolling_then_leaves() {
    use eframe::egui::{Event, MouseWheelUnit, TouchPhase, vec2};
    let mut lumen = super::Lumen::start("scroll-bar");
    for _ in 0..16 {
        lumen.key(Modifiers::COMMAND, Key::T);
    }
    lumen.frames(40);
    assert!(!lumen.shows("Scroll bar"), "quiet at rest");
    // The wheel over the lanes, away from the right edge.
    let lanes = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    lumen
        .harness
        .hover_at(pos2(lanes.center().x, lanes.bottom() + 120.0));
    lumen.frames(2);
    lumen.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    lumen.frames(10);
    assert!(lumen.shows("Scroll bar"), "shows while scrolling");
    // A moment after, it fades away.
    // (The harness's clock moves a sixtieth of a second a frame.)
    lumen.frames(90);
    assert!(!lumen.shows("Scroll bar"), "gone again");
}

#[test]
fn a_closed_window_fades_out() {
    let mut lumen = super::Lumen::start("window-fade");
    lumen.click("Lumen");
    lumen.click("History\u{2026}");
    lumen.frames(20);
    lumen.key(Modifiers::NONE, Key::Escape);
    // Closed at once (nothing in it can be used), drawn fading for a moment:
    // each of those frames asks for the next.
    let fading = (0..4).any(|_| {
        lumen.harness.step();
        lumen
            .harness
            .ctx
            .repaint_causes()
            .iter()
            .any(|c| c.file.ends_with("fade_out.rs"))
    });
    assert!(fading, "the window fades");
    lumen.frames(30);
    assert!(
        !lumen
            .harness
            .ctx
            .repaint_causes()
            .iter()
            .any(|c| c.file.ends_with("fade_out.rs")),
        "then rests"
    );
}
