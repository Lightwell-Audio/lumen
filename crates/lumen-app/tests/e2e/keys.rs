//! The keys and menus slice 7d added: Z zooms to the selection, Ctrl+A
//! selects every clip, Ctrl+L loops the selection, Fit loop; F6 moves the
//! keyboard between panes; Shift+Tab toggles the detail panel except while
//! typing; menus and the ruler show shortcuts from the keymap.

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;

use super::properties::{select, with_two_clips};
use super::{Lumen, keys};

/// Right-clicks the control named `label`.
fn right_click(lumen: &mut Lumen, label: &str) {
    let p: Pos2 = lumen.harness.get_by_label(label).rect().center();
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
}

fn clip_width(lumen: &Lumen, index: usize) -> f32 {
    let ctx = lumen.harness.ctx.clone();
    let id = lumen.app().project().tracks[index].clips[0].id;
    lumen.app().clip_rect(&ctx, id).expect("in view").width()
}

#[test]
fn z_zooms_to_the_selection_and_x_goes_back() {
    let (mut lumen, kick, _) = with_two_clips("keys-zoom");
    let at = lumen
        .app()
        .project()
        .tracks
        .iter()
        .position(|t| t.clips[0].id == kick)
        .expect("the kick's track");
    let before = clip_width(&lumen, at);
    select(&mut lumen, kick);
    lumen.key(Modifiers::NONE, Key::Z);
    lumen.frames(30);
    let zoomed = clip_width(&lumen, at);
    assert!(
        zoomed > before * 2.0,
        "the clip fills the view: {before} to {zoomed}"
    );
    lumen.key(Modifiers::NONE, Key::X);
    lumen.frames(30);
    assert!((clip_width(&lumen, at) - before).abs() < 1.0, "X goes back");
}

#[test]
fn ctrl_a_selects_every_clip_and_ctrl_l_loops_them() {
    let (mut lumen, kick, snare) = with_two_clips("keys-loop");
    lumen.key(Modifiers::COMMAND, Key::A);
    let mut selected = lumen.app().selected_clips();
    selected.sort();
    let mut both = vec![kick, snare];
    both.sort();
    assert_eq!(selected, both);

    lumen.key(Modifiers::COMMAND, Key::L);
    let transport = lumen.app().project().transport;
    let end = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.start + c.length)
        .max()
        .expect("clips");
    assert_eq!(
        (transport.loop_start, transport.loop_end),
        (Ticks::ZERO, end)
    );
    assert!(transport.loop_enabled);
    assert_eq!(lumen.app().undo_label(), Some("Loop selection"));

    // Fit loop, from the ruler's menu, which lists the view's commands
    // with their keys.
    lumen.key(Modifiers::NONE, Key::Escape);
    let before = lumen.app().view().zoom;
    right_click(&mut lumen, "Ruler: click to move the playhead");
    assert!(lumen.shows("Zoom to selection Z"));
    lumen.click("Fit loop");
    lumen.frames(30);
    assert_ne!(lumen.app().view().zoom, before, "the loop fills the view");
}

#[test]
fn f6_moves_the_keyboard_between_panes() {
    let mut lumen = Lumen::start("keys-panes");
    let pane = |lumen: &Lumen| {
        let ctx = lumen.harness.ctx.clone();
        lumen.app().focused_pane(&ctx)
    };
    assert_eq!(pane(&lumen), None);
    lumen.key(Modifiers::NONE, Key::F6);
    assert_eq!(pane(&lumen).as_deref(), Some("Arrangement"));
    lumen.key(Modifiers::NONE, Key::F6);
    assert_eq!(pane(&lumen).as_deref(), Some("Detail"));
    lumen.key(Modifiers::NONE, Key::F6);
    assert_eq!(pane(&lumen).as_deref(), Some("Browser"));
    lumen.key(Modifiers::SHIFT, Key::F6);
    assert_eq!(pane(&lumen).as_deref(), Some("Detail"));
    assert!(lumen.shows("Detail panel"), "panes are named");
}

#[test]
fn shift_tab_toggles_the_detail_panel_except_while_typing() {
    let mut lumen = Lumen::start("keys-shift-tab");
    assert!(lumen.app().detail_shown());
    lumen.key(Modifiers::SHIFT, Key::Tab);
    assert!(!lumen.app().detail_shown(), "hidden");
    lumen.key(Modifiers::SHIFT, Key::Tab);
    assert!(lumen.app().detail_shown(), "shown again");

    // Renaming a track: Shift+Tab is the field's, the panel stays.
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::NONE, Key::F2);
    lumen.key(Modifiers::SHIFT, Key::Tab);
    assert!(lumen.app().detail_shown(), "typing keeps Shift+Tab");
}

