//! Clips, in the owner's split-clip model: the top half moves (Ctrl
//! copies), the bottom half selects a time range, empty lanes draw a
//! marquee; clicks select; Delete and Esc.

use eframe::egui::{Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::{Lumen, wav};

const BAR: i64 = Ticks::quarters(4).0;

/// Lumen with two one-second files dropped in (two tracks, a clip each),
/// on a one-bar grid.
fn with_two_clips(name: &str) -> (Lumen, Id, Id) {
    let mut lumen = Lumen::start(name);
    let kick = wav(&lumen.dir().join("kick.wav"), 1.0);
    let snare = wav(&lumen.dir().join("snare.wav"), 1.0);
    lumen.drop_files(&[kick, snare]);
    lumen.frames(5);
    lumen.click("Grid: 1/8");
    lumen.click("1 bar");
    // The grid menu stays open for more choices: close it. New tracks
    // glide to their height: let them settle.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(40);
    let tracks = &lumen.app().project().tracks;
    let (a, b) = (tracks[0].clips[0].id, tracks[1].clips[0].id);
    (lumen, a, b)
}

fn rect(lumen: &Lumen, clip: Id) -> Rect {
    let ctx = lumen.harness.ctx.clone();
    lumen
        .app()
        .clip_rect(&ctx, clip)
        .expect("the clip is in view")
}

fn top_half(r: Rect) -> Pos2 {
    Pos2::new(r.left() + 12.0, r.top() + 6.0)
}

fn bottom_half(r: Rect) -> Pos2 {
    Pos2::new(r.left() + 12.0, r.bottom() - 6.0)
}

/// `(track index, start)` of `clip`.
fn place(lumen: &Lumen, clip: Id) -> (usize, Ticks) {
    let tracks = &lumen.app().project().tracks;
    tracks
        .iter()
        .enumerate()
        .find_map(|(i, t)| t.clips.iter().find(|c| c.id == clip).map(|c| (i, c.start)))
        .expect("the clip exists")
}

#[test]
fn the_top_half_moves_a_clip_across_tracks_snapped_in_one_step() {
    let (mut lumen, kick, snare) = with_two_clips("clip-move");
    let (_, start) = place(&lumen, kick);
    let zoom = lumen.app().view().zoom as f32;
    // A bar and a bit to the right, and down onto the snare's track.
    let from = top_half(rect(&lumen, kick));
    let to = Pos2::new(from.x + 4.3 * zoom, top_half(rect(&lumen, snare)).y);
    lumen.drag(from, to, Modifiers::NONE);
    let (track, moved) = place(&lumen, kick);
    assert_eq!(track, 1, "on the snare's track");
    assert_eq!(moved.0 % BAR, 0, "snapped to a bar: {moved:?}");
    assert!(moved > start);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    assert!(lumen.app().is_dirty());

    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(place(&lumen, kick), (0, start), "one undo step");
}

#[test]
fn ctrl_dragging_copies_and_selects_the_copy() {
    let (mut lumen, kick, _) = with_two_clips("clip-copy");
    let zoom = lumen.app().view().zoom as f32;
    let from = top_half(rect(&lumen, kick));
    lumen.drag(from, from + vec2(8.0 * zoom, 0.0), Modifiers::COMMAND);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2, "the original stays");
    assert_eq!(clips[0].id, kick);
    assert_eq!(lumen.app().selected_clips(), [clips[1].id]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
}

#[test]
fn a_marquee_selects_clips_and_delete_removes_them() {
    let (mut lumen, kick, snare) = with_two_clips("clip-marquee");
    let (a, b) = (rect(&lumen, kick), rect(&lumen, snare));
    // From empty space right of the clips, back over both.
    let from = Pos2::new(a.right().max(b.right()) + 120.0, a.top() + 2.0);
    let to = Pos2::new(a.left() + 20.0, b.bottom() - 2.0);
    lumen.drag(from, to, Modifiers::NONE);
    let mut selected = lumen.app().selected_clips();
    selected.sort();
    let mut both = vec![kick, snare];
    both.sort();
    assert_eq!(selected, both);

    lumen.key(Modifiers::NONE, Key::Delete);
    assert!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .all(|t| t.clips.is_empty())
    );
    assert_eq!(lumen.app().project().tracks.len(), 2, "the tracks stay");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .all(|t| t.clips.len() == 1)
    );
}

