use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, MouseWheelUnit, Pos2, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use lumen_devices::dynamics::compressor;

use super::Lumen;
use super::dynamics::{at, param, undone_once, whole};
use lumen_app::testing::{face_frame, face_repaint_ask, forget_face_repaint_asks};

fn ring_edge(lumen: &Lumen) -> Pos2 {
    let field = lumen.harness.get_by_label_contains("Threshold ring").rect();
    Pos2::new(field.left() + 4.0, field.center().y)
}

#[test]
fn a_fine_drag_on_the_ring_moves_a_tenth_as_far_as_a_free_one() {
    let (mut lumen, device) = whole("gravity-fine", "Add Gravity", compressor::NAME);
    let start = param(&lumen, device, compressor::THRESHOLD).expect("set");
    let edge = ring_edge(&lumen);
    lumen.drag(edge, edge + vec2(-30.0, 0.0), Modifiers::ALT);
    lumen.frames(10);
    let free = start - param(&lumen, device, compressor::THRESHOLD).expect("set");
    assert!(free > 0.0, "the ring went out: {free}");
    undone_once(&mut lumen, "Add Gravity");

    let edge = ring_edge(&lumen);
    lumen.drag(
        edge,
        edge + vec2(-30.0, 0.0),
        Modifiers::SHIFT | Modifiers::ALT,
    );
    lumen.frames(10);
    let fine = start - param(&lumen, device, compressor::THRESHOLD).expect("set");
    assert!(
        fine > 0.0 && (fine - free * 0.1).abs() < free * 0.02,
        "fine {fine} against free {free}"
    );
}

#[test]
fn double_clicking_the_ring_and_the_ball_put_their_dials_back_in_one_step_each() {
    let (mut lumen, device) = whole("gravity-reset", "Add Gravity", compressor::NAME);
    let threshold = param(&lumen, device, compressor::THRESHOLD).expect("set");
    let ratio = param(&lumen, device, compressor::RATIO).expect("set");

    let edge = ring_edge(&lumen);
    lumen.drag(edge, edge + vec2(-24.0, 0.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(param(&lumen, device, compressor::THRESHOLD).expect("set") < threshold);
    let edge = ring_edge(&lumen);
    lumen.double_click(edge);
    lumen.frames(10);
    assert!((param(&lumen, device, compressor::THRESHOLD).expect("set") - threshold).abs() < 1e-6);

    let ball = at(&lumen, "Body weight");
    lumen.drag(ball, ball + vec2(0.0, 30.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(param(&lumen, device, compressor::RATIO).expect("set") > ratio);
    let ball = at(&lumen, "Body weight");
    lumen.double_click(ball);
    lumen.frames(10);
    assert!((param(&lumen, device, compressor::RATIO).expect("set") - ratio).abs() < 1e-6);
    assert_eq!(
        param(&lumen, device, compressor::THRESHOLD).map(|v| (v - threshold).abs() < 1e-6),
        Some(true)
    );
}

#[test]
fn the_wheel_over_the_ring_moves_the_threshold_by_the_dials_notches() {
    let (mut lumen, device) = whole("gravity-wheel", "Add Gravity", compressor::NAME);
    let before = param(&lumen, device, compressor::THRESHOLD).expect("set");
    let edge = ring_edge(&lumen);
    lumen.harness.hover_at(edge);
    lumen.frames(2);
    lumen.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 80.0),
        modifiers: Modifiers::NONE,
        phase: TouchPhase::Move,
    });
    lumen.frames(12);
    let after = param(&lumen, device, compressor::THRESHOLD).expect("set");
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
fn the_arrow_keys_move_the_focused_ball_and_the_ring_by_a_notch() {
    let (mut lumen, device) = whole("gravity-keys", "Add Gravity", compressor::NAME);
    let ratio = param(&lumen, device, compressor::RATIO).expect("set");
    lumen.harness.get_by_label_contains("Body weight").focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.frames(5);
    let after = param(&lumen, device, compressor::RATIO).expect("set");
    assert!(after > ratio, "{ratio} then {after}");

    let threshold = param(&lumen, device, compressor::THRESHOLD).expect("set");
    lumen
        .harness
        .get_by_label_contains("Threshold ring")
        .focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    lumen.frames(5);
    assert!(param(&lumen, device, compressor::THRESHOLD).expect("set") < threshold);
}

#[test]
fn the_ring_and_the_dial_are_one_edit_and_the_face_names_what_it_shows() {
    let (mut lumen, device) = whole("gravity-names", "Add Gravity", compressor::NAME);
    let before = param(&lumen, device, compressor::THRESHOLD).expect("set");
    let edge = ring_edge(&lumen);
    lumen.drag(edge, edge + vec2(-20.0, 0.0), Modifiers::NONE);
    lumen.frames(10);
    let threshold = param(&lumen, device, compressor::THRESHOLD).expect("set");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Pull, parameter")
            .is_some()
    );
    assert!(
        lumen
            .harness
            .query_by_label_contains("Threshold ring (Pull): ")
            .is_some()
    );
    assert!(
        threshold < before,
        "the ring's drag is the dial's edit: {before} then {threshold}"
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
fn gravity_rests_asks_for_the_face_frame_while_pulling_and_rests_again() {
    let (mut lumen, device) = whole("gravity-cadence", "Add Gravity", compressor::NAME);
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
            "a resting well asks for nothing, frame {frame}"
        );
    }

    lumen.app_mut().pretend_track_level(Some(-6.0));
    lumen.app_mut().pretend_reduction(device, 6.0, -6.0);
    lumen.frames(10);
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            Some(face_frame()),
            "a pulling well asks for the face frame, frame {frame}"
        );
    }

    lumen.app_mut().pretend_track_level(None);
    lumen.app_mut().pretend_reduction(device, 0.0, -60.0);
    lumen.run_until("the well to go to sleep", |_| {
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
