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

fn split_parts(name: &str) -> (Lumen, Id, Id) {
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
    let (left, right) = {
        let clips = &lumen.app().project().tracks[0].clips;
        assert_eq!(clips.len(), 2);
        (clips[0].id, clips[1].id)
    };
    assert_eq!(clip(&lumen, right).start, Ticks::quarters(2));
    lumen.app_mut().select_clip(right);
    lumen.frames(2);
    (lumen, left, right)
}

fn end_of(lumen: &Lumen, id: Id) -> Ticks {
    let c = clip(lumen, id);
    c.start + c.length
}

#[test]
fn the_automatic_setting_the_command_and_the_menu_roundtrip() {
    let (mut lumen, left, right) = split_parts("crossfade-e2e");
    assert!(
        lumen.app().project().auto_crossfade,
        "new projects enable automatic crossfades"
    );

    lumen.click("Lumen");
    assert!(!lumen.shows("Automatic crossfades"));
    lumen.key(Modifiers::NONE, Key::Escape);
    let r = rect(&lumen, right);
    lumen.right_click(r.left_top() + vec2(12.0, 6.0));
    assert!(lumen.shows(&keys("Properties\u{2026} Alt+Enter")));
    assert!(!lumen.shows("Automatic crossfades"));
    lumen.key(Modifiers::NONE, Key::Escape);
    command(&mut lumen, "Toggle automatic crossfades");
    assert!(!lumen.app().project().auto_crossfade);
    assert_eq!(
        lumen.app().undo_label(),
        Some("Disable automatic crossfades")
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(lumen.app().project().auto_crossfade);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(!lumen.app().project().auto_crossfade);
    command(&mut lumen, "Toggle automatic crossfades");
    assert!(lumen.app().project().auto_crossfade);

    assert_eq!(lumen.app().selected_clips(), [right]);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
    let made = clip(&lumen, right);
    assert!(made.crossfade && made.fade_in.length > Ticks::ZERO);
    assert_eq!(end_of(&lumen, left), made.start, "the clips still touch");
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(!clip(&lumen, right).crossfade);
    assert_eq!(clip(&lumen, right).start, Ticks::quarters(2));
    command(&mut lumen, "Crossfade with previous");
    assert!(clip(&lumen, right).crossfade);
    let r = rect(&lumen, right);
    lumen.right_click(Pos2::new(r.left() + 40.0, r.top() + 6.0));
    lumen.frames(2);
    let menu_item = keys("Crossfade with previous Ctrl+Shift+X");
    assert!(lumen.shows(&menu_item));
    lumen.click(&menu_item);
    assert!(!clip(&lumen, right).crossfade);
    assert_eq!(
        end_of(&lumen, left),
        clip(&lumen, right).start,
        "they stay touching"
    );
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::X);
    assert!(clip(&lumen, right).crossfade);

    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    assert!(at.crossfade, "the crossing is on the left clip's handle");
    lumen.harness.hover_at(at.fade_out);
    lumen.frames(2);
    assert!(lumen.info().starts_with("Crossfade"), "{}", lumen.info());
    let initial = clip(&lumen, right).fade_in.length;
    lumen.drag(
        at.fade_out,
        at.fade_out - vec2(at.wave.width() * 0.1, 0.0),
        Modifiers::NONE,
    );
    let changed = clip(&lumen, right).fade_in.length;
    assert_ne!(changed, initial);
    assert_eq!(end_of(&lumen, left), clip(&lumen, right).start);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, right).fade_in.length, initial);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(clip(&lumen, right).fade_in.length, changed);

    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    lumen.double_click(at.fade_out);
    assert!(!clip(&lumen, right).crossfade);
    assert_eq!(clip(&lumen, right).fade_in.length, Ticks::ZERO);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, right).fade_in.length, changed);

    let path = lumen_app::testing::project_file(lumen.dir(), "Crossfade");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the project to save", |app| {
        !app.is_busy() && !app.is_dirty()
    });
    let dir = lumen.crash();
    let reopened = Lumen::start_with(dir, |launch| launch.opening(path));
    assert!(reopened.app().project().auto_crossfade);
    assert!(clip(&reopened, right).crossfade);
    assert_eq!(clip(&reopened, right).fade_in.length, changed);
    assert_eq!(end_of(&reopened, left), clip(&reopened, right).start);
}

#[test]
fn pulling_the_left_clips_handle_over_the_right_clip_makes_a_crossfade() {
    let (mut lumen, left, right) = split_parts("crossfade-left-handle");
    let before = lumen.app().project().clone();
    let (a, b) = (clip(&lumen, left), clip(&lumen, right));
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    lumen.harness.hover_at(at.fade_out);
    lumen.frames(2);
    assert!(
        lumen
            .info()
            .contains("drag into the next clip to crossfade"),
        "{}",
        lumen.info()
    );
    let reach = vec2(rect(&lumen, right).width() * 0.2, 0.0);
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);

    let (a2, b2) = (clip(&lumen, left), clip(&lumen, right));
    assert!(b2.crossfade && b2.fade_in.length > Ticks::ZERO);
    assert_eq!(a2.start, a.start);
    assert!(a2.length > a.length, "the left clip is extended");
    assert_eq!(
        b2.start,
        a2.start + a2.length,
        "the right clip starts where the crossfade ends"
    );
    assert_eq!(b2.start - Ticks::quarters(2), b2.fade_in.length);
    assert_eq!(b2.start + b2.length, b.start + b.length, "its end stays");
    let shown = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    assert!(shown.crossfade);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(*lumen.app().project(), before);
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(clip(&lumen, right).crossfade);

    let length = clip(&lumen, right).fade_in.length;
    let start = clip(&lumen, right).start;
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    lumen.drag(at.fade_out, at.fade_out - reach * 0.5, Modifiers::NONE);
    assert!(clip(&lumen, right).fade_in.length > length);
    assert_eq!(clip(&lumen, right).start, start);
    assert_eq!(end_of(&lumen, left), start);
}