#[test]
fn the_bottom_half_selects_a_time_range_across_tracks() {
    let (mut lumen, kick, snare) = with_two_clips("clip-range");
    let zoom = lumen.app().view().zoom as f32;
    let (_, start) = place(&lumen, kick);
    let from = bottom_half(rect(&lumen, kick));
    let to = Pos2::new(from.x + 4.0 * zoom, bottom_half(rect(&lumen, snare)).y);
    lumen.drag(from, to, Modifiers::NONE);
    let (a, b, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!(b.0 - a.0, BAR, "a bar, snapped");
    assert!(a >= start);
    let ids: Vec<Id> = lumen.app().project().tracks.iter().map(|t| t.id).collect();
    assert_eq!(tracks, ids, "both tracks");
    assert!(
        lumen.app().selected_clips().is_empty(),
        "a range, not clips"
    );
    assert_eq!(place(&lumen, kick).1, start, "nothing moved");
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.app().time_range().is_none());
}

#[test]
fn clicks_select_and_hovering_names_each_half() {
    let (mut lumen, kick, snare) = with_two_clips("clip-click");
    let (a, b) = (rect(&lumen, kick), rect(&lumen, snare));
    lumen.harness.hover_at(top_half(a));
    lumen.frames(2);
    assert_eq!(lumen.app().hint(), Some("drag to move · Ctrl copies"));
    lumen.harness.hover_at(bottom_half(a));
    lumen.frames(2);
    assert_eq!(lumen.app().hint(), Some("drag to select time"));

    let click = |lumen: &mut Lumen, p: Pos2, m: Modifiers| {
        lumen
            .harness
            .event(eframe::egui::Event::ModifiersChanged(m));
        lumen.harness.hover_at(p);
        lumen.harness.drag_at(p);
        lumen.frames(1);
        lumen.harness.drop_at(p);
        lumen.frames(2);
        lumen
            .harness
            .event(eframe::egui::Event::ModifiersChanged(Modifiers::NONE));
        lumen.frames(1);
    };
    click(&mut lumen, top_half(a), Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    click(&mut lumen, bottom_half(b), Modifiers::COMMAND);
    assert_eq!(lumen.app().selected_clips().len(), 2, "Ctrl adds");
    click(&mut lumen, top_half(a), Modifiers::COMMAND);
    assert_eq!(lumen.app().selected_clips(), [snare], "Ctrl toggles");
    click(
        &mut lumen,
        Pos2::new(a.right() + 200.0, a.center().y),
        Modifiers::NONE,
    );
    assert!(
        lumen.app().selected_clips().is_empty(),
        "empty space clears"
    );
}

/// Selects bars `from` to `to` (quarters, snapped) over both tracks by
/// dragging the kick's bottom half down to the snare's.
fn select_range(lumen: &mut Lumen, kick: Id, snare: Id, from: f64, to: f64) {
    let zoom = lumen.app().view().zoom as f32;
    let (a, b) = (rect(lumen, kick), rect(lumen, snare));
    let left = lumen
        .app()
        .clip_rect(&lumen.harness.ctx.clone(), kick)
        .unwrap()
        .left()
        - (place(lumen, kick).1.as_quarters() as f32) * zoom;
    let p = |q: f64, r: Rect| Pos2::new(left + q as f32 * zoom, r.bottom() - 5.0);
    lumen.drag(p(from, a), p(to, b), Modifiers::NONE);
    assert!(lumen.app().time_range().is_some(), "a range");
}

fn starts(lumen: &Lumen, track: usize) -> Vec<(i64, i64)> {
    lumen.app().project().tracks[track]
        .clips
        .iter()
        .map(|c| (c.start.0, c.length.0))
        .collect()
}

#[test]
fn a_range_on_two_tracks_cuts_and_pastes_as_one_step_each() {
    let (mut lumen, kick, snare) = with_two_clips("range-cut");
    // The one-second clips last two quarters from quarter 0.
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)]);
    // Quarters: the one-bar grid, twice finer.
    lumen.key(Modifiers::COMMAND, Key::Num1);
    lumen.key(Modifiers::COMMAND, Key::Num1);
    select_range(&mut lumen, kick, snare, 1.0, 1.9);
    let (a, b, tracks) = lumen.app().time_range().unwrap();
    assert_eq!((a, b), (Ticks::quarters(1), Ticks::quarters(2)));
    assert_eq!(tracks.len(), 2);

    // Cut: both clips end at the range; one undo step brings both back.
    lumen.key(Modifiers::COMMAND, Key::X);
    for track in 0..2 {
        assert_eq!(starts(&lumen, track), [(0, BAR / 4)], "track {track}");
    }
    lumen.key(Modifiers::COMMAND, Key::Z);
    for track in 0..2 {
        assert_eq!(starts(&lumen, track), [(0, BAR / 2)], "track {track}");
    }

    // Copy it, and paste at the playhead (bar 3): both tracks get a piece.
    select_range(&mut lumen, kick, snare, 1.0, 1.9);
    lumen.key(Modifiers::COMMAND, Key::C);
    lumen.key(Modifiers::NONE, Key::Escape);
    locate(&mut lumen, 8.0);
    lumen.key(Modifiers::COMMAND, Key::V);
    for track in 0..2 {
        assert_eq!(
            starts(&lumen, track),
            [(0, BAR / 2), (2 * BAR, BAR / 4)],
            "track {track}"
        );
    }
    let (a, _, tracks) = lumen
        .app()
        .time_range()
        .expect("the pasted range is selected");
    assert_eq!((a, tracks.len()), (Ticks(2 * BAR), 2));
}

