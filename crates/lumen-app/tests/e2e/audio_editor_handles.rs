//! The audio editor's handles (M9.5 S2a): trims, fades, the gain line and
//! slip in the Clip tab, each one undo step that equals the same edit on
//! the arrangement; Alt leaves the grid; locked clips and frozen tracks
//! refuse; a looping clip's end grip sits at its real end; a fade out
//! dragged past the clip crossfades; the handles from the keyboard (Tab,
//! Alt and an arrow, Delete, Enter's popover); the D27 origin after a split
//! at a later repeat; the header's layouts at the smallest widths; and a
//! trim dragged past the view scrolls it.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::fade::FadeShape;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::{Clip, ClipContent, Id};

use super::{Lumen, midi_play, wav};

/// Lumen with a two-second file (four beats at 120 BPM) dropped in, its
/// clip trimmed to beats 1 to 3 and opened in the Clip tab.
fn opened(name: &str) -> (Lumen, Id) {
    opened_with(Lumen::start(name), 2.0, (1, 3))
}

fn opened_with(mut lumen: Lumen, seconds: f32, (from, to): (i64, i64)) -> (Lumen, Id) {
    let file = wav(&lumen.dir().join("kick.wav"), seconds);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    if from > 0 {
        lumen
            .app_mut()
            .trim_clip_start_to(clip, Ticks::quarters(from));
    }
    lumen
        .app_mut()
        .trim_clip_end_to(clip, Ticks::quarters(to), false);
    open(&mut lumen, clip);
    (lumen, clip)
}

/// Opens `clip` in the Clip tab and waits for its handles.
fn open(lumen: &mut Lumen, clip: Id) {
    // A click on the clip takes the keyboard from any handle; Enter on one
    // would open its popover instead.
    lumen.harness.ctx.memory_mut(|m| {
        if let Some(focused) = m.focused() {
            m.surrender_focus(focused);
        }
    });
    lumen.app_mut().select_clip(clip);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the audio editor's handles", |app| {
        app.audio_editor_shown()
            .is_some_and(|s| s.handles.is_some())
    });
    lumen.frames(3);
}

fn shown(lumen: &Lumen) -> lumen_app::testing::AudioEditorShown {
    lumen
        .app()
        .audio_editor_shown()
        .expect("the editor is drawn")
}

fn handles(lumen: &Lumen) -> lumen_app::testing::AudioHandlesShown {
    shown(lumen).handles.expect("the clip shows its handles")
}

fn clip(lumen: &Lumen, id: Id) -> Clip {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .map(|c| (**c).clone())
        .expect("the clip exists")
}

fn gain(lumen: &Lumen, id: Id) -> f32 {
    match clip(lumen, id).content {
        ClipContent::Audio { gain_db, .. } => gain_db,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    }
}

/// The x of beat `q` in the editor.
fn x_of(lumen: &Lumen, q: f64) -> f32 {
    let s = shown(lumen);
    s.view.x(s.body.left(), q)
}

/// The edit just made, undone and made again on the arrangement's own
/// path: the two clips are the same.
fn equals_the_arrangement_trim(lumen: &mut Lumen, id: Id, edited: &Clip) {
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(2);
    lumen
        .app_mut()
        .trim_clip_end_to(id, edited.start + edited.length, false);
    lumen.frames(2);
    assert_eq!(
        &clip(lumen, id),
        edited,
        "the arrangement's trim gives the same clip"
    );
}