#[test]
fn menus_show_the_keys_the_keymap_has() {
    let dir = lumen_project::testing::TempDir::new("e2e-keys-menus");
    let mut lumen = Lumen::start_with(dir, |launch| {
        launch.with_saved_shortcut("track.add_audio", &["Alt+T"])
    });
    lumen.click("Add track");
    assert!(
        lumen.shows(&keys("Add audio track Alt+T")),
        "the changed key"
    );
    assert!(lumen.shows(&keys("Add MIDI track Ctrl+Shift+T")));
    lumen.click(&keys("Add audio track Alt+T"));
    assert_eq!(lumen.track_names().len(), 1);
}

/// With a control reached by Tab, shortcuts still run (egui counts any
/// focused widget as wanting the keyboard, which used to stop them all);
/// only the keys the control uses itself stay with it.
#[test]
fn shortcuts_still_run_with_a_control_focused() {
    let mut lumen = Lumen::start("keys-focused");
    lumen.key(Modifiers::NONE, Key::Tab);
    let ctx = lumen.harness.ctx.clone();
    assert!(ctx.memory(|m| m.focused().is_some()), "a control has focus");
    lumen.key(Modifiers::COMMAND, Key::T);
    assert_eq!(lumen.track_names().len(), 1, "Ctrl+T still adds a track");
}

/// A right-click on a track's name opens the track's menu, as one on the
/// rest of its header does (the name sat on top and took the click).
#[test]
fn a_right_click_on_a_track_name_opens_its_menu() {
    let mut lumen = Lumen::start("keys-track-menu");
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.wait_for("two tracks", |app| app.project().tracks.len() == 2);
    lumen.frames(30);
    // The first track: it stays where it is while the second one's row
    // settles in under a loaded run.
    let name = lumen.track_names()[0].clone();
    right_click(&mut lumen, &format!("{name}: double-click to rename"));
    assert!(
        lumen.shows(&keys("Rename Ctrl+R")),
        "the track's menu, with its keys"
    );
    assert!(lumen.shows(&keys("Move down Alt+Down")));
    // The first track cannot go further up, so the menu leaves it out
    // (the interface audit's M3).
    assert!(!lumen.shows(&keys("Move up Alt+Up")));
}

/// The M3 review's H2, in the owner's choice: with a clip selected the
/// arrows move between clips, Shift adds to the
/// selection, and the view follows; with tracks selected they act on
/// tracks as before. Ctrl+A then an arrow reaches a single clip.
#[test]
fn arrows_move_between_clips_when_a_clip_is_selected() {
    use super::wav;
    let mut lumen = Lumen::start("clip-arrows");
    let files: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|n| wav(&lumen.dir().join(format!("{n}.wav")), 1.0))
        .collect();
    // Track 1: a and b one after the other; track 2: c. All by keys.
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.app_mut().answer_next_dialog(files[..2].to_vec());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    lumen.wait_for("a and b", |app| app.project().tracks[0].clips.len() == 2);
    lumen.key(Modifiers::COMMAND, Key::T);
    lumen.app_mut().answer_next_dialog(files[2..].to_vec());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    lumen.wait_for("c", |app| app.project().tracks[1].clips.len() == 1);
    let id =
        |lumen: &Lumen, track: usize, i: usize| lumen.app().project().tracks[track].clips[i].id;
    let (a, b, c) = (id(&lumen, 0, 0), id(&lumen, 0, 1), id(&lumen, 1, 0));

    // Tracks selected: ↑ selects the track above, as before.
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert_eq!(lumen.app().selected_tracks().len(), 1);
    assert!(lumen.app().selected_clips().is_empty());

    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(lumen.app().selected_clips(), [a], "the first clip alone");
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert_eq!(lumen.app().selected_clips(), [b]);
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    assert_eq!(lumen.app().selected_clips(), [c], "the nearest clip below");
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    assert_eq!(lumen.app().selected_clips(), [a], "a lies under c's start");
    lumen.key(Modifiers::SHIFT, Key::ArrowRight);
    let mut both = lumen.app().selected_clips();
    both.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(both, expected, "Shift adds to the selection");

    // The selected clip is what clip commands act on: 0 mutes it.
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    lumen.key(Modifiers::NONE, Key::Num0);
    assert!(lumen.app().project().tracks[0].clips[0].muted);
    assert!(!lumen.app().project().tracks[0].clips[1].muted);
}

/// A clip chosen from the keyboard far off screen scrolls into view.
#[test]
fn the_view_follows_a_clip_chosen_with_the_arrows() {
    let (mut lumen, kick, snare) = with_two_clips("clip-arrows-view");
    select(&mut lumen, kick);
    // Look far to the right, past both clips.
    let mut view = lumen.app().view();
    view.left = 400.0;
    lumen.app_mut().set_view(view);
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.wait_for("the view to glide", |app| app.view().left < 400.0);
    lumen.frames(60);
    assert_eq!(lumen.app().selected_clips(), [snare]);
    let ctx = lumen.harness.ctx.clone();
    assert!(
        lumen.app().clip_rect(&ctx, snare).is_some(),
        "the chosen clip is on screen"
    );
}
