//! The audio editor's range and edit cursor (M9.5 S2b, WD A, CU A, ZC A,
//! D15, D23, D28): a drag on the part the clip plays picks the
//! arrangement's own range, which split, crop, delete and Join act on; a
//! range is cut to the heard stretch; a click places the edit cursor that
//! Ctrl+E splits at; Zero crossings moves an edge onto a quiet frame, from
//! memory and from a file played from disk; the wave from the keyboard; a
//! range dragged past the view scrolls it.

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};

use super::audio_editor_handles::{
    clip, open, opened, opened_with, shown, to_the_detail_panel, x_of,
};
use super::{Lumen, TempDir, midi_play};

fn clips(lumen: &Lumen) -> usize {
    lumen.app().project().tracks[0].clips.len()
}

fn range(lumen: &Lumen) -> Option<(Ticks, Ticks)> {
    lumen.app().time_range().map(|(a, b, _)| (a, b))
}

/// The middle of the wave, at beat `q`.
fn at(lumen: &Lumen, q: f64) -> Pos2 {
    Pos2::new(x_of(lumen, q), shown(lumen).body.center().y)
}

fn q(beats: f64) -> Ticks {
    Ticks((beats * PPQ as f64).round() as i64)
}

/// Presses Tab until the editor's stop `name` has the keyboard.
fn tab_to_stop(lumen: &mut Lumen, name: &str) {
    let ctx = lumen.harness.ctx.clone();
    for _ in 0..90 {
        if lumen.app().audio_stop_focused(&ctx) == Some(name) {
            return;
        }
        lumen.key(Modifiers::NONE, Key::Tab);
    }
    panic!("Tab never reached the editor's {name} stop");
}

#[test]
fn a_drag_on_the_wave_picks_a_range_that_split_crop_delete_and_join_act_on() {
    let (mut lumen, id) = opened("editor-range-drag");
    let track = lumen.app().project().tracks[0].id;
    let before = clip(&lumen, id);
    lumen.harness.hover_at(at(&lumen, 2.0));
    lumen.frames(2);
    assert!(lumen.info().contains("pick a range"), "{}", lumen.info());
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    let (a, b, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!((a, b), (q(1.5), q(2.5)), "on the grid");
    assert_eq!(tracks, [track], "on the clip's track");
    assert_eq!(clip(&lumen, id), before, "picking edits nothing");

    // Ctrl+E splits at both edges; Join puts them back; undo each.
    lumen.harness.hover_at(at(&lumen, 2.0));
    lumen.key(Modifiers::COMMAND, Key::E);
    assert_eq!(clips(&lumen), 3, "split at both edges");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clips(&lumen), 1);
    assert_eq!(clip(&lumen, id), before, "one undo step");

    // Delete takes the range out of the clip.
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    lumen.key(Modifiers::NONE, Key::Delete);
    let left: Vec<_> = lumen.app().project().tracks[0]
        .clips
        .iter()
        .map(|c| (c.start, c.start + c.length))
        .collect();
    assert_eq!(left, [(q(1.0), q(1.5)), (q(2.5), q(3.0))], "{left:?}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);

    // Crop keeps only the range.
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    midi_play::palette(&mut lumen, "Crop to selection");
    let kept: Vec<_> = lumen.app().project().tracks[0]
        .clips
        .iter()
        .map(|c| (c.start, c.start + c.length))
        .collect();
    assert_eq!(kept, [(q(1.5), q(2.5))]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);
}

#[test]
fn a_split_range_joins_back_into_the_clip() {
    let (mut lumen, id) = opened("editor-range-join");
    let before = clip(&lumen, id);
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    lumen.harness.hover_at(at(&lumen, 2.0));
    lumen.key(Modifiers::COMMAND, Key::E);
    assert_eq!(clips(&lumen), 3);
    lumen.app_mut().select_all_clips_on_track(0);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::J);
    lumen.wait_for("the parts joined", |app| {
        app.project().tracks[0].clips.len() == 1
    });
    let joined = lumen.app().project().tracks[0].clips[0].clone();
    assert_eq!(
        (joined.start, joined.length),
        (before.start, before.length),
        "split and join give the clip back"
    );
}

