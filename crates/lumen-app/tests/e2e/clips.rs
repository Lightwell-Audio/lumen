//! Clips: the header moves (Ctrl
//! copies), the body selects a time range, a drag in empty lanes
//! selects time on the tracks it spans and Ctrl+Shift draws a marquee
//! (ADR-0049); clicks select; Delete and Esc.

use eframe::egui::{Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::{Lumen, keys, wav};

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
    // In the order they were dropped, whichever loads first.
    assert_eq!(lumen.track_names(), ["kick", "snare"]);
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

fn header(r: Rect) -> Pos2 {
    Pos2::new(r.left() + 12.0, r.top() + 6.0)
}

fn body(r: Rect) -> Pos2 {
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
fn the_header_moves_a_clip_across_tracks_snapped_in_one_step() {
    let (mut lumen, kick, snare) = with_two_clips("clip-move");
    let (_, start) = place(&lumen, kick);
    let zoom = lumen.app().view().zoom as f32;
    // A bar and a bit to the right, and down onto the snare's track.
    let from = header(rect(&lumen, kick));
    let to = Pos2::new(from.x + 4.3 * zoom, header(rect(&lumen, snare)).y);
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
    let from = header(rect(&lumen, kick));
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
    // From empty space right of the clips, back over both. Ctrl+Shift draws
    // the marquee; a plain drag there selects time (ADR-0049).
    let from = Pos2::new(a.right().max(b.right()) + 120.0, a.top() + 2.0);
    let to = Pos2::new(a.left() + 20.0, b.bottom() - 2.0);
    lumen.drag(from, to, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(lumen.app().time_range().is_none(), "a marquee, not a range");
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
fn the_body_selects_a_time_range_across_tracks() {
    let (mut lumen, kick, snare) = with_two_clips("clip-range");
    let zoom = lumen.app().view().zoom as f32;
    let (_, start) = place(&lumen, kick);
    let from = body(rect(&lumen, kick));
    let to = Pos2::new(from.x + 4.0 * zoom, body(rect(&lumen, snare)).y);
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

    // Body space shares the empty lane's explicit clip marquee gesture.
    let to = Pos2::new(
        rect(&lumen, snare).right() + 20.0,
        body(rect(&lumen, snare)).y,
    );
    lumen.drag(from, to, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(lumen.app().time_range().is_none());
    let mut selected = lumen.app().selected_clips();
    selected.sort();
    let mut both = vec![kick, snare];
    both.sort();
    assert_eq!(selected, both);
    assert_eq!(place(&lumen, kick).1, start);
}

#[test]
fn clicks_select_and_hovering_names_the_header_and_body() {
    let (mut lumen, kick, snare) = with_two_clips("clip-click");
    let (a, b) = (rect(&lumen, kick), rect(&lumen, snare));
    lumen.harness.hover_at(header(a));
    lumen.frames(2);
    // M4 slice 0: in the info line, not by the pointer.
    assert!(
        lumen.info().starts_with("Clip “kick”: drag to move"),
        "{}",
        lumen.info()
    );
    assert_eq!(lumen.lit().as_deref(), Some("drag to move"));
    assert!(lumen.app().hint().is_none(), "nothing floats over the clip");
    lumen.harness.hover_at(body(a));
    lumen.frames(2);
    assert!(
        lumen
            .info()
            .starts_with("Clip “kick”, body: drag to select time"),
        "{}",
        lumen.info()
    );

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
    click(&mut lumen, header(a), Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    click(&mut lumen, body(b), Modifiers::COMMAND);
    assert_eq!(lumen.app().selected_clips().len(), 2, "Ctrl adds");
    click(&mut lumen, header(a), Modifiers::COMMAND);
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
/// dragging the kick's body down to the snare's.
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

/// Owner, MT-APP-14 run 2: the menu had grown to 33 rows. Its choice A:
/// each right-click gets the menu for what it landed on.
#[test]
fn each_right_click_gets_the_menu_for_what_it_landed_on() {
    let (mut lumen, kick, snare) = with_two_clips("menus");
    let a = rect(&lumen, kick);
    // A clip: clip edits and properties; no grid, no time edits.
    lumen.right_click(header(a));
    assert!(lumen.shows(&keys("Rename Ctrl+R")) && lumen.shows(&keys("Cut Ctrl+X")));
    assert!(!lumen.shows(&keys("Delete time Ctrl+Shift+Delete")));
    assert!(!lumen.shows("Narrowest"));
    // Esc closes the menu and keeps the selection.
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.shows(&keys("Rename Ctrl+R")), "closed");
    assert_eq!(lumen.app().selected_clips(), [kick]);
    let away = Pos2::new(a.right() + 300.0, a.bottom() + 120.0);

    // Empty lanes: the grid, in a submenu; Paste only once there is
    // something to paste.
    let empty = Pos2::new(a.right() + 300.0, a.center().y);
    lumen.right_click(empty);
    // The ruler's grid button has the same name: the menu adds a second.
    let grids = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("Grid: 1 bar")
            .count()
    };
    assert_eq!(grids(&lumen), 2);
    assert!(!lumen.shows(&keys("Cut Ctrl+X")) && !lumen.shows(&keys("Paste Ctrl+V")));
    // A click elsewhere closes it.
    lumen.drag(away, away, Modifiers::NONE);
    assert_eq!(grids(&lumen), 1);

    // A time range: its edits, Delete time and Insert time.
    // From a beat in (the left edge would trim), snapped to bar 1.
    select_range(&mut lumen, kick, snare, 1.0, 4.0);
    let inside = Pos2::new(a.center().x, a.bottom() - 5.0);
    lumen.right_click(inside);
    assert!(lumen.shows(&keys("Delete time Ctrl+Shift+Delete")));
    assert!(!lumen.shows(&keys("Rename Ctrl+R")));
}

/// Owner, MT-APP-14 run 2: a click anywhere on the lanes moves the
/// playhead there while stopped (snapped); on a clip it also selects it.
#[test]
fn a_click_on_the_lanes_moves_the_playhead_while_stopped() {
    let (mut lumen, kick, _) = with_two_clips("click-locate");
    let a = rect(&lumen, kick);
    let zoom = lumen.app().view().zoom as f32;
    // Empty lane, near bar 3 (8 quarters): snapped to it.
    let p = Pos2::new(a.left() + 8.4 * zoom, a.center().y);
    lumen.drag(p, p, Modifiers::NONE);
    lumen.wait_for("the playhead", |app| app.playhead() == Ticks::quarters(8));
    // On a clip: selected, and the playhead goes to its bar.
    lumen.drag(header(a), header(a), Modifiers::NONE);
    assert_eq!(lumen.app().selected_clips(), [kick]);
    lumen.wait_for("the playhead", |app| app.playhead() == Ticks::ZERO);
    // Ctrl+click only adds to the selection.
    let empty = Pos2::new(a.left() + 4.2 * zoom, a.center().y);
    lumen.drag(empty, empty, Modifiers::COMMAND);
    lumen.frames(10);
    assert_eq!(lumen.app().playhead(), Ticks::ZERO);
}

/// Owner, M5: a click on a clip's header while stopped puts the playhead
/// at the clip's start, wherever along the header it lands; a click on
/// its body still locates to the clicked place.
#[test]
fn a_click_on_a_clip_header_moves_the_playhead_to_its_start_while_stopped() {
    let (mut lumen, kick, _) = with_two_clips("header-click-locate");
    let zoom = lumen.app().view().zoom as f32;
    let from = header(rect(&lumen, kick));
    lumen.drag(
        from,
        Pos2::new(from.x + 4.3 * zoom, from.y),
        Modifiers::NONE,
    );
    let (_, start) = place(&lumen, kick);
    assert_eq!(start, Ticks::quarters(4));
    let a = rect(&lumen, kick);
    // Alt places freely, so a click a quarter in would land there.
    let p = Pos2::new(a.left() + 1.0 * zoom, a.top() + 6.0);
    lumen.drag(p, p, Modifiers::ALT);
    lumen.wait_for("the playhead", |app| app.playhead() == start);
    // The body still locates where it is clicked.
    let b = Pos2::new(a.left() + 1.0 * zoom, a.bottom() - 6.0);
    lumen.drag(b, b, Modifiers::ALT);
    let near = |t: Ticks| (t.0 - Ticks::quarters(5).0).abs() < Ticks::quarters(1).0 / 8;
    lumen.wait_for("the playhead", |app| near(app.playhead()));
}

#[test]
fn playing_clip_and_lane_clicks_select_without_seeking() {
    let (mut lumen, kick, snare) = with_two_clips("playing-click-select");
    // Keep the clicked positions in view as playback advances.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback past the clips", |app| {
        app.is_playing() && app.playhead() > Ticks::quarters(4)
    });

    for point in [header(rect(&lumen, kick)), body(rect(&lumen, snare))] {
        let before = lumen.app().playhead();
        lumen.drag(point, point, Modifiers::NONE);
        // Give the audio thread a block to hear a locate, if one was sent.
        lumen.listen(std::time::Duration::ZERO, 256);
        assert!(lumen.app().is_playing());
        assert!(
            lumen.app().playhead() >= before,
            "a clip click must not rewind"
        );
    }
    assert_eq!(lumen.app().selected_clips(), [snare]);

    let point = header(rect(&lumen, kick));
    lumen.drag(point, point, Modifiers::COMMAND);
    let mut selected = lumen.app().selected_clips();
    selected.sort();
    let mut both = vec![kick, snare];
    both.sort();
    assert_eq!(selected, both, "Ctrl+click still builds the selection");

    let a = rect(&lumen, kick);
    let zoom = lumen.app().view().zoom as f32;
    let empty = Pos2::new(a.left() + 12.4 * zoom, a.center().y);
    lumen.drag(empty, empty, Modifiers::NONE);
    lumen.listen(std::time::Duration::ZERO, 256);
    assert!(lumen.app().selected_clips().is_empty());
    assert!(lumen.app().is_playing());
    assert!(
        lumen.app().playhead() < Ticks::quarters(12),
        "an empty lane click must not jump forward"
    );
}

#[test]
fn playing_ruler_and_scrub_clicks_still_seek_and_keep_clip_selection() {
    let (mut lumen, kick, _) = with_two_clips("playing-click-seek");
    let point = header(rect(&lumen, kick));
    lumen.drag(point, point, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());

    for (label, quarters) in [
        ("Ruler: click to move the playhead", 8),
        ("Scrub strip: click to jump, drag to listen", 16),
    ] {
        let zone = lumen.harness.get_by_label(label).rect();
        let zoom = lumen.app().view().zoom as f32;
        let point = Pos2::new(zone.left() + quarters as f32 * zoom, zone.center().y);
        lumen.drag(point, point, Modifiers::NONE);
        let target = Ticks::quarters(quarters);
        lumen.wait_for("the playing seek", |app| app.playhead() >= target);
        assert!(lumen.app().playhead() < target + Ticks::quarters(1));
        assert!(lumen.app().is_playing());
        assert_eq!(lumen.app().selected_clips(), [kick]);
    }
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
    let p = header(rect(&lumen, kick));
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
    let p = header(rect(&lumen, kick));
    lumen.drag(p, p, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::C);
    // The playhead to bar 3 by the ruler, then paste.
    locate(&mut lumen, 8.0);
    lumen.key(Modifiers::COMMAND, Key::V);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2), (2 * BAR, BAR / 2)]);

    // Delete bar 2 as time: a range from the pasted clip back to bar 2 (a
    // range starts on a clip's body), then Ctrl+Shift+Delete. The
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
    let p = header(rect(&lumen, kick));
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
    lumen.click(&keys("Duplicate Ctrl+D"));
    assert_eq!(starts(&lumen, 0).len(), 2);
}