/// Right-clicks at `p`.
fn right_click(lumen: &mut Lumen, p: Pos2) {
    lumen.harness.hover_at(p);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(eframe::egui::Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
}

/// Owner, MT-APP-14 run 2: the menu had grown to 33 rows. Its choice A:
/// each right-click gets the menu for what it landed on.
#[test]
fn each_right_click_gets_the_menu_for_what_it_landed_on() {
    let (mut lumen, kick, snare) = with_two_clips("menus");
    let a = rect(&lumen, kick);
    // A clip: clip edits and properties; no grid, no time edits.
    right_click(&mut lumen, top_half(a));
    assert!(lumen.shows("Rename Ctrl+R") && lumen.shows("Cut Ctrl+X"));
    assert!(!lumen.shows("Delete time Ctrl+Shift+Delete"));
    assert!(!lumen.shows("Narrowest"));
    // Esc closes the menu and keeps the selection.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.shows("Rename Ctrl+R"), "closed");
    assert_eq!(lumen.app().selected_clips(), [kick]);
    let away = Pos2::new(a.right() + 300.0, a.bottom() + 120.0);

    // Empty lanes: the grid, in a submenu; Paste only once there is
    // something to paste.
    let empty = Pos2::new(a.right() + 300.0, a.center().y);
    right_click(&mut lumen, empty);
    // The ruler's grid button has the same name: the menu adds a second.
    let grids = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("Grid: 1 bar")
            .count()
    };
    assert_eq!(grids(&lumen), 2);
    assert!(!lumen.shows("Cut Ctrl+X") && !lumen.shows("Paste Ctrl+V"));
    // A click elsewhere closes it.
    lumen.drag(away, away, Modifiers::NONE);
    assert_eq!(grids(&lumen), 1);

    // A time range: its edits, Delete time and Insert time.
    // From a beat in (the left edge would trim), snapped to bar 1.
    select_range(&mut lumen, kick, snare, 1.0, 4.0);
    let inside = Pos2::new(a.center().x, a.bottom() - 5.0);
    right_click(&mut lumen, inside);
    assert!(lumen.shows("Delete time Ctrl+Shift+Delete"));
    assert!(!lumen.shows("Rename Ctrl+R"));
}