#[test]
fn a_range_is_cut_to_the_heard_stretch() {
    // One turn is beats 0 to 2; the clip loops to beat 8 and is split at 5:
    // the later clip starts a beat into its loop, and is heard as the file
    // plays it only up to its first repeat at beat 6.
    let (mut lumen, id) = opened_with(Lumen::start("editor-range-heard"), 2.0, (0, 2));
    lumen
        .app_mut()
        .trim_clip_end_to(id, Ticks::quarters(8), true);
    lumen.app_mut().locate_to(Ticks::quarters(5));
    lumen.app_mut().select_clip(id);
    lumen.frames(2);
    lumen.key(Modifiers::COMMAND, Key::E);
    let later = lumen.app().project().tracks[0].clips[1].id;
    open(&mut lumen, later);
    lumen.drag(
        at(&lumen, 5.25),
        at(&lumen, 5.9) + vec2(200.0, 0.0),
        Modifiers::NONE,
    );
    let (a, b) = range(&lumen).expect("a range");
    assert_eq!(a, q(5.25));
    assert_eq!(b, q(6.0), "it stops at the first repeat");
}

#[test]
fn a_click_places_the_cursor_and_ctrl_e_splits_there() {
    let (mut lumen, id) = opened("editor-cursor");
    let head = lumen.app().playhead();
    lumen.harness.hover_at(at(&lumen, 2.0) + vec2(2.0, 0.0));
    lumen.frames(1);
    lumen.click_at(at(&lumen, 2.0) + vec2(2.0, 0.0), Modifiers::NONE);
    lumen.frames(3);
    assert_eq!(
        lumen.app().audio_editor_cursor(),
        Some(q(2.0)),
        "on the grid"
    );
    assert_eq!(lumen.app().playhead(), head, "the playhead stays");
    lumen.key(Modifiers::COMMAND, Key::E);
    let parts: Vec<_> = lumen.app().project().tracks[0]
        .clips
        .iter()
        .map(|c| c.start)
        .collect();
    assert_eq!(parts, [q(1.0), q(2.0)], "split at the cursor");
    assert_eq!(lumen.app().project().tracks[0].clips[0].id, id);
    lumen.key(Modifiers::COMMAND, Key::Z);

    // A click on the dimmed file clears it, and Esc does too.
    lumen.harness.get_by_label("Whole file").click();
    lumen.frames(3);
    lumen.click_at(at(&lumen, 2.0), Modifiers::NONE);
    lumen.frames(2);
    assert!(lumen.app().audio_editor_cursor().is_some());
    lumen.click_at(at(&lumen, 0.5), Modifiers::NONE);
    lumen.frames(2);
    assert_eq!(lumen.app().audio_editor_cursor(), None);
    lumen.click_at(at(&lumen, 2.0), Modifiers::NONE);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(lumen.app().audio_editor_cursor(), None, "Esc clears it");
}

/// The file's frame under the cursor.
fn cursor_frame(lumen: &Lumen) -> f64 {
    let at = lumen.app().audio_editor_cursor().expect("a cursor");
    lumen
        .app()
        .audio_editor_frame_at(at.as_quarters())
        .expect("the cursor is over the file")
}

#[test]
fn zero_crossings_move_the_cursor_onto_the_quiet_frame() {
    // The test file is sin(0.05 n): beat 2 is frame 48,000, and the edge
    // goes to the quietest frame within 5 ms (240 frames) of it.
    let quiet = quietest(48_000, 240);
    let (mut lumen, _) = opened("editor-crossings");
    lumen.click_at(at(&lumen, 2.0) + vec2(1.0, 0.0), Modifiers::NONE);
    lumen.frames(2);
    assert!(
        (cursor_frame(&lumen) - 48_000.0).abs() < 0.5,
        "the grid alone"
    );
    lumen.app_mut().set_zero_crossings(true);
    lumen.click_at(at(&lumen, 2.0) + vec2(1.0, 0.0), Modifiers::NONE);
    lumen.frames(2);
    let frame = cursor_frame(&lumen);
    assert!((frame - quiet as f64).abs() < 0.5, "on {quiet}: {frame}");
    // Alt places it freely, crossing or not.
    lumen.click_at(at(&lumen, 2.0) + vec2(1.0, 0.0), Modifiers::ALT);
    assert!(
        (cursor_frame(&lumen) - quiet as f64).abs() > 0.5,
        "Alt leaves the crossing"
    );
}

