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
    Pos2::new(
        rect.left() + quarter as f32 * lumen.app().view().zoom as f32,
        rect.bottom() - 6.0,
    )
}

fn click_at(lumen: &mut Lumen, at: Pos2, modifiers: Modifiers) {
    lumen.harness.event(Event::ModifiersChanged(modifiers));
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
    lumen
        .harness
        .event(Event::ModifiersChanged(Modifiers::NONE));
    lumen.frames(1);
}

#[test]
fn every_arrangement_tool_is_reachable_from_the_palette() {
    let mut lumen = Lumen::start("tools-palette");
    for name in ["Draw", "Erase", "Split", "Mute", "Zoom", "Pointer"] {
        let label = if name == "Draw" {
            "Arrangement draw tool".to_owned()
        } else {
            format!("{name} tool")
        };
        command(&mut lumen, &label);
        assert_eq!(lumen.app().arrangement_tool_name(), name);
    }
}

#[test]
fn the_tool_switch_opens_its_menu_and_selects_a_tool_with_the_mouse() {
    let mut lumen = Lumen::start("tools-switch");
    assert!(!lumen.shows("Erase"), "the menu is closed to begin with");
    lumen.click("Arrangement tool: Pointer");
    assert!(lumen.shows("Split"), "the mouse opens the tool menu");
    lumen.click("Split");
    assert_eq!(lumen.app().arrangement_tool_name(), "Split");
    assert!(lumen.shows("Arrangement tool: Split"));
    lumen.frames(30);
    assert!(
        !lumen.shows("Erase"),
        "choosing a tool closes the menu, so a pick is one click"
    );
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
    assert_eq!(
        lumen.app().project().tracks[0].clips.len(),
        1,
        "aiming is a preview"
    );
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
    assert!(
        (clips[1].start.as_quarters() - 2.13).abs() < 0.05,
        "{:?}",
        clips[1].start
    );
    assert_ne!(clips[1].start, Ticks::quarters(2));
}

#[test]
fn ctrl_e_cuts_a_selected_clip_at_the_stopped_edit_position() {
    let mut lumen = audio("tools-ctrl-e");
    let at = spot(&lumen, 2.0);
    click_at(&mut lumen, at, Modifiers::NONE);
    lumen.wait_for("the edit position", |app| {
        app.playhead() == Ticks::quarters(2)
    });
    lumen.key(Modifiers::COMMAND, Key::E);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].start, Ticks::quarters(2));
}

#[test]
fn ctrl_e_cuts_at_the_moving_playhead_and_keeps_playing() {
    let mut lumen = audio("tools-playing");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback inside the clip", |app| {
        app.playhead() > Ticks::quarters(1)
    });
    lumen.key(Modifiers::COMMAND, Key::E);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    let cut = clips[1].start;
    assert!(cut > Ticks::ZERO && cut < Ticks::quarters(8));
    assert!(lumen.app().is_playing());
    lumen.wait_for("playback beyond the cut", |app| app.playhead() > cut);
    assert!(lumen.app().is_playing());
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

#[test]
fn escape_cancels_a_cut_before_the_pointer_is_released() {
    let mut lumen = audio("tools-cancel");
    command(&mut lumen, "Split tool");
    let from = spot(&lumen, 1.0);
    let to = spot(&lumen, 3.0);
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    lumen.harness.hover_at(to);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.harness.drop_at(to);
    lumen.frames(3);
    assert_eq!(lumen.app().project().tracks[0].clips.len(), 1);
    assert_eq!(lumen.app().arrangement_tool_name(), "Pointer");
}

#[test]
fn zoom_tool_fits_a_drag_and_x_restores_the_view() {
    let mut lumen = audio("tools-zoom");
    command(&mut lumen, "Zoom tool");
    let zoom = lumen.app().view().zoom;
    let from = spot(&lumen, 1.0);
    let to = spot(&lumen, 3.0);
    lumen.drag(from, to, Modifiers::NONE);
    lumen.frames(40);
    assert!(lumen.app().view().zoom > zoom);
    lumen.key(Modifiers::NONE, Key::X);
    lumen.frames(40);
    assert!((lumen.app().view().zoom - zoom).abs() < 0.01);
}

#[test]
fn split_loop_phase_survives_save_and_reopen() {
    let mut lumen = audio("tools-save-loop");
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen
        .app_mut()
        .trim_clip_end_to(id, Ticks::quarters(16), true);
    lumen.frames(40);
    command(&mut lumen, "Split tool");
    let at = spot(&lumen, 3.0);
    click_at(&mut lumen, at, Modifiers::NONE);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].loop_phase, Ticks::quarters(3));
    let expected = clips.clone();
    let path = lumen_app::testing::project_file(lumen.dir(), "Split song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the split to save", |app| !app.is_busy() && !app.is_dirty());
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path));
    again.wait_for("the split to reopen", |app| !app.is_busy());
    assert_eq!(again.app().project().tracks[0].clips, expected);
    assert_eq!(again.app().arrangement_tool_name(), "Pointer");
}

#[test]
fn a_split_keeps_exact_source_time_through_save_and_reopen() {
    let mut lumen = audio("tools-save-source-clock");
    command(&mut lumen, "Split tool");
    let at = spot(&lumen, 3.0);
    click_at(&mut lumen, at, Modifiers::NONE);
    let clips = &lumen.app().project().tracks[0].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].audio_phase_seconds, 1.5);
    let expected = clips.clone();
    let path = lumen_app::testing::project_file(lumen.dir(), "Source clock");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the source clock to save", |app| {
        !app.is_busy() && !app.is_dirty()
    });
    let dir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path));
    again.wait_for("the source clock to reopen", |app| !app.is_busy());
    assert_eq!(again.app().project().tracks[0].clips, expected);
}

#[test]
fn locked_clips_ignore_the_split_erase_and_mute_tools() {
    let mut lumen = audio("tools-locked");
    let at = spot(&lumen, 2.0);
    lumen.right_click(at);
    lumen.click("Lock clip");
    let before = lumen.app().project().tracks[0].clips.clone();
    assert!(before[0].locked);
    for tool in ["Split tool", "Erase tool", "Mute tool"] {
        command(&mut lumen, tool);
        click_at(&mut lumen, at, Modifiers::NONE);
        assert_eq!(lumen.app().project().tracks[0].clips, before, "{tool}");
    }
}

#[test]
fn draw_tool_creates_a_midi_clip_on_an_empty_midi_lane_in_one_step() {
    let mut lumen = audio("tools-draw");
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    lumen.frames(40);
    let name = lumen.track_names()[1].clone();
    let header = lumen
        .harness
        .get_by_label_contains(&format!("Track {name},"))
        .rect();
    let at = Pos2::new(spot(&lumen, 2.0).x, header.center().y);
    command(&mut lumen, "Arrangement draw tool");
    click_at(&mut lumen, at, Modifiers::NONE);
    assert_eq!(lumen.app().project().tracks[1].clips.len(), 1);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(lumen.app().project().tracks[1].clips.is_empty());
}
