//! M7b E0: arrangement tools, cuts, keyboard routing and undo.

use eframe::egui::{Event, Key, Modifiers, Pos2, Rect};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::{Lumen, wav};

fn command(lumen: &mut Lumen, name: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(name.into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    assert!(!lumen.app().palette_open());
}

fn audio(name: &str) -> Lumen {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("take.wav"), 4.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    lumen
}

fn clip_rect(lumen: &Lumen) -> Rect {
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app().clip_rect(&lumen.harness.ctx, id).unwrap()
}

fn spot(lumen: &Lumen, quarter: f64) -> Pos2 {
    let rect = clip_rect(lumen);
    Pos2::new(rect.left() + quarter as f32 * lumen.app().view().zoom as f32, rect.center().y)
}

fn click_at(lumen: &mut Lumen, at: Pos2, modifiers: Modifiers) {
    lumen.harness.hover_at(at);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
        lumen.frames(1);
    }
    lumen.frames(3);
}

#[test]
fn every_arrangement_tool_is_reachable_from_the_palette() {
    let mut lumen = Lumen::start("tools-palette");
    for name in ["Draw", "Erase", "Split", "Mute", "Zoom", "Pointer"] {
        command(&mut lumen, &format!("{name} tool"));
        assert_eq!(lumen.app().arrangement_tool_name(), name);
    }
}

#[test]
fn b_toggles_arrangement_draw_and_escape_returns_to_pointer() {
    let mut lumen = Lumen::start("tools-keys");
    lumen.key(Modifiers::NONE, Key::B);
    assert_eq!(lumen.app().arrangement_tool_name(), "Draw");
    lumen.key(Modifiers::NONE, Key::B);
    assert_eq!(lumen.app().arrangement_tool_name(), "Pointer");
    command(&mut lumen, "Split tool");
    lumen.key(Modifiers::NONE, Key::Escape);
    assert_eq!(lumen.app().arrangement_tool_name(), "Pointer");
}

#[test]
fn split_tool_clicks_cut_on_the_grid_and_undo_restores_the_original() {
    let mut lumen = audio("tools-click");
    let original = lumen.app().project().tracks[0].clips.clone();
    command(&mut lumen, "Split tool");
    let at = spot(&lumen, 2.13);
    click_at(&mut lumen, at, Modifiers::NONE);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start, Ticks::quarters(2));
    assert_eq!(clips[0].start + clips[0].length, clips[1].start);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips, original);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 2);
}

#[test]
fn split_drag_waits_for_release_and_cuts_at_the_aimed_position() {
    let mut lumen = audio("tools-aim");
    command(&mut lumen, "Split tool");
    let from = spot(&lumen, 1.0);
    let to = spot(&lumen, 3.0);
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    lumen.harness.hover_at(to);
    lumen.frames(2);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1, "aiming is a preview");
    lumen.harness.drop_at(to);
    lumen.frames(4);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start, Ticks::quarters(3));
}

#[test]
fn alt_bypasses_snapping_for_a_cut() {
    let mut lumen = audio("tools-free");
    command(&mut lumen, "Split tool");
    let at = spot(&lumen, 2.13);
    click_at(&mut lumen, at, Modifiers::ALT);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert!((clips[1].start.as_quarters() - 2.13).abs() < 0.03);
    assert_ne!(clips[1].start, Ticks::quarters(2));
}

#[test]
fn ctrl_e_cuts_a_selected_clip_at_the_stopped_edit_position() {
    let mut lumen = audio("tools-ctrl-e");
    let at = spot(&lumen, 2.0);
    click_at(&mut lumen, at, Modifiers::NONE);
    lumen.wait_for("the edit position", |app| app.playhead() == Ticks::quarters(2));
    lumen.key(Modifiers::COMMAND, Key::E);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start, Ticks::quarters(2));
}

#[test]
fn ctrl_e_cuts_both_edges_of_a_time_range_as_one_step() {
    let mut lumen = audio("tools-range");
    let original = lumen.app().project().tracks[0].clips.clone();
    let from = spot(&lumen, 1.0);
    let to = spot(&lumen, 3.0);
    lumen.drag(from, to, Modifiers::NONE);
    let (a, b, _) = lumen.app().time_range().expect("a time range");
    lumen.key(Modifiers::COMMAND, Key::E);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 3);
    assert_eq!(clips[1].start, a);
    assert_eq!(clips[2].start, b);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips, original);
}

#[test]
fn mute_and_erase_tools_edit_the_clicked_clip_in_one_step() {
    let mut lumen = audio("tools-mute-erase");
    command(&mut lumen, "Mute tool");
    let at = spot(&lumen, 2.0);
    click_at(&mut lumen, at, Modifiers::NONE);
    assert!(lumen.app().project().tracks[0].clips[0].muted);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!lumen.app().project().tracks[0].clips[0].muted);
    command(&mut lumen, "Erase tool");
    click_at(&mut lumen, at, Modifiers::NONE);
    assert!(lumen.app().project().tracks[0].clips.is_empty());
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
}
