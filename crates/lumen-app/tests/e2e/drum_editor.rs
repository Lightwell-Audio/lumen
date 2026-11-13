//! The drum editor's lanes (M8 D2): a clip on a track with a drum map opens as
//! one lane a sound; a hit is drawn, erased, moved, copied, boxed, nudged and
//! given a velocity; the lanes' own mute, solo and order are the map's; Q uses
//! a lane's own grid; Hide unused and the view switch are commands; and the
//! arrangement draws a mapped clip as drum rows.
//!
//! The gesture points come from the editor's own geometry (`App::drum_hit_point`
//! and the lane's name points), so a test clicks where a person would.

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::grid::{Feel, Size};
use lumen_core::time::Ticks;
use lumen_project::drums::presets::Preset;
use lumen_project::model::{ClipContent, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::midi_play::palette;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

/// A two-bar clip of a kick, a snare and closed hats on the 808-style map.
fn kit_project() -> Project {
    let notes = vec![
        Note::new(Ticks::ZERO, Ticks(60), 36, 110),
        Note::new(q(2), Ticks(60), 36, 100),
        Note::new(q(1), Ticks(60), 38, 100),
        Note::new(q(3), Ticks(60), 38, 96),
        Note::new(Ticks(PPQ_HALF), Ticks(30), 42, 80),
        Note::new(q(2) + Ticks(PPQ_HALF), Ticks(30), 42, 72),
    ];
    let mut project = project_of(
        "Drums",
        vec![midi_track(1, "Drums", "pluck", (0, 8), None, notes, vec![])],
    );
    let map = Preset::EIGHT_OH_EIGHT.map(Id(500));
    project.drum_maps = vec![map.clone()];
    let track = std::sync::Arc::make_mut(&mut project.tracks[0]);
    track.drum_map = Some(map.id);
    project
}

/// Half a quarter note in ticks.
const PPQ_HALF: i64 = lumen_core::time::PPQ / 2;

/// Starts Lumen on the kit project, opens its clip in the lanes, and gives the
/// panel the room the lanes need.
fn launch(name: &str) -> (Lumen, Id) {
    let project = kit_project();
    let clip = project.tracks[0].clips[0].id;
    let dir = TempDir::new(&format!("e2e-drum-{name}"));
    let path = dir.path().join("kit.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.frames(30);
    lumen.app_mut().set_detail_size(520.0);
    lumen.app_mut().open_clip_editor(clip);
    lumen.frames(30);
    (lumen, clip)
}

fn notes_of(lumen: &Lumen, clip: Id) -> Vec<Note> {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == clip)
        .and_then(|c| match &c.content {
            ClipContent::Midi { notes, .. } => Some(notes.clone()),
            ClipContent::Audio { .. } => None,
        })
        .expect("the clip")
}

fn note_at(lumen: &Lumen, clip: Id, pitch: u8, start: Ticks) -> Option<Note> {
    notes_of(lumen, clip)
        .into_iter()
        .find(|n| n.pitch() == pitch && n.start() == start)
}

fn map(lumen: &Lumen) -> lumen_project::model::DrumMap {
    lumen.app().project().drum_maps[0].clone()
}

fn hit(lumen: &Lumen, local: Ticks, input: u8) -> Pos2 {
    lumen
        .app()
        .drum_hit_point(local, input)
        .expect("the hit is in view")
}

#[test]
fn a_mapped_clip_opens_as_one_lane_a_sound_in_the_maps_order() {
    let (lumen, _) = launch("lanes-open");
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
    assert!(lumen.shows("Drum lanes: Drums"), "the lanes are the editor");
    // Every sound of the map, in its order, not only the three played.
    let lanes = lumen.app().open_drum_lanes();
    let map = map(&lumen);
    assert_eq!(lanes.len(), map.sounds.len());
    assert_eq!(
        lanes.iter().map(String::as_str).collect::<Vec<_>>(),
        map.sounds
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );
    // Each lane says what it holds for a screen reader, and its M and S are
    // there.
    assert!(lumen.shows("Kick lane, 2 hits"));
    assert!(lumen.shows("Snare lane, 2 hits"));
    assert!(lumen.shows("Closed Hat lane, 2 hits"));
    assert!(lumen.shows("Mute Kick") && lumen.shows("Solo Kick"));
    assert!(
        lumen.shows("Move the Kick lane"),
        "the drawing is the lane's drag handle"
    );
}

#[test]
fn a_click_on_a_lane_auditions_it_and_a_vertical_drag_on_a_hit_sets_its_velocity() {
    let (mut lumen, clip) = launch("lanes-velocity");
    let name = lumen.app().drum_name_point(38).expect("the snare lane");
    lumen.click_at(name, Modifiers::NONE);
    assert!(lumen.app().audition_sounding(), "the sound was heard");
    lumen.app_mut().release_audition();

    // Pick the snare hit on beat 2, then drag up: its velocity, not its lane.
    let at = hit(&lumen, q(1), 38);
    lumen.click_at(at, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked().len(), 1);
    let before = note_at(&lumen, clip, 38, q(1)).expect("the hit").velocity();
    lumen.drag(at, at - vec2(0.0, 6.0), Modifiers::NONE);
    let after = note_at(&lumen, clip, 38, q(1)).expect("the hit");
    assert!(
        after.velocity() > before,
        "louder: {before} to {}",
        after.velocity()
    );
    assert_eq!(after.start(), q(1), "the drag did not move it");
    assert_eq!(
        lumen.app().undo_label(),
        Some("Draw velocities"),
        "one undo step"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        note_at(&lumen, clip, 38, q(1)).expect("back").velocity(),
        before
    );
}

#[test]
fn the_draw_tool_adds_a_hit_and_removes_one_in_its_lane() {
    let (mut lumen, clip) = launch("lanes-draw");
    lumen.key(Modifiers::NONE, Key::B);
    assert!(lumen.app().editor_draw_tool());
    // A kick on beat 4 in the kick lane's row.
    let at = hit(&lumen, q(4), 36);
    lumen.click_at(at, Modifiers::NONE);
    assert!(
        note_at(&lumen, clip, 36, q(4)).is_some(),
        "a hit was added to the kick's lane"
    );
    assert_eq!(lumen.app().undo_label(), Some("Draw note"));
    // Clicking the hit again removes it.
    let at = hit(&lumen, q(4), 36);
    lumen.click_at(at, Modifiers::NONE);
    assert!(note_at(&lumen, clip, 36, q(4)).is_none());
    assert_eq!(lumen.app().undo_label(), Some("Delete note"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        note_at(&lumen, clip, 36, q(4)).is_some(),
        "undo brings it back"
    );
}

#[test]
fn a_hit_moves_along_its_lane_and_a_copy_leaves_the_original() {
    let (mut lumen, clip) = launch("lanes-move");
    let at = hit(&lumen, q(1), 38);
    lumen.click_at(at, Modifiers::NONE);
    let zoom = lumen.app().editor_view().expect("the view").zoom as f32;
    // One beat right, staying in the snare's lane.
    lumen.drag(at, at + vec2(zoom, 0.0), Modifiers::NONE);
    assert!(
        note_at(&lumen, clip, 38, q(2)).is_some(),
        "moved a beat later, still the snare: {:?}",
        notes_of(&lumen, clip)
    );
    let at = hit(&lumen, q(2), 38);
    lumen.drag(at, at + vec2(zoom, 0.0), Modifiers::COMMAND);
    assert!(
        note_at(&lumen, clip, 38, q(2)).is_some(),
        "the original stays"
    );
    assert_eq!(
        notes_of(&lumen, clip)
            .iter()
            .filter(|n| n.pitch() == 38)
            .count(),
        3,
        "the copy is a third snare"
    );
}

#[test]
fn a_box_picks_the_hits_it_covers_and_the_arrows_nudge_them() {
    let (mut lumen, clip) = launch("lanes-box");
    // A box over both hat hits, in the closed hat's row: from empty space at
    // the row's left edge to just past the second hit.
    let body = lumen.app().editor_body().expect("the lanes drew");
    let row = hit(&lumen, Ticks::ZERO, 42);
    let from = Pos2::new(body.left() + 2.0, row.y - 12.0);
    let to = hit(&lumen, q(2) + Ticks(PPQ_HALF), 42) + vec2(10.0, 12.0);
    lumen.drag(from, to, Modifiers::NONE);
    assert_eq!(lumen.app().editor_picked().len(), 2, "both hats");
    let before: Vec<Ticks> = notes_of(&lumen, clip)
        .iter()
        .filter(|n| n.pitch() == 42)
        .map(|n| n.start())
        .collect();
    // The keyboard over the editor, as after a click in it.
    lumen.harness.hover_at(to);
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    let after: Vec<Ticks> = notes_of(&lumen, clip)
        .iter()
        .filter(|n| n.pitch() == 42)
        .map(|n| n.start())
        .collect();
    assert!(
        after.iter().zip(&before).all(|(a, b)| *a > *b),
        "nudged later: {before:?} to {after:?}"
    );
}

#[test]
fn quantize_uses_the_lanes_own_grid() {
    let (mut lumen, clip) = launch("lanes-quantize");
    // The snare's lane gets a quarter-note grid.
    lumen
        .app_mut()
        .set_lane_quantize(38, Size::Note(4), Feel::Straight);
    // Draw a snare 3/4 of a beat after beat 2: a 1/16 of the editor's grid,
    // off the lane's quarters.
    let at = hit(&lumen, q(1) + Ticks(3 * lumen_core::time::PPQ / 4), 38);
    lumen.key(Modifiers::NONE, Key::B);
    lumen.click_at(at, Modifiers::NONE);
    lumen.key(Modifiers::NONE, Key::B);
    let drawn = notes_of(&lumen, clip)
        .into_iter()
        .filter(|n| n.pitch() == 38)
        .find(|n| n.start() != q(1) && n.start() != q(3))
        .expect("a drawn snare");
    assert_eq!(drawn.start(), q(1) + Ticks(3 * lumen_core::time::PPQ / 4));
    // It is picked after drawing: Q takes it to the lane's own quarter.
    lumen.key(Modifiers::NONE, Key::Q);
    assert!(
        note_at(&lumen, clip, 38, q(2)).is_some(),
        "on the lane's quarter: {:?}",
        notes_of(&lumen, clip)
    );
    assert_eq!(lumen.app().undo_label(), Some("Quantize note"));
}

#[test]
fn muting_and_soloing_a_lane_are_the_maps_and_one_undo_step_each() {
    let (mut lumen, _) = launch("lanes-mute");
    let mute = lumen.app().drum_toggle_point(36, false).expect("the M");
    lumen.click_at(mute, Modifiers::NONE);
    assert!(map(&lumen).sound(36).expect("kick").mute);
    assert_eq!(lumen.app().undo_label(), Some("Mute drum sound"));
    let solo = lumen.app().drum_toggle_point(38, true).expect("the S");
    lumen.click_at(solo, Modifiers::NONE);
    assert!(map(&lumen).sound(38).expect("snare").solo);
    assert_eq!(lumen.app().undo_label(), Some("Solo drum sound"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!map(&lumen).sound(38).expect("snare").solo);
}

#[test]
fn dragging_a_lanes_drawing_moves_the_sound_in_the_map() {
    let (mut lumen, _) = launch("lanes-reorder");
    let before: Vec<u8> = map(&lumen).sounds.iter().map(|s| s.input).collect();
    let handle = lumen
        .app()
        .drum_handle_point(36)
        .expect("the kick's drawing");
    let target = lumen.app().drum_handle_point(39).expect("the clap's row");
    lumen.drag(handle, target, Modifiers::NONE);
    let after: Vec<u8> = map(&lumen).sounds.iter().map(|s| s.input).collect();
    assert_ne!(before, after, "the map's order changed");
    assert_eq!(lumen.app().undo_label(), Some("Move a drum sound"));
    assert_eq!(after.len(), before.len(), "nothing was lost");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(
        map(&lumen)
            .sounds
            .iter()
            .map(|s| s.input)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn hide_unused_leaves_the_sounds_the_clip_plays() {
    let (mut lumen, _) = launch("lanes-unused");
    assert_eq!(lumen.app().open_drum_lanes().len(), 16);
    palette(&mut lumen, "Hide unused lanes");
    assert!(lumen.app().hides_unused_lanes_shown());
    assert_eq!(
        lumen.app().open_drum_lanes(),
        ["Kick", "Snare", "Closed Hat"],
        "the played sounds, in the map's order"
    );
    palette(&mut lumen, "Hide unused lanes");
    assert_eq!(lumen.app().open_drum_lanes().len(), 16, "every sound again");
}

#[test]
fn the_view_switch_shows_the_piano_roll_and_back() {
    let (mut lumen, _) = launch("lanes-view");
    lumen.click("Keys view");
    lumen.frames(20);
    assert_eq!(lumen.app().drum_view_shown(), Some("keys"));
    assert!(lumen.shows("Note editor: Drums"), "the piano roll");
    palette(&mut lumen, "Show the drum lanes");
    lumen.frames(20);
    assert_eq!(lumen.app().drum_view_shown(), Some("lanes"));
    assert!(lumen.shows("Drum lanes: Drums"));
}

#[test]
fn the_arrangement_draws_a_mapped_clip_as_drum_rows() {
    let (mut lumen, clip) = launch("lanes-arrangement");
    // The lanes keep most of the panel: fold it away to see the arrangement.
    lumen.key(Modifiers::SHIFT, Key::Tab);
    lumen.frames(30);
    let picture = lumen
        .app()
        .arrangement_drum_picture(clip)
        .expect("the clip is in view");
    assert_eq!(picture.0, 3, "one row a sound the clip plays");
    assert!(picture.1 > 0, "its hits are cells: {picture:?}");
    // The same clip with its map taken off draws the mini piano roll again.
    lumen.app_mut().detach_the_drum_map();
    lumen.frames(30);
    assert!(
        lumen.app().arrangement_drum_picture(clip).is_none(),
        "no drum rows without a map"
    );
}

#[test]
fn the_deepest_zoom_keeps_the_drum_picture_narrow_and_the_frame_quick() {
    // Review H1: the arrangement's drum picture is gathered for the visible
    // part only, so a clip tens of thousands of points wide does not turn its
    // hits into walls or gather the whole clip each frame.
    let (mut lumen, clip) = launch("lanes-deep-zoom");
    lumen.key(Modifiers::SHIFT, Key::Tab);
    lumen.frames(30);
    let normal = lumen
        .app()
        .arrangement_drum_picture(clip)
        .expect("the picture at the normal zoom");
    assert!(normal.1 > 0, "cells at the normal zoom: {normal:?}");
    // Deep in, as a player zoomed to a hit would: 4,000 points a beat, so the
    // two-bar clip's body is tens of thousands of points wide.
    lumen.app_mut().set_view_to(0.0, 4000.0);
    lumen.frames(20);
    let deep = lumen
        .app()
        .arrangement_drum_picture(clip)
        .expect("the picture at the deepest zoom");
    assert!(deep.1 > 0, "cells at the deepest zoom: {deep:?}");
    // A handful of frames, each bounded (a debug build: the floor is loose).
    let start = std::time::Instant::now();
    lumen.frames(10);
    let per_frame = start.elapsed().as_secs_f64() * 1000.0 / 10.0;
    assert!(
        per_frame < 1000.0,
        "a frame at the deepest zoom stays quick: {per_frame:.1} ms"
    );
}

#[test]
fn a_press_below_the_last_lane_draws_nothing() {
    // Review M1: no row below the last lane (and none in a map with no lanes).
    let (mut lumen, clip) = launch("lanes-below");
    // Hide unused leaves three lanes, so there is ground below them.
    palette(&mut lumen, "Hide unused lanes");
    lumen.frames(10);
    assert_eq!(lumen.app().open_drum_lanes().len(), 3);
    let body = lumen.app().editor_body().expect("the lanes drew");
    lumen.key(Modifiers::NONE, Key::B);
    let at = Pos2::new(body.left() + 40.0, body.bottom() - 2.0);
    lumen.click_at(at, Modifiers::NONE);
    assert!(
        notes_of(&lumen, clip).len() == 6,
        "nothing was drawn below the lanes: {:?}",
        notes_of(&lumen, clip)
    );
    lumen.key(Modifiers::NONE, Key::B);
}

#[test]
fn a_box_takes_a_hits_diamond_not_its_length() {
    // Review M2: a box in the lanes takes the diamonds (a hit is its diamond),
    // not the note's length bar.
    let (mut lumen, clip) = launch("lanes-box-diamond");
    // A long snare: draw one and drag it two beats long.
    lumen.key(Modifiers::NONE, Key::B);
    let from = hit(&lumen, q(4), 38);
    lumen.drag(
        from,
        from + vec2(lumen.app().editor_view().unwrap().zoom as f32 * 2.0, 0.0),
        Modifiers::NONE,
    );
    lumen.key(Modifiers::NONE, Key::B);
    let long = notes_of(&lumen, clip)
        .into_iter()
        .filter(|n| n.pitch() == 38)
        .max_by_key(|n| n.length())
        .expect("the long snare");
    assert!(long.length() > q(1), "a long hit: {:?}", long.length());
    let diamond = lumen
        .app()
        .drum_hit_point(long.start(), 38)
        .expect("in view");
    // A box over the tail only: nothing.
    let tail = diamond + vec2(lumen.app().editor_view().unwrap().zoom as f32, 0.0);
    lumen.drag(
        tail - vec2(6.0, 6.0),
        tail + vec2(6.0, 6.0),
        Modifiers::NONE,
    );
    assert!(
        lumen.app().editor_picked().is_empty(),
        "the length bar is not a handle"
    );
    // A box over the diamond: the hit.
    lumen.drag(
        diamond - vec2(8.0, 6.0),
        diamond + vec2(8.0, 6.0),
        Modifiers::NONE,
    );
    assert_eq!(lumen.app().editor_picked().len(), 1);
}

#[test]
fn the_picked_lanes_mute_is_a_command_and_the_toggle_takes_space() {
    // Review M4: the mute is a command (keyboard only), and the M toggle
    // activates on Space when focused.
    let (mut lumen, _) = launch("lanes-keys");
    let at = hit(&lumen, q(1), 38);
    lumen.click_at(at, Modifiers::NONE);
    palette(&mut lumen, "Mute the picked lane");
    assert!(map(&lumen).sound(38).expect("snare").mute);
    assert_eq!(lumen.app().undo_label(), Some("Mute drum sound"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!map(&lumen).sound(38).expect("snare").mute);
    // The M under the pointer, reached from the keyboard: Tab to it (the
    // harness focuses the node), then Space toggles it (review M4). A mouse
    // click leaves Space the transport's, so the toggle never eats play/stop.
    let mute = lumen.app().drum_toggle_point(38, false).expect("the M");
    lumen.harness.get_by_label("Mute Snare").focus();
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(
        map(&lumen).sound(38).expect("snare").mute,
        "Space on the focused M mutes it"
    );
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(
        !map(&lumen).sound(38).expect("snare").mute,
        "and Space again unmutes it"
    );
    // A click still toggles it, and leaves the keyboard where it was.
    lumen.click_at(mute, Modifiers::NONE);
    assert!(map(&lumen).sound(38).expect("snare").mute);
}

#[test]
fn the_rolls_row_height_survives_opening_a_drum_clip() {
    // Review M5: a drum clip never writes the roll's row height.
    let (mut lumen, _) = launch("lanes-row-height");
    lumen.app_mut().set_editor_row_height(24.0);
    lumen.frames(3);
    // Reopen the clip: a drum clip frames itself without touching the roll's
    // height.
    let clip = lumen.app().editor_open_clip().expect("the clip");
    lumen.app_mut().close_the_clip_editor();
    lumen.frames(3);
    lumen.app_mut().open_clip_editor(clip);
    lumen.frames(10);
    assert_eq!(lumen.app().editor_row_height(), 24.0);
}

#[test]
fn quantize_says_the_lanes_grid_only_when_a_lane_had_one() {
    // Review L3: the toast does not claim each lane's grid when none had one.
    let (mut lumen, _) = launch("lanes-toast");
    let at = hit(&lumen, q(1), 38);
    lumen.click_at(at, Modifiers::NONE);
    lumen.key(Modifiers::NONE, Key::Q);
    let toast = lumen.app().toast_text().unwrap_or_default().to_owned();
    assert!(toast.starts_with("Quantized"), "{toast}");
    assert!(
        !toast.contains("each lane's own grid"),
        "no lane had a grid: {toast}"
    );
    lumen
        .app_mut()
        .set_lane_quantize(38, Size::Note(4), Feel::Straight);
    lumen.key(Modifiers::NONE, Key::Q);
    let toast = lumen.app().toast_text().unwrap_or_default().to_owned();
    assert!(toast.contains("each lane's own grid"), "{toast}");
}
