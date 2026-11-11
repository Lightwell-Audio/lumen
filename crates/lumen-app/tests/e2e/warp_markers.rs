//! The audio editor's warp markers (M9.5 S5b, the owner's G B and P A): the
//! warp strip between the ruler and the wave, a ghost marker that is made and
//! moved in one drag, double-click to add and delete, Shift and drag to slide
//! the audio under a marker, the 25% to 400% limit, and the markers from the
//! keyboard. Each drag is one undo step.
//!
//! "Fix this hit" (M9.5 review M2, the owner's C) is here too: on a transient
//! it pins the hits either side in one step, so a drag of the hit's own marker
//! moves only it, and the command does the same on the focused marker.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::{PPQ, Ticks};
use lumen_core::warp::{MAX_RATIO, MIN_RATIO, ratio};
use lumen_project::model::{Clip, Id};

use super::audio_editor_handles::{clip, open, shown, x_of};
use super::clip_commands::command;
use super::warp_chip::{clicks_at, close_chip, open_chip, opened_click_track};
use super::{Lumen, TempDir};

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
    assert_eq!(strip.height(), 18.0);
    assert_eq!(strip.bottom(), after.top(), "the wave starts under it");
    assert!((after.top() - before.top() - 18.0).abs() < 0.01);
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

/// Where marker `index` plays on the timeline, and the grid line it is on
/// (`None` when it is between lines).
fn on_a_line(lumen: &Lumen, id: Id, index: usize) -> (Ticks, bool) {
    let c = clip(lumen, id);
    let t = c.start + markers(lumen, id)[index].at;
    let (line, _) = lumen.app().audio_editor_grid_cell_at(t).expect("a grid");
    (t, line == t)
}

#[test]
fn a_dragged_marker_lands_on_a_grid_line() {
    let (mut lumen, id) = warped_editor("warp-snap");
    let from = on_strip(&lumen, 2.0);
    for dx in [37.0, 61.0, 83.0, 107.0] {
        lumen.drag(from, Pos2::new(from.x + dx, from.y), Modifiers::NONE);
        let (t, on) = on_a_line(&lumen, id, 1);
        assert!(on, "a drag of {dx} points left the marker at {t:?}");
        lumen.key(Modifiers::COMMAND, Key::Z);
        lumen.frames(3);
    }
}

#[test]
fn a_marker_goes_to_the_line_the_pointer_is_on_however_it_was_grabbed() {
    let (mut lumen, id) = warped_editor("warp-snap-pointer");
    lumen.double_click(on_strip(&lumen, 2.0));
    let (strip, flags) = lumen.app().audio_warp_strip().unwrap();
    let y = strip.center().y;
    // Grabbed 5 points off its centre, then the pointer is put on beat 3.
    lumen.drag(
        Pos2::new(flags[1] + 5.0, y),
        Pos2::new(x_of(&lumen, 3.0), y),
        Modifiers::NONE,
    );
    let c = clip(&lumen, id);
    assert_eq!(
        c.start + markers(&lumen, id)[1].at,
        c.start + Ticks::quarters(3)
    );
}