/// Owner, MT-APP-14 run 2: a click anywhere on the lanes moves the
/// playhead there (snapped), as in Ableton; on a clip it also selects it.
#[test]
fn a_click_on_the_lanes_moves_the_playhead() {
    let (mut lumen, kick, _) = with_two_clips("click-locate");
    let a = rect(&lumen, kick);
    let zoom = lumen.app().view().zoom as f32;
    // Empty lane, near bar 3 (8 quarters): snapped to it.
    let p = Pos2::new(a.left() + 8.4 * zoom, a.center().y);
    lumen.drag(p, p, Modifiers::NONE);
    lumen.wait_for("the playhead", |app| app.playhead() == Ticks::quarters(8));
    // On a clip: selected, and the playhead goes to its bar.
    lumen.drag(top_half(a), top_half(a), Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    lumen.wait_for("the playhead", |app| app.playhead() == Ticks::ZERO);
    // Ctrl+click only adds to the selection.
    let empty = Pos2::new(a.left() + 4.2 * zoom, a.center().y);
    lumen.drag(empty, empty, Modifiers::COMMAND);
    lumen.frames(10);
    assert_eq!(lumen.app().playhead(), Ticks::ZERO);
}

/// Moves the playhead to `quarters` by clicking the ruler.
fn locate(lumen: &mut Lumen, quarters: f64) {
    let ruler = lumen
        .harness
        .get_by_label("Ruler: click to move the playhead")
        .rect();
    let zoom = lumen.app().view().zoom as f32;
    let at = Pos2::new(ruler.left() + quarters as f32 * zoom, ruler.center().y);
    lumen.drag(at, at, Modifiers::NONE);
    let target = Ticks::quarters(quarters as i64);
    lumen.wait_for("the playhead", |app| app.playhead() == target);
}

#[test]
fn ctrl_d_duplicates_clips_right_after_them() {
    let (mut lumen, kick, _) = with_two_clips("clip-duplicate");
    let p = top_half(rect(&lumen, kick));
    lumen.drag(p, p, Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    lumen.key(Modifiers::COMMAND, Key::D);
    let clips = starts(&lumen, 0);
    assert_eq!(clips, [(0, BAR / 2), (BAR / 2, BAR / 2)], "right after it");
    assert_eq!(starts(&lumen, 1).len(), 1, "the snare is untouched");
    let copy = lumen.app().project().tracks[0].clips[1].id;
    assert_eq!(lumen.app().selected_clips(), [copy], "the copy is selected");
    lumen.key(Modifiers::COMMAND, Key::D);
    assert_eq!(starts(&lumen, 0).len(), 3, "again, after the copy");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(starts(&lumen, 0).len(), 1);
}

#[test]
fn copy_pastes_at_the_playhead_and_delete_time_closes_the_gap() {
    let (mut lumen, kick, snare) = with_two_clips("clip-paste");
    let p = top_half(rect(&lumen, kick));
    lumen.drag(p, p, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::C);
    // The playhead to bar 3 by the ruler, then paste.
    locate(&mut lumen, 8.0);
    lumen.key(Modifiers::COMMAND, Key::V);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2), (2 * BAR, BAR / 2)]);

    // Delete bar 2 as time: a range from the pasted clip back to bar 2 (a
    // range starts on a clip's bottom half), then Ctrl+Shift+Delete. The
    // paste moves back a bar.
    let pasted = lumen.app().project().tracks[0].clips[1].id;
    select_range(&mut lumen, pasted, snare, 8.6, 4.0);
    let (a, b, _) = lumen.app().time_range().unwrap();
    assert_eq!((a, b), (Ticks(BAR), Ticks(2 * BAR)));
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Delete);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2), (BAR, BAR / 2)]);
    assert!(lumen.app().time_range().is_none());
}

#[test]
fn the_lanes_menu_edits_what_was_right_clicked() {
    let (mut lumen, kick, _) = with_two_clips("clip-menu");
    let p = top_half(rect(&lumen, kick));
    lumen.harness.hover_at(p);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(eframe::egui::Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(2);
    assert_eq!(
        lumen.app().selected_clips(),
        [kick],
        "selected by the right-click"
    );
    lumen.click("Duplicate Ctrl+D");
    assert_eq!(starts(&lumen, 0).len(), 2);
}

/// Owner, slice 6b: a range could only start on a clip, so empty bars could
/// not be chosen. Dragging the time ruler selects time on every track.
#[test]
fn dragging_the_time_ruler_selects_empty_bars_on_every_track() {
    let (mut lumen, _, _) = with_two_clips("time-ruler-range");
    let ruler = lumen
        .harness
        .get_by_label("Time ruler: drag to select time on every track")
        .rect();
    let zoom = lumen.app().view().zoom as f32;
    let x = |q: f32| ruler.left() + q * zoom;
    let y = ruler.center().y;
    lumen.drag(Pos2::new(x(8.2), y), Pos2::new(x(16.3), y), Modifiers::NONE);
    let (a, b, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!((a, b), (Ticks(2 * BAR), Ticks(4 * BAR)), "snapped to bars");
    assert_eq!(
        tracks.len(),
        lumen.app().project().tracks.len(),
        "every track"
    );

    // Insert two bars of time there: one step, the clips before it stay.
    lumen.key(Modifiers::COMMAND, Key::I);
    assert_eq!(lumen.app().undo_label(), Some("Insert time"));
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)]);
}

