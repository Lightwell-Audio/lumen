use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, MouseWheelUnit, Pos2, TouchPhase, vec2};
use egui_kittest::kittest::Queryable;
use lumen_devices::eq::{self, freq_of, gain_of, q_of};
use lumen_project::model::{Id, PluginRef};

use super::Lumen;
use super::lane_view::{run_command, tone};
use lumen_app::testing::{face_frame, face_repaint_ask, forget_face_repaint_asks};

fn iris(lumen: &Lumen) -> Option<Id> {
    lumen.app().project().tracks[0]
        .devices
        .iter()
        .find(|d| matches!(&d.plugin, PluginRef::Builtin { name } if name == eq::NAME))
        .map(|d| d.id)
}

fn launched(name: &str) -> (Lumen, Id) {
    let (mut lumen, _track) = tone(name);
    run_command(&mut lumen, "Add Iris");
    lumen.wait_for("Iris's card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(&d.plugin, PluginRef::Builtin { name } if name == eq::NAME))
    });
    lumen.show_whole_cards();
    lumen.frames(10);
    let device = iris(&lumen).expect("the device is on the track");
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(device)
    });
    lumen.frames(10);
    (lumen, device)
}

#[track_caller]
fn node(lumen: &Lumen, band: usize) -> Pos2 {
    lumen
        .harness
        .get_by_label_contains(&format!("Band {band}, "))
        .rect()
        .center()
}

fn param(lumen: &Lumen, device: Id, id: u32) -> f64 {
    lumen
        .app()
        .device_params(device)
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, value)| *value)
        .expect("the parameter is held")
}