#[test]
fn a_crossing_from_disk_lands_while_held_and_a_released_drag_makes_no_later_edit() {
    let lumen = Lumen::start_with(TempDir::new("e2e-editor-crossings-disk"), |launch| {
        launch.playing_from_disk()
    });
    let (mut lumen, _) = opened_with(lumen, 40.0, (1, 3));
    assert_eq!(lumen.app().media_from_disk(), [("kick".into(), true)]);
    lumen.app_mut().set_zero_crossings(true);
    // Held still at beat 2, the end stands on the grid until the read
    // answers, then lands on the quietest frame (D23).
    let from = at(&lumen, 1.5);
    let to = at(&lumen, 2.0) + vec2(1.0, 0.0);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=4 {
        lumen
            .harness
            .hover_at(from + (to - from) * (step as f32 / 4.0));
        lumen.frames(1);
    }
    let quiet = quietest(48_000, 240) as f64;
    lumen.run_until("the end on its crossing", |l| {
        range(l).is_some_and(|(_, b)| {
            l.app()
                .audio_editor_frame_at(b.as_quarters())
                .is_some_and(|f| (f - quiet).abs() < 0.5)
        })
    });
    lumen.harness.drop_at(to);
    lumen.frames(3);

    // A quick drag elsewhere: whatever stood at release stays, and the
    // answer that arrives later makes no edit.
    lumen.drag(at(&lumen, 2.5), at(&lumen, 2.75), Modifiers::NONE);
    let picked = range(&lumen).expect("a range");
    let undo = lumen.app().undo_label().map(str::to_owned);
    lumen.wait_for("the read answered", |app| !app.audio_crossings_waiting());
    lumen.frames(5);
    assert_eq!(range(&lumen), Some(picked), "nothing moves after release");
    assert_eq!(lumen.app().undo_label().map(str::to_owned), undo);
}

#[test]
fn with_no_pointer_the_wave_places_the_cursor_splits_and_picks_a_range() {
    let (mut lumen, id) = opened("editor-wave-keys");
    let before = clip(&lumen, id);
    // The pointer is away from the editor: only the keyboard works here.
    lumen.harness.hover_at(Pos2::new(5.0, 5.0));
    to_the_detail_panel(&mut lumen);
    tab_to_stop(&mut lumen, "wave");
    // The playhead is at 0, outside the heard stretch: the first press
    // puts the cursor at the stretch's start; the next moves it by the
    // grid.
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert_eq!(lumen.app().audio_editor_cursor(), Some(q(1.0)));
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    let cursor = lumen.app().audio_editor_cursor().expect("a cursor");
    assert!(cursor > q(1.0) && cursor < q(3.0), "{cursor:?}");
    assert!(
        lumen
            .harness
            .get_all_by(|n| n
                .label()
                .is_some_and(|l| l.starts_with("Clip wave, cursor at")))
            .next()
            .is_some(),
        "its name says where"
    );
    lumen.key(Modifiers::COMMAND, Key::E);
    assert_eq!(clips(&lumen), 2, "Ctrl+E splits at the cursor");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before);

    // Shift picks a range from the cursor; Delete takes it out.
    tab_to_stop(&mut lumen, "wave");
    lumen.key(Modifiers::NONE, Key::Home);
    assert_eq!(lumen.app().audio_editor_cursor(), Some(q(1.0)));
    lumen.key(Modifiers::SHIFT, Key::ArrowRight);
    lumen.key(Modifiers::SHIFT, Key::ArrowRight);
    let (a, b) = range(&lumen).expect("a range from the keys");
    assert_eq!(a, q(1.0));
    assert!(b > a);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(
        lumen.app().project().tracks[0].clips[0].start,
        b,
        "the range went"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id), before, "one undo step");

    // Esc clears the range, and the cursor comes back at its anchor; then
    // Esc clears the cursor.
    tab_to_stop(&mut lumen, "wave");
    lumen.key(Modifiers::SHIFT, Key::End);
    assert!(range(&lumen).is_some());
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(range(&lumen), None);
    assert_eq!(
        lumen.app().audio_editor_cursor(),
        Some(q(1.0)),
        "the anchor"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(lumen.app().audio_editor_cursor(), None);
}