#[test]
fn dragging_the_end_grip_trims_the_clip_in_one_step_as_the_arrangement_does() {
    let (mut lumen, id) = opened("editor-trim-end");
    let before = clip(&lumen, id);
    let h = handles(&lumen);
    let mid = shown(&lumen).body.center().y;
    let grip = Pos2::new(h.end - 2.0, mid);
    lumen.harness.hover_at(grip);
    lumen.frames(2);
    assert!(lumen.info().starts_with("Clip end"), "{}", lumen.info());

    // Half a beat in: the end lands on the grid.
    let to = Pos2::new(x_of(&lumen, 2.5) + 3.0, mid);
    lumen.drag(grip, to, Modifiers::NONE);
    let after = clip(&lumen, id);
    assert_eq!(after.start, before.start, "the start stays");
    assert_eq!(
        after.start + after.length,
        Ticks::quarters(2) + Ticks(PPQ / 2)
    );
    assert_eq!(lumen.app().undo_label(), Some("Trim clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before, "one undo step takes it back");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    equals_the_arrangement_trim(&mut lumen, id, &after);
}

#[test]
fn alt_and_a_drag_trims_off_the_grid() {
    let (mut lumen, id) = opened("editor-trim-alt");
    let mid = shown(&lumen).body.center().y;
    let grip = Pos2::new(handles(&lumen).end - 2.0, mid);
    let to = Pos2::new(x_of(&lumen, 2.37), mid);
    lumen.drag(grip, to, Modifiers::ALT);
    let end = clip(&lumen, id).start + clip(&lumen, id).length;
    let wanted = 2.37 * PPQ as f64;
    assert!(
        (end.0 as f64 - wanted).abs() < PPQ as f64 * 0.03,
        "the end follows the hand: {end:?}"
    );
    assert_ne!(end.0 % (PPQ / 8), 0, "and is not on the grid");
}

#[test]
fn a_fade_handle_and_the_gain_line_edit_the_clip_and_double_click_removes_the_fade() {
    let (mut lumen, id) = opened("editor-fade-gain");
    let h = handles(&lumen);
    lumen.harness.hover_at(h.fade_in);
    lumen.frames(2);
    assert!(
        lumen.info().starts_with("Fade in: drag to set its length"),
        "{}",
        lumen.info()
    );
    let to = Pos2::new(x_of(&lumen, 1.5), h.fade_in.y);
    lumen.drag(h.fade_in, to, Modifiers::NONE);
    let faded = clip(&lumen, id);
    assert_eq!(
        faded.fade_in.length,
        Ticks(PPQ / 2),
        "half a beat, on the grid"
    );
    assert_eq!(lumen.app().undo_label(), Some("Fade in"));

    // The gain line, down: one step.
    let h = handles(&lumen);
    let on_line = Pos2::new(x_of(&lumen, 2.0), h.gain_y);
    lumen.drag(on_line, on_line + vec2(0.0, 40.0), Modifiers::NONE);
    assert!(
        gain(&lumen, id) < -1.0,
        "the gain went down: {}",
        gain(&lumen, id)
    );
    assert_eq!(lumen.app().undo_label(), Some("Clip gain"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(gain(&lumen, id), 0.0);

    // FD A: a double-click on the handle removes the fade, as on the
    // arrangement.
    let at = handles(&lumen).fade_in;
    lumen.double_click(at);
    assert_eq!(clip(&lumen, id).fade_in.length, Ticks::ZERO);
}

#[test]
fn the_fade_menu_lists_the_shapes() {
    let (mut lumen, id) = opened("editor-fade-shapes");
    let h = handles(&lumen);
    let to = Pos2::new(x_of(&lumen, 1.5), h.fade_in.y);
    lumen.drag(h.fade_in, to, Modifiers::NONE);
    let before = clip(&lumen, id).fade_in.curve;
    lumen.right_click(handles(&lumen).fade_in);
    assert!(lumen.shows("Fade in: S-curve"), "the shapes are listed");
    lumen.click("Fade in: S-curve");
    assert_ne!(clip(&lumen, id).fade_in.curve, before);
    assert_eq!(lumen.app().undo_label(), Some("Fade curve"));
}

#[test]
fn the_dimmed_file_and_ctrl_alt_slip_the_audio_and_the_clip_stays() {
    let (mut lumen, id) = opened("editor-slip");
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    let before = clip(&lumen, id);
    let mid = shown(&lumen).body.center().y;
    let dimmed = Pos2::new(x_of(&lumen, 0.5), mid);
    lumen.harness.hover_at(dimmed);
    lumen.frames(2);
    assert!(lumen.info().contains("slide the audio"), "{}", lumen.info());
    lumen.drag(
        dimmed,
        dimmed + vec2(-(x_of(&lumen, 1.0) - x_of(&lumen, 0.5)), 0.0),
        Modifiers::NONE,
    );
    let slipped = clip(&lumen, id);
    assert_eq!(
        (slipped.start, slipped.length),
        (before.start, before.length)
    );
    assert!(
        source_at(&slipped) > source_at(&before),
        "the file slid earlier under the clip"
    );
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);

    // The keys: Ctrl+Alt+Right, one grid step later, where the file allows.
    lumen.harness.hover_at(shown(&lumen).body.center());
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowRight);
    let later = clip(&lumen, id);
    assert!(
        source_at(&later) < source_at(&before),
        "{:?} {:?}",
        source_at(&later),
        source_at(&before)
    );
    assert_eq!(later.start, before.start);
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
}

#[test]
fn a_locked_clip_and_a_frozen_track_take_no_edit_and_say_why() {
    let (mut lumen, id) = opened("editor-locked");
    // D19: the wave's menu is the clip menu.
    lumen.right_click(Pos2::new(x_of(&lumen, 2.0), shown(&lumen).body.center().y));
    lumen.click("Lock");
    assert!(clip(&lumen, id).locked);
    let locked = clip(&lumen, id);
    let mid = shown(&lumen).body.center().y;
    let grip = Pos2::new(handles(&lumen).end - 2.0, mid);
    lumen.drag(grip, grip + vec2(-80.0, 0.0), Modifiers::NONE);
    assert_eq!(clip(&lumen, id), locked, "nothing moved");
    let note = lumen.app().note().unwrap_or_default().to_owned();
    assert!(note.contains("locked"), "{note}");
    lumen.harness.hover_at(grip);
    lumen.frames(2);
    assert!(lumen.info().contains("can't change"), "{}", lumen.info());

    // Unlocked, then frozen: the frozen message.
    lumen.key(Modifiers::COMMAND, Key::Z);
    let track = lumen.app().project().tracks[0].id;
    let name = lumen.app().project().tracks[0].name.clone();
    lumen.app_mut().select_tracks(&[track]);
    midi_play::palette(&mut lumen, "Freeze track");
    lumen.wait_for("the freeze", |app| app.undo_label() == Some("Freeze"));
    open(&mut lumen, id);
    let frozen = clip(&lumen, id);
    let at = handles(&lumen).fade_in;
    lumen.drag(at, at + vec2(60.0, 0.0), Modifiers::NONE);
    assert_eq!(clip(&lumen, id), frozen);
    let note = lumen.app().note().unwrap_or_default().to_owned();
    assert!(note.contains(&format!("{name} is frozen")), "{note}");
    assert_eq!(lumen.app().undo_label(), Some("Freeze"), "no undo step");
}

#[test]
fn a_looping_clips_end_grip_sits_at_its_real_end() {
    // One turn is beats 1 to 3; the clip loops on to beat 7.
    let (mut lumen, id) = opened("editor-loop-end");
    lumen
        .app_mut()
        .trim_clip_end_to(id, Ticks::quarters(7), true);
    lumen.frames(3);
    assert!(clip(&lumen, id).loop_length.is_some(), "it loops");
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    let s = shown(&lumen);
    let h = s.handles.expect("handles");
    assert!(
        (h.end - s.view.x(s.body.left(), 7.0)).abs() < 1.0,
        "at beat 7"
    );
    let (first, _) = s.played;
    assert!(
        first.1 <= 3.0 + 1e-6,
        "the bright part is one turn: {first:?}"
    );
}

#[test]
fn a_fade_out_dragged_past_the_end_crossfades_into_the_next_clip() {
    let (mut lumen, left) = opened_with(Lumen::start("editor-crossfade"), 4.0, (0, 8));
    // Split at beat 4: two halves side by side, the left one opened.
    lumen.app_mut().select_clip(left);
    lumen.frames(2);
    let ruler = lumen
        .harness
        .get_by_label("Editor ruler: click to move the playhead")
        .rect();
    let at = Pos2::new(x_of(&lumen, 4.0), ruler.center().y);
    super::audio_editor::click_at(&mut lumen, at);
    lumen.wait_for("the playhead at beat 4", |app| {
        (app.playhead().0 - Ticks::quarters(4).0).abs() < PPQ / 8
    });
    lumen.key(Modifiers::COMMAND, Key::E);
    let right = lumen.app().project().tracks[0].clips[1].id;
    assert_eq!(clip(&lumen, right).start, Ticks::quarters(4));
    open(&mut lumen, left);
    let h = handles(&lumen);
    let past = Pos2::new(h.end + 60.0, h.fade_out.y);
    lumen.drag(h.fade_out, past, Modifiers::NONE);
    assert!(
        clip(&lumen, right).crossfade,
        "a crossfade into the right clip"
    );
    assert_eq!(lumen.app().undo_label(), Some("Crossfade"));
}

#[test]
fn the_handles_stay_quick_at_the_deepest_zoom() {
    let lumen = Lumen::start("editor-deep");
    let (mut lumen, _) = opened_with(lumen, 60.0, (1, 100));
    let p = shown(&lumen).body.center();
    lumen.harness.hover_at(p);
    lumen.frames(2);
    let started = std::time::Instant::now();
    for _ in 0..80 {
        lumen.key(Modifiers::NONE, Key::Plus);
    }
    lumen.harness.hover_at(p + vec2(1.0, 0.0));
    lumen.frames(3);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "zooming in took {:?}",
        started.elapsed()
    );
    assert!(shown(&lumen).view.zoom > 10_000.0, "deep in");
    assert!(lumen.app().info_line().is_some(), "hovering the wave");
}

/// Presses F6 until the detail panel has the keyboard, wherever it is now.
fn to_the_detail_panel(lumen: &mut Lumen) {
    let ctx = lumen.harness.ctx.clone();
    for _ in 0..4 {
        if lumen.app().focused_pane(&ctx).as_deref() == Some("Detail") {
            return;
        }
        lumen.key(Modifiers::NONE, Key::F6);
    }
    panic!("F6 never reached the detail panel");
}

/// Presses Tab until the handle `name` has the keyboard.
fn tab_to(lumen: &mut Lumen, name: &str) {
    let ctx = lumen.harness.ctx.clone();
    lumen.harness.hover_at(shown(lumen).body.center());
    lumen.frames(1);
    for _ in 0..90 {
        if lumen.app().audio_handle_focused(&ctx) == Some(name) {
            return;
        }
        lumen.key(Modifiers::NONE, Key::Tab);
    }
    panic!("Tab never reached the {name} handle");
}

#[test]
fn the_handles_are_tab_stops_that_take_their_own_keys() {
    let (mut lumen, id) = opened("editor-handle-keys");
    // F6 to the detail panel, then Tab along its stops.
    to_the_detail_panel(&mut lumen);
    tab_to(&mut lumen, "trim start");
    assert!(
        lumen.shows("Trim start at bar 1 beat 2"),
        "a name with its value"
    );
    tab_to(&mut lumen, "fade in");
    let before = clip(&lumen, id);

    // Alt and an arrow moves the fade, not the clip.
    lumen.key(Modifiers::ALT, Key::ArrowRight);
    let after = clip(&lumen, id);
    assert!(after.fade_in.length > Ticks::ZERO, "the fade grew");
    assert_eq!((after.start, after.length), (before.start, before.length));
    assert_eq!(lumen.app().undo_label(), Some("Fade in"));
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert!(
        clip(&lumen, id).fade_in.length > after.fade_in.length,
        "by the grid"
    );

    // Delete takes the fade and leaves the clip.
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(clip(&lumen, id).fade_in.length, Ticks::ZERO);
    assert_eq!(
        lumen.app().project().tracks[0].clips.len(),
        1,
        "the clip stays"
    );

    // Enter types a length; the handle has the keyboard again after.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert!(
        lumen
            .harness
            .query_all_by_label("Fade in length")
            .next()
            .is_some(),
        "the popover"
    );
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("125 ms".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(lumen.app().audio_handle_focused(&ctx), Some("fade in"));
    // 125 ms at 120 BPM is a quarter of a beat.
    let length = clip(&lumen, id).fade_in.length;
    assert!((length.0 - PPQ / 4).abs() <= 1, "{length:?}");
    assert!(lumen.shows("Fade in 125 ms"), "the name says the new value");

    // The gain from the keyboard: Up a dB, Delete back to 0 dB.
    tab_to(&mut lumen, "gain");
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert!((gain(&lumen, id) - 1.0).abs() < 1e-4);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(gain(&lumen, id), 0.0);
    assert!(!lumen.app().project().tracks[0].clips.is_empty());
}

#[test]
fn a_looped_clip_split_at_a_later_repeat_draws_the_frame_it_plays() {
    // One turn is beats 0 to 2; the clip loops to beat 8; split at beat 5.
    let (mut lumen, id) = opened_with(Lumen::start("editor-split-origin"), 2.0, (0, 2));
    lumen
        .app_mut()
        .trim_clip_end_to(id, Ticks::quarters(8), true);
    lumen.frames(3);
    // What plays at beat 5 before the split: one beat into a turn, the
    // engine's `(position + phase) mod period`, half a second at 48 kHz.
    // The editor draws the file once from the loop's origin, so that
    // frame is drawn at beat 1.
    let before = lumen
        .app()
        .audio_editor_frame_at(1.0)
        .expect("drawn at beat 1");
    assert!((before - 24_000.0).abs() < 1.0, "{before}");
    lumen.app_mut().locate_to(Ticks::quarters(5));
    lumen.wait_for("the playhead at beat 5", |app| {
        (app.playhead().0 - Ticks::quarters(5).0).abs() < PPQ / 8
    });
    lumen.app_mut().select_clip(id);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::E);
    let later = lumen.app().project().tracks[0].clips[1].clone();
    assert_eq!(later.start, Ticks::quarters(5));
    open(&mut lumen, later.id);
    // The turn holding beat 5 starts at beat 4: that is what is bright, and
    // not a turn before the clip's start.
    let (first, second) = shown(&lumen).played;
    assert!(
        (first.0 - 4.0).abs() < 1e-6 && (first.1 - 6.0).abs() < 1e-6,
        "{first:?}"
    );
    assert!(second.is_none());
    // The frame drawn at the later clip's start is the one played there.
    let after = lumen
        .app()
        .audio_editor_frame_at(5.0)
        .expect("drawn at beat 5");
    assert!(
        (after - before).abs() < 1.0,
        "{after} after the split, {before} before"
    );
}