fn one_undo_back_to(lumen: &mut Lumen, label: &str) {
    assert!(
        lumen
            .app()
            .undo_label()
            .is_some_and(|label| label.starts_with("Change") || label.starts_with("Reset")),
        "one named step: {:?}",
        lumen.app().undo_label()
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    assert_eq!(lumen.app().undo_label(), Some(label), "one undo");
}

#[test]
fn dragging_a_node_moves_frequency_and_gain_in_one_undo_step() {
    let (mut lumen, device) = launched("iris-drag");
    let (freq, gain) = (
        param(&lumen, device, freq_of(3)),
        param(&lumen, device, gain_of(3)),
    );
    let from = node(&lumen, 3);
    lumen.drag(from, from + vec2(48.0, -30.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(param(&lumen, device, freq_of(3)) > freq + 0.03);
    assert!(param(&lumen, device, gain_of(3)) > gain + 0.03);
    one_undo_back_to(&mut lumen, "Add Iris");
    assert_eq!(param(&lumen, device, freq_of(3)), freq);
    assert_eq!(param(&lumen, device, gain_of(3)), gain);
}

#[test]
fn the_node_sits_where_it_was_dragged_to() {
    let (mut lumen, _device) = launched("iris-where");
    let from = node(&lumen, 3);
    let to = from + vec2(40.0, -26.0);
    lumen.drag(from, to, Modifiers::NONE);
    lumen.frames(10);
    let landed = node(&lumen, 3);
    assert!(
        (landed.x - to.x).abs() < 2.0 && (landed.y - to.y).abs() < 1.5,
        "dragged to {to:?}, drawn at {landed:?}"
    );
}

#[test]
fn a_fine_drag_moves_a_tenth_as_far_as_a_free_one() {
    let (mut lumen, device) = launched("iris-fine");
    let start = param(&lumen, device, freq_of(3));
    let from = node(&lumen, 3);
    lumen.drag(from, from + vec2(30.0, 0.0), Modifiers::ALT);
    lumen.frames(10);
    let free = param(&lumen, device, freq_of(3)) - start;
    assert!(free > 0.0, "the node went across: {free}");
    one_undo_back_to(&mut lumen, "Add Iris");

    let from = node(&lumen, 3);
    lumen.drag(
        from,
        from + vec2(30.0, 0.0),
        Modifiers::SHIFT | Modifiers::ALT,
    );
    lumen.frames(10);
    let fine = param(&lumen, device, freq_of(3)) - start;
    assert!(
        fine > 0.0 && (fine - free * 0.1).abs() < free * 0.03,
        "fine {fine} against free {free}"
    );
}

#[test]
fn the_wheel_over_a_node_widens_or_narrows_its_q() {
    let (mut lumen, device) = launched("iris-wheel");
    let before = param(&lumen, device, q_of(3));
    let at = node(&lumen, 3);
    lumen.harness.hover_at(at);
    lumen.frames(2);
    lumen.harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 80.0),
        modifiers: Modifiers::NONE,
        phase: TouchPhase::Move,
    });
    lumen.frames(12);
    let after = param(&lumen, device, q_of(3));
    assert!((after - before).abs() > 1e-6, "{before} then {after}");
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
fn the_arrow_keys_move_the_focused_node_by_a_notch() {
    let (mut lumen, device) = launched("iris-keys");
    let (freq, gain) = (
        param(&lumen, device, freq_of(3)),
        param(&lumen, device, gain_of(3)),
    );
    lumen.harness.get_by_label_contains("Band 3, ").focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    lumen.frames(5);
    assert!(
        param(&lumen, device, freq_of(3)) > freq,
        "right goes up in pitch"
    );
    lumen.harness.get_by_label_contains("Band 3, ").focus();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.frames(5);
    assert!(param(&lumen, device, gain_of(3)) > gain, "up is louder");
}

#[test]
fn double_clicking_a_node_puts_its_gain_back_in_one_step() {
    let (mut lumen, device) = launched("iris-reset");
    let gain = param(&lumen, device, gain_of(3));
    let from = node(&lumen, 3);
    lumen.drag(from, from + vec2(0.0, -34.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(param(&lumen, device, gain_of(3)) > gain + 0.03);
    let at = node(&lumen, 3);
    lumen.double_click(at);
    lumen.frames(10);
    assert!((param(&lumen, device, gain_of(3)) - gain).abs() < 1e-6);
    assert_eq!(lumen.app().undo_label(), Some("Reset 3 Gain"));
    one_undo_back_to(&mut lumen, "Change 3 Freq");
    assert!(
        param(&lumen, device, gain_of(3)) > gain + 0.03,
        "one undo gave back the drag's gain"
    );
}

#[test]
fn a_node_and_its_dial_are_one_edit_and_the_node_says_where_it_is() {
    let (mut lumen, device) = launched("iris-names");
    let from = node(&lumen, 3);
    lumen.drag(from, from + vec2(0.0, -30.0), Modifiers::NONE);
    lumen.frames(10);
    assert!(param(&lumen, device, gain_of(3)) > 0.5);
    assert!(
        lumen
            .harness
            .query_by_label_contains("Band 3, bell, 250 Hz, +")
            .is_some(),
        "the node's name follows its gain"
    );
}

fn asked_in_one_frame(lumen: &mut Lumen) -> Option<Duration> {
    let ctx = lumen.harness.ctx.clone();
    forget_face_repaint_asks(&ctx);
    lumen.frames(1);
    face_repaint_ask(&ctx)
}

#[test]
fn iris_rests_asks_for_the_face_frame_while_signal_flows_and_rests_again() {
    let (mut lumen, device) = launched("iris-cadence");
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
            "a resting Iris asks for nothing, frame {frame}"
        );
    }

    let spectrum: Vec<f32> = (0..479).map(|i| -90.0 + (i % 60) as f32).collect();
    lumen.app_mut().pretend_track_level(Some(-6.0));
    lumen.app_mut().pretend_spectrum(device, &spectrum);
    lumen.frames(10);
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            Some(face_frame()),
            "a live Iris asks for the face frame, frame {frame}"
        );
    }

    lumen.app_mut().pretend_track_level(None);
    lumen.app_mut().clear_pretend_spectrum();
    lumen.run_until("Iris to go to sleep", |_| {
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