#[test]
fn a_marker_made_by_a_click_lands_on_a_grid_line() {
    let (mut lumen, id) = warped_editor("warp-snap-click");
    lumen.double_click(on_strip(&lumen, 2.3));
    let (t, on) = on_a_line(&lumen, id, 1);
    assert!(on, "a double-click left the marker at {t:?}");
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

/// The tempo of the loop these tests fix a hit in, and the hit played late:
/// eight beats at 100 BPM, the sixth 80 ms late.
const LOOP_BPM: f64 = 100.0;
const BEAT: f64 = 60.0 / LOOP_BPM;
const LATE: usize = 5;
const LATE_BY: f64 = 0.08;

/// The second of the file the late hit is at.
fn late_at() -> f64 {
    LATE as f64 * BEAT + LATE_BY
}

/// The second of the file each of the loop's eight hits is at.
fn hit_at(k: usize) -> f64 {
    k as f64 * BEAT + if k == LATE { LATE_BY } else { 0.0 }
}

/// A loop of eight beats at 100 BPM with the sixth hit played late, dropped
/// in (so it follows the song), opened in the editor and listened to.
fn warped_late_loop(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start_with(TempDir::new(&format!("e2e-{name}")), |l| {
        l.playing_from_disk()
    });
    let onsets: Vec<f64> = (0..8).map(hit_at).collect();
    let file = clicks_at(&lumen.dir().join("Drums 100 BPM.wav"), &onsets, 8.0 * BEAT);
    lumen.drop_files(&[file]);
    lumen.wait_for("the loop to follow the song", |app| {
        app.project()
            .tracks
            .first()
            .and_then(|t| t.clips.first())
            .is_some_and(|c| c.warp.is_some())
    });
    lumen.frames(30);
    let id = lumen.app().project().tracks[0].clips[0].id;
    open(&mut lumen, id);
    lumen.wait_for("detection", |app| app.audio_detection().ready);
    lumen.frames(3);
    (lumen, id)
}

/// The strip's middle line where the late hit plays, from the clip's map.
fn on_late_hit(lumen: &Lumen, id: Id) -> Pos2 {
    let c = clip(lumen, id);
    let w = c.warp.as_ref().expect("the loop is warped");
    let at = c.start + lumen_core::warp::tick_at_source(&w.markers, w.original_bpm, late_at());
    on_strip(lumen, at.as_quarters())
}

/// Plays from the start for four seconds and gives the frames the hits began
/// at: the first frame over 0.1 after a quiet stretch.
fn heard_hits(lumen: &mut Lumen) -> Vec<usize> {
    lumen.key(Modifiers::NONE, Key::Home);
    lumen.wait_for("the rewind", |app| app.playhead() == Ticks::ZERO);
    lumen.key(Modifiers::NONE, Key::Space);
    let heard = lumen.listen(Duration::ZERO, 4 * 48_000);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    let mut found: Vec<usize> = Vec::new();
    for (i, f) in heard.iter().enumerate() {
        if f[0].abs() > 0.1 && found.last().is_none_or(|last| i - last > 12_000) {
            found.push(i);
        }
    }
    found
}

/// Whether every hit of the loop plays at the beat it did in `before`: the
/// map's beat for each second of the file, whole ticks.
fn hits_did_not_move(lumen: &Lumen, id: Id, before: &Clip) {
    let was = before.warp.as_ref().expect("warped");
    let now = clip(lumen, id);
    let is = now.warp.as_ref().expect("still warped");
    for k in 0..8 {
        let at = hit_at(k);
        assert_eq!(
            lumen_core::warp::tick_at_source(&is.markers, is.original_bpm, at),
            lumen_core::warp::tick_at_source(&was.markers, was.original_bpm, at),
            "hit {k} at {at} s moved"
        );
    }
}

#[test]
fn fix_this_hit_pins_the_hits_either_side_so_only_the_late_hit_moves() {
    let (mut lumen, id) = warped_late_loop("warp-fix-hit");
    let before = clip(&lumen, id);
    assert_eq!(markers(&lumen, id).len(), 1, "one marker to begin with");

    // Right-click the late hit: the strip's menu offers to fix it, and one
    // step makes its marker and the two anchors.
    lumen.right_click(on_late_hit(&lumen, id));
    assert!(lumen.shows("Fix this hit"), "the menu offers it");
    lumen.click("Fix this hit");
    assert_eq!(lumen.app().undo_label(), Some("Fix this hit"));
    assert_eq!(
        markers(&lumen, id).len(),
        4,
        "the hit and the hits beside it"
    );
    hits_did_not_move(&lumen, id, &before);

    // Drag the hit's marker onto its beat: only that hit moves.
    let (strip, flags) = lumen.app().audio_warp_strip().expect("the strip");
    let hit = on_late_hit(&lumen, id);
    let index = flags
        .iter()
        .position(|x| (x - hit.x).abs() < 2.0)
        .expect("the hit's marker");
    let c = clip(&lumen, id);
    let to = x_of(
        &lumen,
        (c.start + Ticks::quarters(LATE as i64)).as_quarters(),
    );
    lumen.drag(
        Pos2::new(flags[index], strip.center().y),
        Pos2::new(to, strip.center().y),
        Modifiers::NONE,
    );
    let moved = clip(&lumen, id);
    let map = moved.warp.as_ref().expect("still warped");
    assert_eq!(map.markers[index].at, Ticks::quarters(LATE as i64));
    assert!((map.markers[index].source - late_at()).abs() < 0.001);
    for k in [LATE - 1, LATE + 1] {
        let source = map.source_at(Ticks::quarters(k as i64));
        assert!(
            (source - hit_at(k)).abs() < 0.002,
            "hit {k} stayed where it was: {source} s"
        );
    }

    // Heard: every hit half a second apart, the late one included.
    let heard = heard_hits(&mut lumen);
    assert!(heard.len() >= 7, "hits heard: {heard:?}");
    let gaps: Vec<f64> = heard[1..]
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 / 48_000.0)
        .collect();
    for gap in &gaps {
        assert!((gap - 0.5).abs() < 0.02, "every hit on the beat: {gaps:?}");
    }

    // One undo takes the drag back, a second the three markers.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    let kept = clip(&lumen, id);
    assert_eq!(markers(&lumen, id).len(), 4, "the markers stay");
    assert_eq!(
        lumen_core::warp::tick_at_source(
            &kept.warp.as_ref().unwrap().markers,
            kept.warp.as_ref().unwrap().original_bpm,
            late_at()
        ),
        lumen_core::warp::tick_at_source(
            &before.warp.as_ref().unwrap().markers,
            before.warp.as_ref().unwrap().original_bpm,
            late_at()
        ),
        "the hit is back where it played"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(clip(&lumen, id), before, "one undo takes all three markers");
}

#[test]
fn fix_this_hit_from_the_keyboard_pins_the_focused_markers_hits() {
    let (mut lumen, id) = warped_late_loop("warp-fix-hit-keys");
    let before = clip(&lumen, id);
    // A marker on the late hit, from the keyboard: the cursor goes there and
    // Add warp marker makes it and gives it the keyboard.
    let c = clip(&lumen, id);
    let w = c.warp.as_ref().expect("warped");
    let at = c.start + lumen_core::warp::tick_at_source(&w.markers, w.original_bpm, late_at());
    lumen.app_mut().set_audio_cursor(at);
    command(&mut lumen, "Add warp marker");
    assert_eq!(markers(&lumen, id).len(), 2);
    let made = clip(&lumen, id);

    // Fix this hit, from the palette, acts on that marker: its two neighbours
    // are pinned with it, in one step.
    command(&mut lumen, "Fix this hit");
    assert_eq!(lumen.app().undo_label(), Some("Fix this hit"));
    assert_eq!(
        markers(&lumen, id).len(),
        4,
        "the hit and the hits beside it"
    );
    hits_did_not_move(&lumen, id, &before);

    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(3);
    assert_eq!(
        clip(&lumen, id).warp,
        made.warp,
        "one undo takes the two anchors back"
    );
}

/// Renders the strip for a look, not a check: `SHOTS` names a folder and
/// `LOOK_SCALE` (default 1) the UI scale. The notches, the ghost under the
/// pointer, a picked marker and the triangles, with the grid under the wave.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn look_at_the_strip() {
    let (mut lumen, id) = warped_editor("warp-look");
    let scale: f32 = std::env::var("LOOK_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    lumen.harness.set_pixels_per_point(scale);
    lumen.frames(4);
    let c = clip(&lumen, id);
    for q in [1, 3] {
        lumen
            .app_mut()
            .set_audio_cursor(c.start + Ticks::quarters(q));
        command(&mut lumen, "Add warp marker");
        lumen.frames(3);
    }
    let at = on_strip(&lumen, 2.0);
    lumen.harness.hover_at(at);
    lumen.frames(4);
    let image = lumen.harness.render().expect("render");
    let dir = std::path::PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
    image
        .save(dir.join(format!("strip-dark-{}.png", (scale * 100.0) as u32)))
        .expect("save");
}
