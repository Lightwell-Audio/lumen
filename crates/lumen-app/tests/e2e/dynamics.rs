//! Gravity and Horizon in the app (M8 slice B2): adding each from the command,
//! its drawn face and its four controls, a value typed on the face, a lane
//! driving one, a sidechain from another track ducking it, and the power
//! button.

use std::time::Duration;

use eframe::egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_devices::dynamics::compressor;
use lumen_devices::dynamics::limiter;
use lumen_project::automation::Target;
use lumen_project::model::{Id, PluginRef};

use super::Lumen;
use super::lane_view::{run_command, tone};

/// The built-in device a track has, by the saved name it is.
fn builtin(lumen: &Lumen, track: usize, name: &str) -> Option<Id> {
    lumen
        .app()
        .project()
        .tracks
        .get(track)?
        .devices
        .iter()
        .find(|d| matches!(&d.plugin, PluginRef::Builtin { name: saved } if saved == name))
        .map(|d| d.id)
}

/// Starts with a tone on one audio track, selected, and `command` run.
fn launched(name: &str, command: &str, saved: &str) -> (Lumen, Id, Id) {
    let (mut lumen, track) = tone(name);
    run_command(&mut lumen, command);
    lumen.wait_for("the card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    lumen.frames(20);
    let device = builtin(&lumen, 0, saved).expect("the device is on the track");
    (lumen, track, device)
}

#[track_caller]
fn at(lumen: &Lumen, label: &str) -> Pos2 {
    lumen.harness.get_by_label_contains(label).rect().center()
}

/// What the app is playing now: the loudest frame of a moment.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(120), 1_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()))
}

fn param(lumen: &Lumen, device: Id, id: u32) -> Option<f64> {
    lumen
        .app()
        .device_params(device)
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, value)| *value)
}

#[test]
fn the_commands_put_gravity_and_horizon_on_the_track_each_with_its_face() {
    let (mut lumen, _, gravity) = launched(
        "gravity-add",
        "Add Gravity",
        lumen_devices::dynamics::compressor::NAME,
    );
    assert_eq!(lumen.app().undo_label(), Some("Add Gravity"));
    assert!(lumen.app().is_builtin(gravity));
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(gravity)
    });
    // The card says the device's own name, with what it is under it.
    assert!(lumen.shows("Gravity on tone"), "the card's spoken name");
    // The four the face draws are the table's first four, and every other
    // parameter is one button away.
    assert_eq!(
        lumen.app().strip_parameters(gravity),
        [
            compressor::THRESHOLD,
            compressor::RATIO,
            compressor::ATTACK,
            compressor::RELEASE
        ]
    );
    assert!(lumen.shows("Pull, parameter: \u{2212}18.0 dB"));
    assert!(
        lumen
            .harness
            .query_by_label("All parameters (10)")
            .is_some(),
        "every parameter is one button away"
    );
    // Its meter is read, so its face can draw the gain reduction.
    assert!(lumen.app().has_meter(gravity));

    let (mut lumen, _, horizon) = launched(
        "horizon-add",
        "Add Horizon",
        lumen_devices::dynamics::limiter::NAME,
    );
    assert_eq!(lumen.app().undo_label(), Some("Add Horizon"));
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(horizon)
    });
    assert!(lumen.shows("Horizon on tone"));
    assert_eq!(
        lumen.app().strip_parameters(horizon),
        [
            limiter::CEILING,
            limiter::RELEASE,
            limiter::LOOK,
            limiter::DETECT
        ]
    );
    assert!(lumen.shows("Ceiling, parameter: \u{2212}0.3 dB"));
    assert!(lumen.harness.query_by_label("All parameters (4)").is_some());
    // Its lookahead is a latency the transport is compensated for.
    lumen.wait_for("the compensation", |app| app.compensation() >= 72);
}

#[test]
fn a_value_typed_on_the_face_is_read_at_once_and_is_one_undo_step() {
    let (mut lumen, _, device) = launched(
        "gravity-typed",
        "Add Gravity",
        lumen_devices::dynamics::compressor::NAME,
    );
    lumen
        .harness
        .get_by_label_contains("Pull, parameter")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Pull: type a value")
        .type_text("-40");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    let pulled = param(&lumen, device, compressor::THRESHOLD).expect("it moved");
    // −40 dB of −60 to 0.
    assert!((pulled - 0.3333).abs() < 1e-3, "{pulled}");
    assert_eq!(lumen.app().undo_label(), Some("Change Pull"));
    lumen.frames(5);
    assert!(lumen.shows("Pull, parameter: \u{2212}40.0 dB"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    let back = param(&lumen, device, compressor::THRESHOLD).expect("still there");
    assert!(back > 0.69, "{back}");
}

/// A lane of Horizon's ceiling, from the bottom to the top over two quarters.
#[test]
fn a_lane_drives_the_ceiling_and_a_drag_takes_it_over() {
    let (mut lumen, track, device) = launched(
        "horizon-lane",
        "Add Horizon",
        lumen_devices::dynamics::limiter::NAME,
    );
    let target = Target::Device {
        device,
        param: limiter::CEILING,
    };
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, 0.0),
        Point::new(Ticks::quarters(2), 1.0),
    ])
    .unwrap();
    lumen.app_mut().give_lane(track, target, lane).unwrap();
    lumen.frames(20);
    // The knob's own menu shows the lane it drives.
    lumen.right_click(at(&lumen, "Ceiling, parameter"));
    lumen.click("Automate");
    lumen.frames(30);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.frames(5);
    // The control follows the lane where the playhead is.
    let at_playhead = param(&lumen, device, limiter::CEILING).expect("a value");
    assert!(at_playhead > 0.4, "the lane lifted it: {at_playhead}");
    // Moving the knob takes the lane over: the control's own value is heard.
    let knob = at(&lumen, "Ceiling, parameter");
    lumen.drag(knob, knob + vec2(0.0, 30.0), Modifiers::NONE);
    lumen.frames(20);
    assert!(lumen.app().lane_taken_over(track, target));
}

