//! M7b E2: automatic and explicit crossfades, their accessible controls,
//! the crossed fade handle, history, and save/reopen.

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::{Lumen, keys, wav};

fn clip(lumen: &Lumen, id: Id) -> lumen_project::model::Clip {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .find(|clip| clip.id == id)
        .map(|clip| (**clip).clone())
        .expect("clip exists")
}

fn rect(lumen: &Lumen, id: Id) -> eframe::egui::Rect {
    lumen
        .app()
        .clip_rect(&lumen.harness.ctx, id)
        .expect("clip is visible")
}

fn command(lumen: &mut Lumen, query: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(query.to_owned()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
}

fn joined_parts(name: &str) -> (Lumen, Id) {
    let mut lumen = Lumen::start(name);
    let file = wav(&lumen.dir().join("crossfade.wav"), 4.0);
    lumen.drop_files(&[file]);
    lumen.frames(40);
    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::Equals);
    }
    lumen.frames(15);
    let first = lumen.app().project().tracks[0].clips[0].id;
    let r = rect(&lumen, first);
    let zoom = lumen.app().view().zoom as f32;
    let cut = Pos2::new(r.left() + 2.0 * zoom, r.bottom() - 6.0);
    lumen.harness.hover_at(cut);
    lumen.frames(1);
    for pressed in [true, false] {
        lumen.harness.event(Event::PointerButton {
            pos: cut,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.key(Modifiers::COMMAND, Key::E);
    let right = lumen.app().project().tracks[0].clips[1].id;
    // Slip the right clip's source window, preserving the join while making
    // the incoming source non-contiguous and leaving tail in the first file.
    lumen.app_mut().select_clip(right);
    let r = rect(&lumen, right);
    let from = Pos2::new(r.center().x, r.top() + 6.0);
    lumen.drag(
        from,
        from + vec2(0.25 * zoom, 0.0),
        Modifiers::COMMAND | Modifiers::ALT,
    );
    assert_eq!(clip(&lumen, right).start, Ticks::quarters(2));
    (lumen, right)
}

#[test]
fn auto_crossfades_toggle_and_explicit_crossfade_handle_roundtrip() {
    let (mut lumen, id) = joined_parts("crossfade-e2e");
    assert!(
        lumen.app().project().auto_crossfade,
        "new projects enable automatic crossfades"
    );

    // The project menu exposes the saved switch, and the change is undoable.
    lumen.click("Lumen");
    lumen.click("Automatic crossfades");
    assert!(!lumen.app().project().auto_crossfade);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(lumen.app().project().auto_crossfade);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(!lumen.app().project().auto_crossfade);
    command(&mut lumen, "Toggle automatic crossfades");
    assert!(lumen.app().project().auto_crossfade);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!lumen.app().project().auto_crossfade);
    lumen.click("Lumen");
    lumen.frames(3);
    lumen.click("Automatic crossfades");
    assert!(lumen.app().project().auto_crossfade);

    // Ctrl+Shift+X and the palette both reach the selected audio command.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
    assert!(clip(&lumen, id).crossfade);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!clip(&lumen, id).crossfade);
    command(&mut lumen, "Toggle custom crossfade");
    assert!(clip(&lumen, id).crossfade);
    let r = rect(&lumen, id);
    lumen.right_click(Pos2::new(r.left() + 12.0, r.top() + 6.0));
    lumen.frames(2);
    let menu_item = keys("Custom crossfade at join Ctrl+Shift+X");
    assert!(lumen.shows(&menu_item));
    lumen.click(&menu_item);
    assert!(!clip(&lumen, id).crossfade);
    assert_eq!(
        lumen.app().note(),
        Some("Custom crossfade disabled; automatic crossfades still apply")
    );
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
    assert!(clip(&lumen, id).crossfade);

    // The existing incoming fade handle lengthens the crossed fade as one
    // undo gesture and its info line names the action.
    let at = lumen.app().clip_handles(&lumen.harness.ctx, id).unwrap();
    lumen.harness.hover_at(at.fade_in);
    lumen.frames(2);
    assert!(
        lumen
            .info()
            .starts_with("Crossfade: drag to set its length")
    );
    let initial = clip(&lumen, id).fade_in.length;
    lumen.drag(
        at.fade_in,
        at.fade_in + vec2(at.wave.width() * 0.2, 0.0),
        Modifiers::NONE,
    );
    assert!(clip(&lumen, id).fade_in.length > initial);
    assert_eq!(lumen.app().undo_label(), Some("Crossfade length"));
    let lengthened = clip(&lumen, id).fade_in.length;
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id).fade_in.length, initial);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(clip(&lumen, id).fade_in.length, lengthened);
    let at = lumen.app().clip_handles(&lumen.harness.ctx, id).unwrap();
    lumen.double_click(at.fade_in);
    assert_eq!(clip(&lumen, id).fade_in.length, Ticks::ZERO);
    assert!(
        clip(&lumen, id).crossfade,
        "zero length explicitly suppresses auto"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, id).fade_in.length, lengthened);

    // Project setting, explicit flag and handle length survive a real save.
    let path = lumen_app::testing::project_file(lumen.dir(), "Crossfade");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the project to save", |app| {
        !app.is_busy() && !app.is_dirty()
    });
    let dir = lumen.crash();
    let reopened = Lumen::start_with(dir, |launch| launch.opening(path));
    assert!(reopened.app().project().auto_crossfade);
    assert!(clip(&reopened, id).crossfade);
    assert_eq!(clip(&reopened, id).fade_in.length, lengthened);
}

