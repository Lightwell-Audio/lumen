//! Clips, in the owner's split-clip model: the top half moves (Ctrl
//! copies), the bottom half selects a time range, empty lanes draw a
//! marquee; clicks select; Delete and Esc.

use eframe::egui::{Key, Modifiers, Pos2, Rect, vec2};
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