/// ADR-0049 (owner, MT-MIDI-4): a drag in empty lane space selects time on
/// the tracks it spans, snapped like a clip's body; the clips stay.
#[test]
fn a_drag_in_empty_lane_space_selects_time_on_the_tracks_it_spans() {
    let (mut lumen, kick, snare) = with_two_clips("lane-range");
    let zoom = lumen.app().view().zoom as f32;
    let (a, b) = (rect(&lumen, kick), rect(&lumen, snare));
    // Bars 3 to 5 (quarters 8.2 to 16.3, snapped), from the kick's lane
    // down to the snare's: empty space on both.
    let x = |q: f32| a.left() + q * zoom;
    lumen.drag(
        Pos2::new(x(8.2), a.center().y),
        Pos2::new(x(16.3), b.center().y),
        Modifiers::NONE,
    );
    let (start, end, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!((start, end), (Ticks(2 * BAR), Ticks(4 * BAR)), "snapped");
    let ids: Vec<Id> = lumen.app().project().tracks.iter().map(|t| t.id).collect();
    assert_eq!(tracks, ids, "both tracks, as the drag spans them");
    assert_eq!(lumen.app().selected_tracks(), ids, "the range's tracks");
    assert!(lumen.app().selected_clips().is_empty());
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)], "nothing moved");
    // The range stays with its selection on later frames.
    lumen.frames(3);
    assert!(lumen.app().time_range().is_some(), "the range stays");
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.app().time_range().is_none());
}