#[test]
fn dragging_the_left_clips_fade_out_handle_into_the_next_clip_makes_the_crossfade() {
    let (mut lumen, right) = joined_parts("crossfade-left-handle");
    let left = lumen.app().project().tracks[0].clips[0].id;
    // With automatic crossfades off, the drag alone makes the crossfade.
    lumen.click("Lumen");
    lumen.click("Automatic crossfades");
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(!lumen.app().project().auto_crossfade);
    assert!(!clip(&lumen, right).crossfade);

    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    lumen.harness.hover_at(at.fade_out);
    lumen.frames(2);
    assert!(
        lumen
            .info()
            .contains("drag into the next clip to crossfade")
    );
    let reach = vec2(rect(&lumen, right).width() * 0.2, 0.0);
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);
    let made = clip(&lumen, right);
    assert!(made.crossfade && made.fade_in.length > Ticks::ZERO);
    assert_eq!(clip(&lumen, left).fade_out.length, Ticks::ZERO);
    // The whole drag is one undo step.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!clip(&lumen, right).crossfade);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(clip(&lumen, right).crossfade);

    // Dragging the handle back to the join is a hard cut there.
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);
    lumen.drag(
        at.fade_out + reach,
        at.fade_out - vec2(4.0, 0.0),
        Modifiers::NONE,
    );
    assert!(clip(&lumen, right).crossfade);
    assert_eq!(clip(&lumen, right).fade_in.length, Ticks::ZERO);
}

#[test]
fn project_menu_keeps_crossfades_and_lower_controls_reachable_in_a_small_window() {
    let mut lumen = Lumen::start("crossfade-small-menu");
    lumen.harness.set_size(vec2(960.0, 540.0));
    lumen.frames(20);
    lumen.click("Lumen");
    let automatic = lumen.harness.get_by_label("Automatic crossfades").rect();
    assert!(automatic.top() >= 0.0 && automatic.bottom() <= 540.0);
    lumen.click("Automatic crossfades");
    assert!(!lumen.app().project().auto_crossfade);
    lumen.harness.hover_at(automatic.center());
    for _ in 0..3 {
        lumen.harness.event(Event::MouseWheel {
            unit: eframe::egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -640.0),
            modifiers: Modifiers::NONE,
            phase: eframe::egui::TouchPhase::Move,
        });
        lumen.frames(10);
    }
    let shortcuts = lumen
        .harness
        .get_by_label("Keyboard shortcuts\u{2026}")
        .rect();
    assert!(
        shortcuts.top() >= 0.0 && shortcuts.bottom() <= 540.0,
        "{shortcuts:?}"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    let file = wav(&lumen.dir().join("menu.wav"), 2.0);
    lumen.drop_files(&[file]);
    lumen.wait_for("the audio clip", |app| !app.project().tracks.is_empty());
    lumen.frames(20);
    let id = lumen.app().project().tracks[0].clips[0].id;
    let r = rect(&lumen, id);
    lumen.right_click(Pos2::new(r.left() + 12.0, r.top() + 6.0));
    let custom = lumen
        .harness
        .get_by_label_contains("Custom crossfade at join")
        .rect();
    assert!(
        custom.top() >= 0.0 && custom.bottom() <= 540.0,
        "{custom:?}"
    );
    lumen.click("Edit");
    assert!(lumen.shows("Reverse"));
}
