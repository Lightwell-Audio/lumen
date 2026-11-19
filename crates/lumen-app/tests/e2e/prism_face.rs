use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, MouseWheelUnit, Pos2, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use lumen_devices::utility::{self, WIDTH};
use lumen_project::model::Id;

use super::Lumen;
use super::dynamics::{param, undone_once, whole};
use lumen_app::testing::{face_frame, face_repaint_ask, forget_face_repaint_asks};

fn whole_prism(file: &str) -> (Lumen, Id) {
    whole(file, "Add Prism", utility::NAME)
}

fn tip(lumen: &Lumen, which: &str) -> Pos2 {
    let handle = lumen.harness.get_by_label_contains(which).rect();
    Pos2::new(handle.right() - 6.0, handle.center().y)
}

fn width(lumen: &Lumen, device: Id) -> f64 {
    param(lumen, device, WIDTH).expect("set")
}

#[test]
fn dragging_the_upper_tip_in_narrows_the_beams_in_one_undo_step() {
    let (mut lumen, device) = whole_prism("prism-upper");
    let start = width(&lumen, device);
    let tip = tip(&lumen, "Beam spread, upper");
    lumen.drag(tip, tip + vec2(0.0, 18.0), Modifiers::NONE);
    lumen.frames(10);
    let after = width(&lumen, device);
    assert!(after < start, "the fan closed: {start} then {after}");
    undone_once(&mut lumen, "Add Prism");
    assert!((width(&lumen, device) - start).abs() < 1e-6);
}

#[test]
fn dragging_the_lower_tip_out_opens_the_beams_in_one_undo_step() {
    let (mut lumen, device) = whole_prism("prism-lower");
    let start = width(&lumen, device);
    let tip = tip(&lumen, "Beam spread, lower");
    lumen.drag(tip, tip + vec2(0.0, 14.0), Modifiers::NONE);
    lumen.frames(10);
    let after = width(&lumen, device);
    assert!(after > start, "the fan opened: {start} then {after}");
    undone_once(&mut lumen, "Add Prism");
    assert!((width(&lumen, device) - start).abs() < 1e-6);
}

#[test]
fn a_fine_drag_moves_a_tenth_as_far_as_a_free_one() {
    let (mut lumen, device) = whole_prism("prism-fine");
    let start = width(&lumen, device);
    let tip_at = tip(&lumen, "Beam spread, upper");
    lumen.drag(tip_at, tip_at + vec2(0.0, 12.0), Modifiers::ALT);
    lumen.frames(10);
    let free = start - width(&lumen, device);
    assert!(free > 0.0, "the free drag closed the fan: {free}");
    undone_once(&mut lumen, "Add Prism");

    let tip_at = tip(&lumen, "Beam spread, upper");
    lumen.drag(
        tip_at,
        tip_at + vec2(0.0, 12.0),
        Modifiers::SHIFT | Modifiers::ALT,
    );
    lumen.frames(10);
    let fine = start - width(&lumen, device);
    assert!(
        fine > 0.0 && (fine - free * 0.1).abs() < free * 0.03,
        "fine {fine} against free {free}"
    );
}

#[test]
fn the_wheel_over_a_tip_moves_the_width_by_the_dials_notches() {
    let (mut lumen, device) = whole_prism("prism-wheel");
    let before = width(&lumen, device);
    let tip = tip(&lumen, "Beam spread, upper");
    lumen.harness.hover_at(tip);
    lumen.frames(2);
    lumen.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 80.0),
        modifiers: Modifiers::NONE,
        phase: TouchPhase::Move,
    });
    lumen.frames(12);
    let after = width(&lumen, device);
    assert!(
        (after - before).abs() > 1e-6,
        "the wheel moved it: {before} {after}"
    );
    assert!(
        lumen
            .app()
            .undo_label()
            .is_some_and(|label| label.starts_with("Change")),
        "{:?}",
        lumen.app().undo_label()
    );
}

#[test]
fn the_arrow_keys_move_a_focused_tip_by_a_notch() {
    let (mut lumen, device) = whole_prism("prism-keys");
    let before = width(&lumen, device);
    lumen
        .harness
        .get_by_label_contains("Beam spread, upper")
        .focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.frames(5);
    let after = width(&lumen, device);
    assert!(after > before, "{before} then {after}");

    lumen
        .harness
        .get_by_label_contains("Beam spread, lower")
        .focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowDown);
    lumen.frames(5);
    assert!(width(&lumen, device) < after);
}

#[test]
fn double_clicking_a_tip_puts_the_width_back_in_one_step() {
    let (mut lumen, device) = whole_prism("prism-reset");
    let start = width(&lumen, device);
    let tip_at = tip(&lumen, "Beam spread, lower");
    lumen.drag(tip_at, tip_at + vec2(0.0, 12.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(width(&lumen, device) > start);
    let tip_at = tip(&lumen, "Beam spread, lower");
    lumen.double_click(tip_at);
    lumen.frames(10);
    assert!((width(&lumen, device) - start).abs() < 1e-6);
}

#[test]
fn the_tips_and_the_dial_are_one_edit_and_the_face_names_what_it_shows() {
    let (mut lumen, device) = whole_prism("prism-names");
    let before = width(&lumen, device);
    let tip_at = tip(&lumen, "Beam spread, upper");
    lumen.drag(tip_at, tip_at + vec2(0.0, 12.0), Modifiers::NONE);
    lumen.frames(10);
    let after = width(&lumen, device);
    assert!(after < before, "the tip's drag is the dial's edit");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Beam spread, upper (Width): ")
            .is_some()
    );
    assert!(
        lumen
            .harness
            .query_by_label_contains("Beam spread, lower (Width): ")
            .is_some()
    );
    assert!(
        lumen
            .harness
            .query_by_label_contains("Width, parameter")
            .is_some()
    );
    assert_eq!(
        lumen.app().undo_label().map(|l| l.starts_with("Change")),
        Some(true)
    );
}

fn asked_in_one_frame(lumen: &mut Lumen) -> Option<Duration> {
    let ctx = lumen.harness.ctx.clone();
    forget_face_repaint_asks(&ctx);
    lumen.frames(1);
    face_repaint_ask(&ctx)
}

#[test]
fn prism_rests_asks_for_the_face_frame_while_light_flows_and_rests_again() {
    let (mut lumen, _) = whole_prism("prism-cadence");
    let ctx = lumen.harness.ctx.clone();
    lumen.run_until("the face to be asleep", |_| {
        let asked = face_repaint_ask(&ctx);
        forget_face_repaint_asks(&ctx);
        asked.is_none()
    });
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            None,
            "a resting prism asks for nothing, frame {frame}"
        );
    }

    lumen.app_mut().pretend_track_level(Some(-6.0));
    lumen.frames(10);
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            Some(face_frame()),
            "a lit prism asks for the face frame, frame {frame}"
        );
    }

    lumen.app_mut().pretend_track_level(None);
    lumen.run_until("the prism to go to sleep", |_| {
        let asked = face_repaint_ask(&ctx);
        forget_face_repaint_asks(&ctx);
        asked.is_none()
    });
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            None,
            "asleep again, frame {frame}"
        );
    }
}