/// The offset of `clip`'s audio in its file.
fn offset(lumen: &Lumen, clip: Id) -> i64 {
    let project = lumen.app().project();
    let found = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == clip);
    match found.map(|c| &c.content) {
        Some(lumen_project::model::ClipContent::Audio { offset, .. }) => *offset,
        _ => panic!("an audio clip"),
    }
}

#[test]
fn an_edge_trims_within_the_file_and_shift_loops_it() {
    let (mut lumen, kick, _) = with_two_clips("clip-trim");
    // Quarters.
    lumen.key(Modifiers::COMMAND, Key::Num1);
    lumen.key(Modifiers::COMMAND, Key::Num1);
    let zoom = lumen.app().view().zoom as f32;
    let r = rect(&lumen, kick);
    let end = Pos2::new(r.right() - 2.0, r.center().y);
    lumen.harness.hover_at(end);
    lumen.frames(2);
    assert_eq!(lumen.app().hint(), Some("drag to trim · Shift loops"));

    // A quarter shorter: one step.
    lumen.drag(end, end - vec2(zoom, 0.0), Modifiers::NONE);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 4)]);
    assert_eq!(lumen.app().undo_label(), Some("Trim clip"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)]);

    // Past the file's end it stops there (the file is one second, two
    // quarters).
    lumen.drag(end, end + vec2(6.0 * zoom, 0.0), Modifiers::NONE);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)]);

    // With Shift it loops: two bars of a two-quarter clip.
    lumen.drag(end, end + vec2(6.0 * zoom, 0.0), Modifiers::SHIFT);
    let clip = &lumen.app().project().tracks[0].clips[0];
    assert_eq!(
        (clip.length, clip.loop_length),
        (Ticks(2 * BAR), Some(Ticks(BAR / 2)))
    );
    assert_eq!(lumen.app().undo_label(), Some("Loop clip"));
}

#[test]
fn a_clip_dropped_on_another_cuts_it_back_as_in_ableton() {
    let (mut lumen, kick, snare) = with_two_clips("clip-overwrite");
    lumen.key(Modifiers::COMMAND, Key::Num1);
    lumen.key(Modifiers::COMMAND, Key::Num1);
    let zoom = lumen.app().view().zoom as f32;
    // The snare onto the kick's track, a quarter in: the kick keeps its
    // first quarter.
    let from = top_half(rect(&lumen, snare));
    let over = Pos2::new(from.x + zoom, top_half(rect(&lumen, kick)).y);
    lumen.drag(from, over, Modifiers::NONE);
    assert_eq!(place(&lumen, snare), (0, Ticks(BAR / 4)));
    assert_eq!(starts(&lumen, 0), [(0, BAR / 4), (BAR / 4, BAR / 2)]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)], "one step");

    // Passing over the kick and dropping elsewhere leaves it whole.
    let from = top_half(rect(&lumen, snare));
    let over = Pos2::new(from.x + zoom, top_half(rect(&lumen, kick)).y);
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for p in [over, over + vec2(zoom, 0.0), from + vec2(12.0 * zoom, 0.0)] {
        lumen.harness.hover_at(p);
        lumen.frames(2);
    }
    lumen.harness.drop_at(from + vec2(12.0 * zoom, 0.0));
    lumen.frames(3);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)], "the kick is whole");
}

#[test]
fn ctrl_alt_slips_the_audio_inside_the_clip() {
    let (mut lumen, kick, _) = with_two_clips("clip-slip");
    // Trim the start in a quarter first, so there is audio to slip back to.
    lumen.key(Modifiers::COMMAND, Key::Num1);
    lumen.key(Modifiers::COMMAND, Key::Num1);
    let zoom = lumen.app().view().zoom as f32;
    let r = rect(&lumen, kick);
    let start = Pos2::new(r.left() + 2.0, r.center().y);
    lumen.drag(start, start + vec2(zoom, 0.0), Modifiers::NONE);
    assert_eq!(
        offset(&lumen, kick),
        24_000,
        "half a second: a quarter at 120 BPM"
    );

    let r = rect(&lumen, kick);
    let p = Pos2::new(r.center().x, top_half(r).y);
    lumen.drag(
        p,
        p + vec2(zoom / 2.0, 0.0),
        Modifiers::COMMAND | Modifiers::ALT,
    );
    assert!(offset(&lumen, kick) < 24_000, "the audio moved later");
    assert_eq!(starts(&lumen, 0), [(BAR / 4, BAR / 4)], "the clip stayed");
    assert_eq!(lumen.app().undo_label(), Some("Slip clip"));
}

