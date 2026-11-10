//! The audio editor's warp markers (M9.5 S5b, the owner's G B and P A): the
//! warp strip between the ruler and the wave, a ghost marker that is made and
//! moved in one drag, double-click to add and delete, Shift and drag to slide
//! the audio under a marker, the 25% to 400% limit, and the markers from the
//! keyboard. Each drag is one undo step.

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_core::warp::{MAX_RATIO, MIN_RATIO, ratio};
use lumen_project::model::{Clip, Id};

use super::Lumen;
use super::audio_editor_handles::{clip, shown, x_of};
use super::clip_commands::command;
use super::warp_chip::{close_chip, open_chip, opened_click_track};

/// A click track opened in the editor and warped at the tempo found.
fn warped_editor(name: &str) -> (Lumen, Id) {
    let (mut lumen, id) = opened_click_track(name);
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    close_chip(&mut lumen);
    lumen.frames(4);
    (lumen, id)
}

fn markers(lumen: &Lumen, id: Id) -> Vec<lumen_core::warp::Marker> {
    clip(lumen, id).warp.expect("warped").markers
}

/// The strip's middle line, at beat `q`.
fn on_strip(lumen: &Lumen, q: f64) -> Pos2 {
    let (rect, _) = lumen.app().audio_warp_strip().expect("the strip is drawn");
    Pos2::new(x_of(lumen, q), rect.center().y)
}

/// What a clip plays at each of a few ticks: a map's sound.
fn heard(c: &Clip) -> Vec<f64> {
    let w = c.warp.as_ref().expect("warped");
    [0, PPQ / 2, 2 * PPQ, 3 * PPQ + 5, 6 * PPQ]
        .into_iter()
        .map(|t| w.source_at(Ticks(t)))
        .collect()
}

fn same_sound(a: &Clip, b: &Clip) {
    for (x, y) in heard(a).into_iter().zip(heard(b)) {
        assert!((x - y).abs() < 1e-6, "{x} against {y}");
    }
}

#[test]
fn only_a_warped_clip_has_a_strip_and_it_sits_between_the_ruler_and_the_wave() {
    let (mut lumen, _) = opened_click_track("warp-strip-none");
    assert!(lumen.app().audio_warp_strip().is_none(), "not warped");
    let before = shown(&lumen).body;
    open_chip(&mut lumen);
    lumen.click("Warp clip");
    close_chip(&mut lumen);
    lumen.frames(4);
    let (strip, flags) = lumen.app().audio_warp_strip().expect("warped");
    let after = shown(&lumen).body;
    assert_eq!(strip.height(), 14.0);
    assert_eq!(strip.bottom(), after.top(), "the wave starts under it");
    assert!((after.top() - before.top() - 14.0).abs() < 0.01);
    assert_eq!(flags.len(), 1, "a warped clip starts with its one marker");
}

#[test]
fn double_click_adds_a_marker_where_the_audio_plays_and_double_click_on_it_deletes_it() {
    let (mut lumen, id) = warped_editor("warp-double");
    let before = clip(&lumen, id);
    lumen.double_click(on_strip(&lumen, 2.0));
    let after = clip(&lumen, id);
    assert_eq!(markers(&lumen, id).len(), 2);
    same_sound(&before, &after);
    // One undo step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(clip(&lumen, id), before);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.frames(3);
    assert_eq!(markers(&lumen, id).len(), 2);

    // Double-click on the flag takes it away again.
    let (_, flags) = lumen.app().audio_warp_strip().unwrap();
    let flag = flags[1];
    let y = lumen.app().audio_warp_strip().unwrap().0.center().y;
    lumen.double_click(Pos2::new(flag, y));
    assert_eq!(markers(&lumen, id).len(), 1);
    same_sound(&before, &clip(&lumen, id));
}

#[test]
fn dragging_the_ghost_makes_a_marker_and_moves_it_as_one_undo_step() {
    let (mut lumen, id) = warped_editor("warp-ghost");
    let before = clip(&lumen, id);
    let from = on_strip(&lumen, 2.0);
    lumen.drag(from, Pos2::new(from.x + 60.0, from.y), Modifiers::NONE);
    let made = markers(&lumen, id);
    assert_eq!(made.len(), 2, "the drag made one marker");
    // Moved: the audio that was at beat 2 now plays later.
    let was = before.warp.as_ref().unwrap().source_at(Ticks::quarters(2));
    assert!(made[1].at > Ticks::quarters(2) || made[1].source != was);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        clip(&lumen, id),
        before,
        "one undo takes back the making and the moving"
    );
}

#[test]
fn a_marker_stops_at_a_quarter_and_at_four_times_the_speed() {
    let (mut lumen, id) = warped_editor("warp-limit");
    lumen.double_click(on_strip(&lumen, 4.0));
    let (strip, flags) = lumen.app().audio_warp_strip().unwrap();
    // Dragged almost onto the first marker: the span before it would play far
    // faster than four times.
    let from = Pos2::new(flags[1], strip.center().y);
    let to = Pos2::new(x_of(&lumen, 0.05), strip.center().y);
    lumen.drag(from, to, Modifiers::ALT);
    let w = clip(&lumen, id).warp.unwrap();
    let r = ratio(w.markers[0], w.markers[1], w.original_bpm);
    assert!(
        (MIN_RATIO - 1e-6..=MAX_RATIO + 1e-6).contains(&r),
        "the span plays at {r}"
    );
    assert!(
        r > MAX_RATIO - 0.01,
        "and the marker went as far as it may: {r}"
    );
}