/// The header's controls by name: none overlaps another, and each is inside
/// the strip.
fn header_controls(lumen: &Lumen) -> Vec<(String, Rect)> {
    let body = shown(lumen).body;
    let named = ["Clip kick", "Zoom to clip", "Whole file", "More"]
        .into_iter()
        .flat_map(|name| {
            lumen
                .harness
                .query_all_by_label(name)
                .map(move |n| (name.to_owned(), n.rect()))
        });
    let grid = lumen
        .harness
        .get_all_by(|n| n.label().is_some_and(|l| l.starts_with("Editor grid:")))
        .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()));
    named
        .chain(grid)
        .filter(|(_, r)| r.bottom() < body.top() && r.left() >= body.left() - 400.0)
        .collect()
}

#[test]
fn the_header_fits_the_smallest_windows_with_every_action_reachable() {
    let (mut lumen, _) = opened("editor-header-fit");
    // The smallest window at 100%, 150% and 200%, in points.
    for (width, height) in [(960.0, 540.0), (640.0, 540.0), (480.0, 540.0)] {
        lumen.harness.set_size(vec2(width, height));
        lumen.app_mut().set_browser_size(220.0);
        lumen.frames(20);
        let s = shown(&lumen);
        let controls = header_controls(&lumen);
        for (i, (a, ra)) in controls.iter().enumerate() {
            assert!(
                ra.right() <= s.body.right() + 0.5,
                "{a} is past the editor at {width}: {controls:?}, body {:?}",
                s.body
            );
            for (b, rb) in &controls[i + 1..] {
                assert!(
                    !ra.shrink(0.5).intersects(rb.shrink(0.5)),
                    "{a} and {b} overlap at {width}: {ra:?} {rb:?}"
                );
            }
        }
        let names: Vec<_> = controls.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"Clip kick"),
            "the title at {width}: {names:?}"
        );
        let views = names.contains(&"Zoom to clip") && names.contains(&"Whole file");
        assert!(
            views || names.contains(&"More"),
            "the views are in the strip or its menu at {width} ({}): {names:?}",
            s.layout
        );
        if s.folded {
            assert!(names.contains(&"More"), "Clip properties is in the menu");
        }
        // Every part is also a command, so the palette finds it.
        midi_play::palette(&mut lumen, "Zoom the audio editor to the clip");
        lumen.frames(2);
    }
    // At the narrowest, the menu holds the view buttons.
    let names: Vec<_> = header_controls(&lumen)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    if names.iter().any(|n| n == "More") && !names.iter().any(|n| n == "Whole file") {
        lumen.click("More");
        assert!(lumen.shows("Whole file"), "the menu lists the views");
        lumen.key(Modifiers::NONE, Key::Escape);
    }
}