#[test]
fn the_right_clip_moves_from_where_the_crossfade_ended() {
    let (mut lumen, left, right) = split_parts("crossfade-reposition");
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    let reach = vec2(rect(&lumen, right).width() * 0.2, 0.0);
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);
    let crossing_end = clip(&lumen, right).start;
    assert!(crossing_end > Ticks::quarters(2));
    assert_eq!(end_of(&lumen, left), crossing_end);

    let zoom = lumen.app().view().zoom as f32;
    let r = rect(&lumen, right);
    let from = Pos2::new(r.center().x, r.top() + 6.0);
    lumen.drag(from, from + vec2(zoom, 0.0), Modifiers::NONE);
    let moved = clip(&lumen, right);
    assert!(moved.start > crossing_end, "{:?}", moved.start);
    assert!(!moved.crossfade);
    assert_eq!(moved.fade_in.length, Ticks::ZERO);
}

#[test]
fn the_right_clips_fade_handle_lengthens_the_crossfade() {
    let (mut lumen, left, right) = split_parts("crossfade-right-handle");
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    let reach = vec2(rect(&lumen, right).width() * 0.1, 0.0);
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);
    let made = clip(&lumen, right);
    assert!(made.crossfade);

    let at = lumen.app().clip_handles(&lumen.harness.ctx, right).unwrap();
    lumen.harness.hover_at(at.fade_in);
    lumen.frames(2);
    assert!(lumen.info().starts_with("Crossfade"), "{}", lumen.info());
    lumen.drag(at.fade_in, at.fade_in + reach, Modifiers::NONE);
    let longer = clip(&lumen, right);
    assert!(longer.crossfade, "still a crossfade, not a fade-in");
    assert!(longer.fade_in.length > made.fade_in.length);
    assert_eq!(
        longer.start - longer.fade_in.length,
        made.start - made.fade_in.length
    );
    assert!(longer.start > made.start);
    assert_eq!(end_of(&lumen, left), longer.start);
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert_eq!(clip(&lumen, right).start, made.start);
    assert_eq!(clip(&lumen, right).fade_in.length, made.fade_in.length);
}

#[test]
fn extending_the_left_clip_keeps_the_crossfade_as_long_as_it_was() {
    let (mut lumen, left, right) = split_parts("crossfade-extend-left");
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    let reach = vec2(rect(&lumen, right).width() * 0.1, 0.0);
    lumen.drag(at.fade_out, at.fade_out + reach, Modifiers::NONE);
    let made = clip(&lumen, right);
    assert!(made.crossfade);

    let r = rect(&lumen, left);
    let edge = Pos2::new(r.right() - 1.0, r.center().y + 8.0);
    lumen.drag(edge, edge + reach, Modifiers::ALT);
    let after = clip(&lumen, right);
    assert!(after.start > made.start, "the left clip was extended");
    assert!(after.crossfade);
    assert_eq!(after.fade_in.length, made.fade_in.length);
    assert_eq!(end_of(&lumen, left), after.start);
}

#[test]
fn dragging_the_handle_back_in_one_gesture_leaves_the_clips_as_they_were() {
    let (mut lumen, left, right) = split_parts("crossfade-drag-back");
    let (a, b) = (clip(&lumen, left), clip(&lumen, right));
    let at = lumen.app().clip_handles(&lumen.harness.ctx, left).unwrap();
    let reach = vec2(rect(&lumen, right).width() * 0.2, 0.0);
    let from = at.fade_out;
    lumen.harness.hover_at(from);
    lumen.harness.drag_at(from);
    lumen.frames(2);
    for step in 1..=8 {
        lumen.harness.hover_at(from + reach * (step as f32 / 8.0));
        lumen.frames(1);
    }
    assert!(clip(&lumen, right).crossfade, "made while dragging");
    for step in (0..=8).rev() {
        lumen.harness.hover_at(from + reach * (step as f32 / 8.0));
        lumen.frames(1);
    }
    lumen.harness.hover_at(from - vec2(2.0, 0.0));
    lumen.frames(1);
    lumen.harness.drop_at(from - vec2(2.0, 0.0));
    lumen.frames(3);
    let (a2, b2) = (clip(&lumen, left), clip(&lumen, right));
    assert_eq!(a2.start + a2.length, b.start, "the clips meet at the split");
    assert_eq!(b2.start, b.start);
    assert_eq!(b2.start + b2.length, b.start + b.length);
    assert!(!b2.crossfade);
    assert!(a2.length >= a.length);
}

#[test]
fn project_menu_keeps_lower_controls_reachable_in_a_small_window() {
    let mut lumen = Lumen::start("crossfade-small-menu");
    lumen.harness.set_size(vec2(960.0, 540.0));
    lumen.frames(20);
    lumen.click("Lumen");
    assert!(!lumen.shows("Automatic crossfades"));
    let first = lumen
        .harness
        .get_by_label_contains(&keys("New Ctrl+"))
        .rect();
    lumen.harness.hover_at(first.center());
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
        .get_by_label_contains("Crossfade with previous")
        .rect();
    assert!(
        custom.top() >= 0.0 && custom.bottom() <= 540.0,
        "{custom:?}"
    );
    lumen.click("Edit");
    assert!(lumen.shows("Reverse"));
}