#[test]
fn shift_and_drag_slides_the_audio_under_a_marker_that_stays() {
    let (mut lumen, id) = warped_editor("warp-slide");
    lumen.double_click(on_strip(&lumen, 4.0));
    let made = markers(&lumen, id);
    let (strip, flags) = lumen.app().audio_warp_strip().unwrap();
    let from = Pos2::new(flags[1], strip.center().y);
    lumen.drag(from, Pos2::new(from.x + 30.0, from.y), Modifiers::SHIFT);
    let slid = markers(&lumen, id);
    assert_eq!(slid[1].at, made[1].at, "the marker did not move");
    assert!(slid[1].source < made[1].source, "the audio moved later");
}

#[test]
fn markers_are_tab_stops_with_names_and_move_and_go_from_the_keyboard() {
    let (mut lumen, id) = warped_editor("warp-keys");
    // The edit cursor, on the wave at beat 2: the marker goes there.
    let body = shown(&lumen).body;
    lumen.click_at(
        Pos2::new(x_of(&lumen, 2.0), body.center().y),
        Modifiers::NONE,
    );
    command(&mut lumen, "Add warp marker");
    lumen.frames(4);
    let added = markers(&lumen, id);
    assert_eq!(added.len(), 2, "the command added one at the cursor");
    // The new marker has the keyboard, with a name that says where it is.
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Warp marker at bar")
            .next()
            .is_some(),
        "an accessible name"
    );
    let at = added[1].at;
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    lumen.frames(2);
    assert!(markers(&lumen, id)[1].at > at, "Right moved it by the grid");
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    lumen.frames(2);
    assert_eq!(markers(&lumen, id)[1].at, at);
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(2);
    assert_eq!(markers(&lumen, id).len(), 1, "Delete took it away");
    // The only marker stays.
    assert!(lumen.app().audio_warp_strip().is_some());
}

#[test]
fn remove_all_warp_markers_is_one_step_and_keeps_the_clip_in_time() {
    let (mut lumen, id) = warped_editor("warp-clear");
    lumen.double_click(on_strip(&lumen, 2.0));
    lumen.double_click(on_strip(&lumen, 5.0));
    assert_eq!(markers(&lumen, id).len(), 3);
    let before = clip(&lumen, id);
    command(&mut lumen, "Remove all warp markers");
    assert_eq!(markers(&lumen, id).len(), 1);
    same_sound(&before, &clip(&lumen, id));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(markers(&lumen, id).len(), 3);
}

#[test]
fn a_drag_brought_back_to_where_it_began_leaves_the_clip_as_it_was() {
    let (mut lumen, id) = warped_editor("warp-return");
    lumen.double_click(on_strip(&lumen, 4.0));
    let made = clip(&lumen, id);
    let (strip, flags) = lumen.app().audio_warp_strip().unwrap();
    let from = Pos2::new(flags[1], strip.center().y);
    // Out and back to the same place without letting go.
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=6 {
        lumen
            .harness
            .hover_at(from + eframe::egui::vec2(10.0 * step as f32, 0.0));
        lumen.frames(1);
    }
    assert_ne!(clip(&lumen, id), made, "it moved");
    for step in (0..=6).rev() {
        lumen
            .harness
            .hover_at(from + eframe::egui::vec2(10.0 * step as f32, 0.0));
        lumen.frames(1);
    }
    lumen.harness.drop_at(from);
    lumen.frames(3);
    assert_eq!(clip(&lumen, id), made, "back where it began");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        markers(&lumen, id).len(),
        1,
        "one undo took the drag and the marker"
    );
}

#[test]
fn shift_and_the_arrows_slide_the_audio_under_the_marker() {
    let (mut lumen, id) = warped_editor("warp-slide-keys");
    let body = shown(&lumen).body;
    lumen.click_at(
        Pos2::new(x_of(&lumen, 2.0), body.center().y),
        Modifiers::NONE,
    );
    command(&mut lumen, "Add warp marker");
    lumen.frames(4);
    let added = markers(&lumen, id);
    lumen.key(Modifiers::SHIFT, Key::ArrowRight);
    lumen.frames(2);
    let slid = markers(&lumen, id);
    assert_eq!(slid[1].at, added[1].at, "the marker stays");
    assert!(slid[1].source < added[1].source, "the audio moved later");
    lumen.key(Modifiers::SHIFT, Key::ArrowLeft);
    lumen.frames(2);
    assert_eq!(markers(&lumen, id), added, "and back");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_ne!(markers(&lumen, id), added, "each press is a step");
}

#[test]
fn the_add_command_in_a_later_repeat_of_a_loop_adds_in_the_turn_that_plays() {
    let (mut lumen, id) = warped_editor("warp-loop-add");
    let c = clip(&lumen, id);
    // Loop the clip over three turns of its length.
    let turn = c.length;
    lumen
        .app_mut()
        .trim_clip_end_to(id, c.start + turn + turn + turn, true);
    lumen.frames(3);
    let looped = clip(&lumen, id);
    assert_eq!(looped.loop_length, Some(turn), "it loops");
    let at = looped.start + Ticks::quarters(2);
    lumen.app_mut().set_audio_cursor(at + turn);
    command(&mut lumen, "Add warp marker");
    lumen.frames(3);
    let later = markers(&lumen, id);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    lumen.app_mut().set_audio_cursor(at);
    command(&mut lumen, "Add warp marker");
    lumen.frames(3);
    assert_eq!(
        later,
        markers(&lumen, id),
        "the same marker as in the first turn"
    );
}