#[test]
fn a_trim_dragged_past_the_view_scrolls_it_and_is_one_step() {
    // A ten-second file with its clip on beats 1 to 3, framed: dragging the
    // end grip to the right edge scrolls the view on.
    let (mut lumen, id) = opened_with(Lumen::start("editor-edge-scroll"), 10.0, (1, 3));
    lumen.app_mut().resume_follow();
    lumen.frames(1);
    assert!(!lumen.app().follow_paused());
    let s = shown(&lumen);
    let mid = s.body.center().y;
    let grip = Pos2::new(handles(&lumen).end - 2.0, mid);
    let edge = Pos2::new(s.body.right() - 2.0, mid);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.harness.hover_at(grip);
    lumen.harness.drag_at(grip);
    lumen.frames(2);
    // The grip sits near the side, framed: pressed and held still, the
    // view stays put until the hand goes on outwards.
    lumen.harness.hover_at(grip + vec2(-2.0, 0.0));
    lumen.frames(30);
    assert_eq!(shown(&lumen).view, s.view, "a still press does not scroll");
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(grip + (edge - grip) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    // Held at the edge: the view moves on.
    lumen.frames(30);
    let moved = shown(&lumen);
    assert!(moved.view.left > s.view.left, "the view scrolled");
    lumen.harness.drop_at(edge);
    lumen.frames(3);
    assert!(lumen.app().follow_paused(), "the scroll paused Follow");
    let c = clip(&lumen, id);
    let end_q = (c.start + c.length).as_quarters();
    assert!(end_q > 3.5, "the end went past where the view was: {end_q}");
    assert_eq!(lumen.app().undo_label(), Some("Trim clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id).length, Ticks::quarters(2), "one step");
}

/// Where in its file, in seconds, the clip starts playing (48 kHz test files).
fn source_at(c: &Clip) -> f64 {
    match c.content {
        ClipContent::Audio { offset, .. } => offset as f64 / 48_000.0 + c.audio_phase_seconds,
        ClipContent::Midi { .. } => panic!("an audio clip"),
    }
}

/// Presses `key` with `modifiers` on the focused handle, checks `changed`,
/// then undoes it and checks the clip is exactly as it was.
fn key_then_undo(
    lumen: &mut Lumen,
    id: Id,
    (modifiers, key): (Modifiers, Key),
    changed: impl Fn(&Clip, &Clip) -> bool,
    what: &str,
) {
    let before = clip(lumen, id);
    lumen.key(modifiers, key);
    let after = clip(lumen, id);
    assert!(changed(&before, &after), "{what}: {before:?} -> {after:?}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        clip(lumen, id),
        before,
        "{what}: one undo step takes it back"
    );
}

#[test]
fn every_handle_moves_by_key_and_each_press_is_one_undo_step() {
    let (mut lumen, id) = opened("editor-every-key");
    to_the_detail_panel(&mut lumen);
    let end = |c: &Clip| c.start + c.length;

    tab_to(&mut lumen, "trim start");
    key_then_undo(
        &mut lumen,
        id,
        (Modifiers::NONE, Key::ArrowRight),
        |b, a| a.start > b.start && end(a) == end(b),
        "trim start by the grid",
    );
    key_then_undo(
        &mut lumen,
        id,
        (Modifiers::ALT, Key::ArrowLeft),
        |b, a| a.start < b.start && b.start - a.start < Ticks(PPQ / 16),
        "trim start finely",
    );
    tab_to(&mut lumen, "fade in");
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert!(clip(&lumen, id).fade_in.length > Ticks::ZERO);
    key_then_undo(
        &mut lumen,
        id,
        (Modifiers::NONE, Key::ArrowUp),
        |b, a| a.fade_in.curve != b.fade_in.curve && a.fade_in.length == b.fade_in.length,
        "fade in bent",
    );
    tab_to(&mut lumen, "fade out");
    key_then_undo(
        &mut lumen,
        id,
        (Modifiers::NONE, Key::ArrowLeft),
        |b, a| a.fade_out.length > b.fade_out.length && end(a) == end(b),
        "fade out by the grid",
    );
    tab_to(&mut lumen, "trim end");
    key_then_undo(
        &mut lumen,
        id,
        (Modifiers::NONE, Key::ArrowLeft),
        |b, a| end(a) < end(b) && a.start == b.start,
        "trim end by the grid",
    );
    // Typed: the end at bar 1, beat 3 and a half.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("1.3.3".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert_eq!(end(&clip(&lumen, id)), Ticks::quarters(2) + Ticks(PPQ / 2));
    assert_eq!(lumen.app().undo_label(), Some("Trim clip"));
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(lumen.app().audio_handle_focused(&ctx), Some("trim end"));
}

#[test]
fn ctrl_alt_slips_the_bright_part_and_a_bend_dot_bends_and_straightens() {
    let (mut lumen, id) = opened("editor-slip-bend");
    let before = clip(&lumen, id);
    let mid = shown(&lumen).body.center().y;
    let bright = Pos2::new(x_of(&lumen, 2.0), mid + 20.0);
    lumen.drag(
        bright,
        bright + vec2(-40.0, 0.0),
        Modifiers::COMMAND | Modifiers::ALT,
    );
    let slipped = clip(&lumen, id);
    assert!(
        source_at(&slipped) > source_at(&before),
        "the audio slid earlier"
    );
    assert_eq!(
        (slipped.start, slipped.length),
        (before.start, before.length)
    );
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);

    // A plain drag on the bright part changes nothing until the range
    // arrives (S2b).
    lumen.drag(bright, bright + vec2(-40.0, 0.0), Modifiers::NONE);
    assert_eq!(clip(&lumen, id), before, "the bright part has no drag yet");

    let h = handles(&lumen);
    lumen.drag(
        h.fade_in,
        Pos2::new(x_of(&lumen, 2.0), h.fade_in.y),
        Modifiers::NONE,
    );
    let faded = clip(&lumen, id);
    let dot = handles(&lumen).bend_in.expect("a fade has its bend dot");
    lumen.drag(dot, dot + vec2(0.0, -30.0), Modifiers::NONE);
    let bent = clip(&lumen, id);
    assert_ne!(bent.fade_in.curve, faded.fade_in.curve, "the drag bends it");
    assert_eq!(bent.fade_in.length, faded.fade_in.length);
    assert_eq!(lumen.app().undo_label(), Some("Fade curve"));
    let dot = handles(&lumen).bend_in.expect("still there");
    lumen.double_click(dot);
    assert_eq!(
        clip(&lumen, id).fade_in.curve,
        lumen_core::curve::Curve::LINEAR,
        "a double-click straightens it"
    );
}

#[test]
fn the_shape_menus_of_a_crossfade_show_the_curve_they_change() {
    let (mut lumen, left, right) = two_halves("editor-crossfade-shapes");
    let h = handles(&lumen);
    lumen.drag(
        h.fade_out,
        Pos2::new(h.end + 60.0, h.fade_out.y),
        Modifiers::NONE,
    );
    assert!(clip(&lumen, right).crossfade, "a crossfade");
    let shape = |lumen: &Lumen| clip(lumen, right).fade_in.curve;

    // The popover on the left clip's fade out, opened twice: the second time
    // it shows Slow, so choosing Linear is a change.
    to_the_detail_panel(&mut lumen);
    for chosen in [FadeShape::Slow, FadeShape::Linear] {
        tab_to(&mut lumen, "fade out");
        lumen.key(Modifiers::NONE, Key::Enter);
        lumen.frames(3);
        lumen.click(&format!("Fade out: {}", chosen.name()));
        lumen.frames(3);
        assert_eq!(shape(&lumen), chosen.curve());
    }

    // The arrangement's Fades submenu reads the same curve.
    for chosen in [FadeShape::Fast, FadeShape::Linear] {
        let ctx = lumen.harness.ctx.clone();
        let wave = lumen
            .app()
            .clip_handles(&ctx, left)
            .expect("the clip shows its handles")
            .wave;
        let p = Pos2::new(wave.center().x, wave.bottom() - 3.0);
        lumen.harness.hover_at(p);
        lumen.frames(1);
        for pressed in [true, false] {
            lumen.harness.event(Event::PointerButton {
                pos: p,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            lumen.frames(1);
        }
        lumen.frames(2);
        lumen.harness.get_by_label("Fades").click();
        lumen.frames(3);
        lumen.click(&format!("Fade out: {}", chosen.name()));
        lumen.frames(3);
        assert_eq!(shape(&lumen), chosen.curve());
    }
}

/// A four-second file split at beat 4, its left half open in the editor.
fn two_halves(name: &str) -> (Lumen, Id, Id) {
    let (mut lumen, left) = opened_with(Lumen::start(name), 4.0, (0, 8));
    lumen.app_mut().locate_to(Ticks::quarters(4));
    lumen.wait_for("the playhead at beat 4", |app| {
        (app.playhead().0 - Ticks::quarters(4).0).abs() < PPQ / 8
    });
    lumen.app_mut().select_clip(left);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::E);
    let right = lumen.app().project().tracks[0].clips[1].id;
    open(&mut lumen, left);
    (lumen, left, right)
}

#[test]
fn keys_on_both_sides_of_a_crossfade_act_on_the_saved_fades() {
    let (mut lumen, left, right) = two_halves("editor-crossfade-keys");
    // The left half: a plain fade in, then a crossfade out of it.
    let h = handles(&lumen);
    lumen.drag(
        h.fade_in,
        Pos2::new(x_of(&lumen, 1.0), h.fade_in.y),
        Modifiers::NONE,
    );
    assert_eq!(clip(&lumen, left).fade_in.length, Ticks::quarters(1));
    let h = handles(&lumen);
    lumen.drag(
        h.fade_out,
        Pos2::new(h.end + 60.0, h.fade_out.y),
        Modifiers::NONE,
    );
    assert!(
        clip(&lumen, right).crossfade,
        "a crossfade into the right half"
    );

    // Its fade in still grows from where it is, by the grid.
    to_the_detail_panel(&mut lumen);
    tab_to(&mut lumen, "fade in");
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    let grown = clip(&lumen, left).fade_in.length;
    assert!(grown > Ticks::quarters(1), "{grown:?}");

    // The right half: its fade in is the crossfade, named so, and Up bends
    // it a step further each time.
    open(&mut lumen, right);
    to_the_detail_panel(&mut lumen);
    tab_to(&mut lumen, "fade in");
    assert!(
        lumen
            .harness
            .query_all_by_label_contains("Fade in: Crossfade")
            .next()
            .is_some(),
        "the stop says it is a crossfade"
    );
    let level = |lumen: &Lumen| clip(lumen, right).fade_in.curve.handle().dot[1];
    let first = level(&lumen);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    let second = level(&lumen);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    let third = level(&lumen);
    assert!(first < second && second < third, "{first} {second} {third}");
}