/// Owner, after 6c: nothing showed that slipping exists. Holding Ctrl over
/// a clip's top half says so.
#[test]
fn holding_ctrl_over_a_clip_tells_how_to_slip() {
    let (mut lumen, kick, _) = with_two_clips("clip-slip-hint");
    let r = rect(&lumen, kick);
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::COMMAND));
    lumen
        .harness
        .hover_at(Pos2::new(r.center().x, top_half(r).y));
    lumen.frames(2);
    assert_eq!(
        lumen.app().hint(),
        Some("drag to copy · add Alt to slip the audio")
    );
}

/// Owner, after 6c: zooming all the way into a long clip crashed ("memory
/// allocation of 68719476736 bytes failed"): the hover's dashed split line
/// ran the clip's whole width, hundreds of millions of points deep in.
/// Drawing is now cut to the view, so deep frames stay quick.
#[test]
fn hovering_a_long_clip_at_the_deepest_zoom_stays_quick() {
    let mut lumen = Lumen::start("deep-long-clip");
    let file = wav(&lumen.dir().join("long.wav"), 60.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    let r = rect(&lumen, clip);
    let p = Pos2::new(r.left() + r.width().min(600.0) * 0.5, top_half(r).y);
    lumen.harness.hover_at(p);
    // About a second in all with the fix; nearly a minute without it.
    let started = std::time::Instant::now();
    for _ in 0..80 {
        lumen.key(Modifiers::NONE, Key::Equals);
    }
    assert!(lumen.app().view().zoom > 100_000.0, "single samples");
    lumen.harness.hover_at(p + vec2(1.0, 0.0));
    lumen.frames(3);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "zooming in took {:?}",
        started.elapsed()
    );
    assert!(lumen.app().hint().is_some(), "hovering the clip");
}

/// Owner, after 6c: dragging a clip while it played crackled. Each new
/// position made the audio under the playhead jump to another point of the
/// file, once per screen frame. Moved clips now crossfade in the engine.
#[test]
fn dragging_a_playing_clip_does_not_click() {
    let mut lumen = Lumen::start("drag-while-playing");
    let file = wav(&lumen.dir().join("tone.wav"), 8.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let clip = lumen.app().project().tracks[0].clips[0].id;
    let (_, start) = place(&lumen, clip);
    lumen.key(Modifiers::NONE, Key::Space);
    // Well inside the clip, however far the drag below moves it.
    lumen.wait_for("the playhead to pass beat 2", |app| {
        app.playhead() > Ticks::quarters(2)
    });
    lumen.app_mut().heard();

    // Back and forth, off the grid (Alt), a step per screen frame.
    let zoom = lumen.app().view().zoom as f32;
    let from = top_half(rect(&lumen, clip));
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::ALT));
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 0..24 {
        let beats = [0.3, 0.6, 0.9, 0.6][step % 4];
        lumen.harness.hover_at(from + vec2(beats * zoom, 0.0));
        lumen.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    lumen.harness.drop_at(from + vec2(0.3 * zoom, 0.0));
    lumen.frames(3);
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::NONE));
    std::thread::sleep(std::time::Duration::from_millis(50));
    lumen.frames(1);
    let heard = lumen.app_mut().heard();
    assert!(lumen.app().is_playing());
    assert!(place(&lumen, clip).1 > start, "the clip moved");

    // The test tone is 0.09 loud and moves at most 0.0046 a frame; a jump
    // to another point of it steps by up to 0.18.
    assert!(heard.len() > 2_000, "heard {} frames", heard.len());
    let loudest = heard.iter().map(|s| s.0.abs()).fold(0.0, f32::max);
    assert!(loudest > 0.05, "the clip played: {loudest}");
    let steepest = heard
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).abs().max((w[1].1 - w[0].1).abs()))
        .fold(0.0, f32::max);
    assert!(steepest < 0.006, "a click: a step of {steepest}");
}