#[test]
fn a_range_dragged_past_the_view_scrolls_it() {
    let (mut lumen, _) = opened_with(Lumen::start("editor-range-scroll"), 10.0, (1, 12));
    lumen.harness.get_by_label("Zoom to clip").click();
    lumen.frames(3);
    // Zoom in so the clip runs past the right edge.
    lumen.harness.hover_at(at(&lumen, 1.5));
    for _ in 0..4 {
        lumen.key(Modifiers::NONE, Key::Plus);
    }
    lumen.frames(3);
    let s = shown(&lumen);
    let from = Pos2::new(s.body.left() + 40.0, s.body.center().y);
    let edge = Pos2::new(s.body.right() - 2.0, s.body.center().y);
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=8 {
        lumen
            .harness
            .hover_at(from + (edge - from) * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.frames(30);
    let moved = shown(&lumen).view;
    lumen.harness.drop_at(edge);
    lumen.frames(3);
    assert!(moved.left > s.view.left, "the view scrolled on");
    assert!(lumen.app().follow_paused(), "scrolling pauses Follow");
    let (_, b) = range(&lumen).expect("a range");
    assert!(
        b.as_quarters() > s.view.beat_at(s.body.left(), s.body.right()),
        "the range reaches past the first view"
    );
}

/// The test file's quietest frame within `reach` of `frame`, the nearer on
/// a tie, as the file holds it (`wav`: sin(0.05 n) at 3,000, in 16 bits).
fn quietest(frame: i64, reach: i64) -> i64 {
    let level = |n: i64| (((n as f32) * 0.05).sin() * 3_000.0) as i16;
    (frame - reach..=frame + reach)
        .min_by_key(|&n| (level(n).unsigned_abs(), (n - frame).abs()))
        .expect("a frame")
}

#[test]
fn zero_crossings_settle_a_dragged_trim_edge_on_the_quiet_frame_and_alt_leaves_it() {
    let quiet = quietest(48_000, 240);
    for (alt, name) in [
        (false, "editor-trim-crossing"),
        (true, "editor-trim-crossing-alt"),
    ] {
        let (mut lumen, id) = opened(name);
        lumen.app_mut().set_zero_crossings(true);
        let grip = Pos2::new(
            super::audio_editor_handles::handles(&lumen).end - 2.0,
            shown(&lumen).body.center().y,
        );
        let to = at(&lumen, 2.0) + vec2(1.0, 0.0);
        let modifiers = if alt { Modifiers::ALT } else { Modifiers::NONE };
        lumen.drag(grip, to, modifiers);
        let end = {
            let c = clip(&lumen, id);
            c.start + c.length
        };
        let frame = lumen
            .app()
            .audio_editor_frame_at(end.as_quarters())
            .expect("the end is over the file");
        if alt {
            assert!(
                (frame - quiet as f64).abs() > 0.5,
                "Alt leaves the crossing"
            );
        } else {
            assert!((frame - quiet as f64).abs() < 0.5, "on {quiet}: {frame}");
        }
        assert_eq!(lumen.app().undo_label(), Some("Trim clip"), "one step");
    }
}

#[test]
fn a_right_click_inside_the_range_keeps_it() {
    let (mut lumen, _id) = opened("editor-range-menu");
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    let picked = range(&lumen).expect("a range");
    lumen.right_click(at(&lumen, 2.0));
    lumen.frames(3);
    assert_eq!(range(&lumen), Some(picked), "the range stands");
    // What the range was for still works from the menu's place.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(range(&lumen), Some(picked), "Esc closed the menu only");
    lumen.harness.hover_at(at(&lumen, 2.0));
    lumen.key(Modifiers::COMMAND, Key::E);
    assert_eq!(clips(&lumen), 3, "split at both edges");
}

#[test]
fn delete_in_the_editor_takes_its_range_not_a_point_picked_on_the_arrangement() {
    use lumen_core::automation::{Lane, Point};
    use lumen_project::automation::Target;
    let (mut lumen, id) = opened("editor-delete-isolation");
    let track = lumen.app().project().tracks[0].id;
    let lane = Lane::new(vec![Point::new(q(2.0), 0.0), Point::new(q(4.0), -6.0)]).unwrap();
    lumen
        .app_mut()
        .give_lane(track, Target::Volume, lane)
        .unwrap();
    lumen.app_mut().select_tracks(&[track]);
    lumen.frames(3);
    lumen.harness.hover_at(Pos2::new(5.0, 5.0));
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(30);
    let spot = super::lane_view::point_pos(&lumen, track, 0);
    super::lane_view::click_at(&mut lumen, spot);
    assert!(
        lumen.app().automation_picked().is_some(),
        "a point is picked"
    );

    // The editor takes the keyboard with a range of its own.
    lumen.drag(at(&lumen, 1.5), at(&lumen, 2.5), Modifiers::NONE);
    assert!(range(&lumen).is_some());
    lumen.harness.hover_at(at(&lumen, 2.0));
    lumen.frames(2);
    let before = super::lane_view::points(&lumen, track, Target::Volume);
    lumen.key(Modifiers::NONE, Key::Delete);
    assert_eq!(
        super::lane_view::points(&lumen, track, Target::Volume),
        before,
        "the lane's point stays"
    );
    assert_ne!(clips(&lumen), 1, "the range was taken out of the clip");
    let _ = id;
}