/// A key from another track ducks Gravity: the chooser is the plugin card's
/// own row (one code path for both kinds of device), and the light follows
/// what arrives.
#[test]
fn a_sidechain_from_another_track_ducks_gravity() {
    let (mut lumen, first, device) = launched(
        "gravity-keyed",
        "Add Gravity",
        lumen_devices::dynamics::compressor::NAME,
    );
    // A second track with a loud tone, to key it from (a dropped file makes
    // its own track).
    let file = super::wav(&lumen.dir().join("key.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(30);
    let (_second, name) = lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| !t.clips.is_empty() && t.id != first)
        .map(|t| (t.id, t.name.clone()))
        .expect("the key's track");
    lumen.app_mut().select_tracks(&[first]);
    lumen.show_whole_cards();
    lumen.frames(20);

    // The row is Gravity's, with no source yet.
    lumen.click("Sidechain source, none");
    lumen.frames(5);
    lumen.click(&name);
    lumen.frames(10);
    let keyed = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.devices)
        .find(|d| d.id == device)
        .and_then(|d| d.sidechain);
    assert!(keyed.is_some(), "Gravity is keyed from {name}");
    assert_eq!(lumen.app().undo_label(), Some("Sidechain Gravity"));
    assert!(lumen.shows(&format!("Sidechain source, {name}, Post FX")));
    // The light follows the key arriving (it sits on the keyed track's card;
    // the test tone's own peak is 0.09).
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    lumen.wait_for("the key's light", |app| app.sidechain_light(first) > 0.02);
    lumen.key(Modifiers::NONE, Key::Space);
    // Undo lets it through again.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    let back = lumen
        .app()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.devices)
        .find(|d| d.id == device)
        .and_then(|d| d.sidechain);
    assert_eq!(back, None);
    assert!(lumen.shows("Sidechain source, none"));
}

/// The power button bypasses Horizon, and its dry signal comes back.
#[test]
fn the_power_button_bypasses_horizon_and_the_dry_signal_returns() {
    let (mut lumen, _, device) = launched(
        "horizon-power",
        "Add Horizon",
        lumen_devices::dynamics::limiter::NAME,
    );
    lumen.show_whole_cards();
    lumen.frames(10);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let before = heard(&mut lumen);
    assert!(before > 0.0, "the tone is heard: {before}");
    lumen.click("Power Horizon");
    lumen.frames(10);
    assert!(
        lumen.app().project().tracks[0]
            .devices
            .iter()
            .any(|d| d.id == device && d.bypass),
        "the power button bypassed it"
    );
    let after = heard(&mut lumen);
    // The ceiling is −0.3 dB and the tone is quiet, so the two are close: what
    // matters is that nothing steps or drops out.
    assert!(
        (after - before).abs() < before * 0.5,
        "bypassed, the dry signal is there: {before} then {after}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

/// The track's own menu adds Gravity, so it is reachable without the palette.
#[test]
fn the_track_menu_adds_gravity_to_that_track() {
    let (mut lumen, track) = tone("gravity-menu");
    lumen.frames(20);
    let header = lumen.harness.get_by_label_contains("Track tone,").rect();
    lumen.right_click(header.center());
    lumen.frames(5);
    assert!(lumen.shows("Add Gravity (compressor)"), "the row is there");
    assert!(lumen.shows("Add Horizon (limiter)"), "and the limiter's");
    lumen.click("Add Gravity (compressor)");
    lumen.wait_for("the card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    assert_eq!(lumen.app().undo_label(), Some("Add Gravity"));
    assert_eq!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map(|t| t.devices.len()),
        Some(1)
    );
}

/// The gain reduction the face draws comes from the audio: playing a loud
/// signal through Gravity publishes a reading the interface can see.
#[test]
fn gravity_publishes_the_gain_reduction_its_face_draws() {
    let (mut lumen, _, device) = launched(
        "gravity-meter",
        "Add Gravity",
        lumen_devices::dynamics::compressor::NAME,
    );
    // Pull the threshold down and the ratio up, so the quiet test tone is
    // compressed plainly.
    lumen.app_mut().set_device_param(
        device,
        compressor::THRESHOLD,
        f64::from(
            lumen_devices::params::find(compressor::TABLE, compressor::THRESHOLD)
                .expect("a parameter")
                .normalised(-60.0),
        ),
    );
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    lumen.wait_for("a reading on the meter", |app| {
        app.builtin_reduction(device) > 1.0
    });
    lumen.key(Modifiers::NONE, Key::Space);
}