#[test]
fn a_horizontal_drag_in_one_lane_selects_only_that_track() {
    let (mut lumen, kick, _) = with_two_clips("lane-range-one");
    let zoom = lumen.app().view().zoom as f32;
    let a = rect(&lumen, kick);
    let (from, to) = (
        Pos2::new(a.left() + 8.2 * zoom, a.center().y),
        Pos2::new(a.left() + 16.3 * zoom, a.center().y),
    );
    lumen.drag(from, to, Modifiers::NONE);
    let kick_track = lumen.app().project().tracks[0].id;
    let (_, _, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!(tracks, [kick_track]);
    assert_eq!(lumen.app().selected_tracks(), [kick_track]);
}

/// ADR-0049: the ruler's range is on the selected tracks, and the selection
/// is kept; with none selected it is on every track (the test below).
#[test]
fn the_ruler_drag_selects_time_on_the_selected_tracks_only() {
    let (mut lumen, _, _) = with_two_clips("ruler-selected");
    let snare = lumen.app().project().tracks[1].id;
    lumen.harness.get_by_label_contains("Track snare,").click();
    lumen.frames(3);
    assert_eq!(lumen.app().selected_tracks(), [snare]);
    let ruler = lumen
        .harness
        .get_by_label("Time ruler: drag to select time on the selected tracks, or every track")
        .rect();
    let zoom = lumen.app().view().zoom as f32;
    let x = |q: f32| ruler.left() + q * zoom;
    let y = ruler.center().y;
    lumen.drag(Pos2::new(x(8.2), y), Pos2::new(x(16.3), y), Modifiers::NONE);
    let (start, end, tracks) = lumen.app().time_range().expect("a range");
    assert_eq!((start, end), (Ticks(2 * BAR), Ticks(4 * BAR)));
    assert_eq!(tracks, [snare], "only the selected track");
    assert_eq!(
        lumen.app().selected_tracks(),
        [snare],
        "the selection stays"
    );
}

/// Owner, slice 6b: a range could only start on a clip, so empty bars could
/// not be chosen. Dragging the time ruler selects time on every track.
#[test]
fn dragging_the_time_ruler_selects_empty_bars_on_every_track() {
    let (mut lumen, _, _) = with_two_clips("time-ruler-range");
    let ruler = lumen
        .harness
        .get_by_label("Time ruler: drag to select time on the selected tracks, or every track")
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
/// Where in the file the clip's first frame plays: the integer anchor plus
/// the exact source time a trim or split has advanced it by.
fn offset(lumen: &Lumen, clip: Id) -> i64 {
    let project = lumen.app().project();
    let found = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == clip)
        .expect("the clip");
    match &found.content {
        lumen_project::model::ClipContent::Audio { offset, .. } => {
            *offset + (found.audio_phase_seconds * f64::from(project.sample_rate)).round() as i64
        }
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
    assert_eq!(
        lumen.info(),
        "End of “kick”: drag to trim · Shift+drag to loop"
    );

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
fn a_clip_dropped_on_another_cuts_it_back() {
    let (mut lumen, kick, snare) = with_two_clips("clip-overwrite");
    lumen.key(Modifiers::COMMAND, Key::Num1);
    lumen.key(Modifiers::COMMAND, Key::Num1);
    let zoom = lumen.app().view().zoom as f32;
    // The snare onto the kick's track, a quarter in: the kick keeps its
    // first quarter.
    let from = header(rect(&lumen, snare));
    let over = Pos2::new(from.x + zoom, header(rect(&lumen, kick)).y);
    lumen.drag(from, over, Modifiers::NONE);
    assert_eq!(place(&lumen, snare), (0, Ticks(BAR / 4)));
    assert_eq!(starts(&lumen, 0), [(0, BAR / 4), (BAR / 4, BAR / 2)]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)], "one step");

    // Passing over the kick and dropping elsewhere leaves it whole.
    let from = header(rect(&lumen, snare));
    let over = Pos2::new(from.x + zoom, header(rect(&lumen, kick)).y);
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
    let p = Pos2::new(r.center().x, header(r).y);
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
/// a clip's header says so.
#[test]
fn holding_ctrl_over_a_clip_tells_how_to_slip() {
    let (mut lumen, kick, _) = with_two_clips("clip-slip-hint");
    let r = rect(&lumen, kick);
    lumen
        .harness
        .event(eframe::egui::Event::ModifiersChanged(Modifiers::COMMAND));
    lumen.harness.hover_at(Pos2::new(r.center().x, header(r).y));
    lumen.frames(2);
    assert_eq!(lumen.lit(), Some(keys("Ctrl+drag to copy")));
    lumen.harness.event(eframe::egui::Event::ModifiersChanged(
        Modifiers::COMMAND | Modifiers::ALT,
    ));
    lumen.frames(2);
    assert_eq!(lumen.lit(), Some(keys("Ctrl+Alt+drag to slip the audio")));
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
    let p = Pos2::new(r.left() + r.width().min(600.0) * 0.5, header(r).y);
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
    assert!(lumen.app().info_line().is_some(), "hovering the clip");
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
    let from = header(rect(&lumen, clip));
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

/// Owner, after slice 7: a clip copied, then pasted onto another track
/// (with keys, or from the lanes' menu) did not land there; and Ctrl+C and
/// Ctrl+V did nothing at all in the real window (`Lumen::key` sends them
/// as the window does).
#[test]
fn a_copied_clip_pastes_onto_the_track_clicked() {
    let (mut lumen, kick, snare) = with_two_clips("paste-other-track");
    let p = header(rect(&lumen, kick));
    lumen.drag(p, p, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::C);
    // A click on the snare's empty lane at bar 3 (two bars right of the
    // clips' start), then Ctrl+V.
    let zoom = lumen.app().view().zoom as f32;
    let s = rect(&lumen, snare);
    let spot = Pos2::new(s.left() + 8.0 * zoom + 2.0, s.center().y);
    lumen.drag(spot, spot, Modifiers::NONE);
    lumen.key(Modifiers::COMMAND, Key::V);
    assert_eq!(
        starts(&lumen, 0),
        [(0, BAR / 2)],
        "the kick's track as it was"
    );
    assert_eq!(starts(&lumen, 1), [(0, BAR / 2), (2 * BAR, BAR / 2)]);

    // The lanes' menu: a right-click on the snare's lane at bar 5, Paste.
    let spot = Pos2::new(s.left() + 16.0 * zoom + 2.0, s.center().y);
    lumen.right_click(spot);
    lumen.click(&keys("Paste Ctrl+V"));
    assert_eq!(starts(&lumen, 0), [(0, BAR / 2)]);
    assert_eq!(
        starts(&lumen, 1),
        [(0, BAR / 2), (2 * BAR, BAR / 2), (4 * BAR, BAR / 2)]
    );

    // Ctrl+V with text on the system clipboard (the window sends a Paste
    // event then): the clip again, on the snare's track at bar 7.
    let spot = Pos2::new(s.left() + 24.0 * zoom + 2.0, s.center().y);
    lumen.drag(spot, spot, Modifiers::NONE);
    lumen.paste_with_text_on_the_clipboard("some text from elsewhere");
    assert_eq!(starts(&lumen, 1).len(), 4);
    assert_eq!(starts(&lumen, 1)[3], (6 * BAR, BAR / 2));
    assert_eq!(starts(&lumen, 0).len(), 1);
}
