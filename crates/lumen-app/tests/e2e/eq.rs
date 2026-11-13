//! Iris, the parametric EQ, in the app (M8 slice B1): the command that adds it,
//! the curve its face draws, dragging a band's point, typing a frequency on it,
//! a lane driving a band, and its power button.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_devices::eq::{self, freq_of, gain_of, on_of, q_of, type_of};
use lumen_project::automation::Target;
use lumen_project::model::{Id, PluginRef};

use super::Lumen;
use super::lane_view::{peak, run_command, tone};

/// The normalised value of `id` at `value` in its own units.
fn at(id: u32, value: f32) -> f64 {
    let info = eq::TABLE
        .iter()
        .find(|info| info.id == id)
        .expect("a parameter of the table");
    f64::from(info.normalised(value))
}

/// The chosen track's Iris, if it has one.
fn iris(lumen: &Lumen) -> Option<Id> {
    lumen.app().project().tracks[0]
        .devices
        .iter()
        .find(|d| matches!(&d.plugin, PluginRef::Builtin { name } if name == eq::NAME))
        .map(|d| d.id)
}

/// Starts with a tone on one audio track, selected, and Iris added to it by the
/// command (as a person runs it from the palette), with its whole card shown.
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
fn at_label(lumen: &Lumen, label: &str) -> Pos2 {
    lumen.harness.get_by_label_contains(label).rect().center()
}

fn param(lumen: &Lumen, device: Id, id: u32) -> Option<f64> {
    lumen
        .app()
        .device_params(device)
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, value)| *value)
}

/// What the app is playing now: the loudest frame of a moment.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(120), 1_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()))
}

/// The tone the test file plays is 382 Hz (the harness's `wav`), so a band put
/// there shapes what is heard.
const TONE: f32 = 382.0;

#[test]
fn the_command_puts_iris_on_the_track_with_a_curve_and_eight_bands() {
    let (lumen, device) = launched("eq-add");
    assert_eq!(lumen.app().undo_label(), Some("Add Iris"));
    assert!(lumen.app().is_builtin(device));
    // The card says the device's own name, and its kind under it.
    assert!(lumen.shows("Iris on tone"), "the card's spoken name");

    // The face draws eight bands, and every point says its band in words.
    assert!(
        lumen.shows("Band 3, bell, 250 Hz, 0.0 dB, Q 1.00"),
        "a bell's point"
    );
    assert!(
        lumen.shows("Band 1, low cut, off, 30.0 Hz, 24/oct"),
        "the low cut's point"
    );
    assert!(lumen.shows("Band 8, high cut, off, 18.0 kHz, 24/oct"));
    // The band chips are buttons, and every parameter is one button away.
    assert!(lumen.shows("Band 2 (bell)"));
    for band in 1..=8 {
        let on = lumen.is_on(&format!(
            "Band {band} ({})",
            if band == 1 {
                "low cut"
            } else if band == 8 {
                "high cut"
            } else {
                "bell"
            }
        ));
        assert_eq!(
            on,
            (2..=7).contains(&band),
            "band {band} is on at its defaults"
        );
    }
    assert!(
        lumen
            .harness
            .query_by_label("All parameters (38)")
            .is_some(),
        "every parameter is one button away"
    );
}

#[test]
fn dragging_a_point_changes_its_band_and_is_one_undo_step() {
    let (mut lumen, device) = launched("eq-drag");
    // The band where the tone is, so the shape is heard.
    lumen
        .app_mut()
        .set_device_param(device, freq_of(3), at(freq_of(3), TONE));
    lumen.frames(10);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let before = peak(&mut lumen);
    assert!(before > 0.0, "the tone is heard: {before}");

    // Drag the point up: the band's gain rises, and the tone with it.
    let point = at_label(&lumen, "Band 3, bell");
    lumen.drag(point, point + vec2(0.0, -40.0), Modifiers::NONE);
    lumen.frames(10);
    let gain = param(&lumen, device, gain_of(3)).expect("the band has a gain");
    // 40 pt up on a 176 pt screen of ±16 dB is about 7 dB.
    assert!(gain > 0.6, "dragged up: {gain}");
    assert_eq!(lumen.app().undo_label(), Some("Change 3 Freq"));
    let after = peak(&mut lumen);
    assert!(
        after > before * 1.2,
        "the boost is heard: {before} then {after}"
    );

    // One undo takes the whole drag back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(20);
    let back = param(&lumen, device, gain_of(3)).expect("the gain is still there");
    assert!(back < 0.55, "at its default again: {back}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_frequency_typed_on_a_point_is_read_at_once() {
    let (mut lumen, device) = launched("eq-type");
    // Pick the band, then open its field from the keyboard and type.
    lumen.harness.get_by_label_contains("Band 3, bell").click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(6);
    // Enter on a point opens the Freq dial's field, which is aimed at it.
    assert!(
        lumen
            .harness
            .query_by_label_contains("type a value")
            .is_some(),
        "the field is up"
    );
    lumen
        .harness
        .get_by_label_contains("type a value")
        .type_text("2k");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    let freq = param(&lumen, device, freq_of(3)).expect("the frequency is set");
    assert!((freq - at(freq_of(3), 2_000.0)).abs() < 0.01, "{freq}");
    assert!(
        lumen
            .harness
            .query_by_label_contains("Band 3, bell, 2.00 kHz")
            .is_some(),
        "the point says the frequency"
    );
    // What it does not understand changes nothing.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    assert!(
        (param(&lumen, device, freq_of(3)).unwrap() - at(freq_of(3), 250.0)).abs() < 0.01,
        "one undo step"
    );
}

#[test]
fn a_lane_driving_a_band_s_gain_is_heard_and_the_drag_takes_it_over() {
    let (mut lumen, device) = launched("eq-lane");
    lumen
        .app_mut()
        .set_device_param(device, freq_of(3), at(freq_of(3), TONE));
    lumen.frames(5);
    let track = lumen.app().project().tracks[0].id;
    let target = Target::Device {
        device,
        param: gain_of(3),
    };
    // +12 dB down to −24 dB over two quarters: the tone fades away.
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, at(gain_of(3), 12.0) as f32),
        Point::new(Ticks::quarters(2), at(gain_of(3), -24.0) as f32),
    ])
    .unwrap();
    lumen
        .app_mut()
        .give_lane(track, target, lane)
        .expect("the lane fits");
    lumen.frames(20);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(2));
    lumen.frames(30);
    let quiet = peak(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(quiet < 0.1, "the lane took the band's gain down: {quiet}");

    // Dragging the point takes the lane over: the face's own value is heard.
    let point = at_label(&lumen, "Band 3, bell");
    lumen.drag(point, point + vec2(0.0, 30.0), Modifiers::NONE);
    lumen.frames(20);
    assert!(lumen.app().lane_taken_over(track, target));
}

#[test]
fn the_power_button_bypasses_iris_and_the_tone_comes_back() {
    let (mut lumen, device) = launched("eq-power");
    lumen
        .app_mut()
        .set_device_param(device, freq_of(3), at(freq_of(3), TONE));
    lumen
        .app_mut()
        .set_device_param(device, gain_of(3), at(gain_of(3), 12.0));
    lumen
        .app_mut()
        .set_device_param(device, on_of(3), at(on_of(3), 1.0));
    lumen.frames(10);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let boosted = heard(&mut lumen);
    assert!(
        boosted > 0.0,
        "the tone is heard through the band: {boosted}"
    );

    lumen.click("Power Iris");
    lumen.frames(10);
    assert!(
        lumen.app().project().tracks[0]
            .devices
            .iter()
            .any(|d| d.id == device && d.bypass),
        "the power button bypassed it"
    );
    let dry = heard(&mut lumen);
    assert!(
        dry < boosted * 0.7,
        "the dry tone is back: {boosted} then {dry}"
    );
    lumen.click("Power Iris");
    lumen.frames(10);
    let again = heard(&mut lumen);
    assert!(again > dry * 1.3, "on again: {dry} then {again}");
    lumen.key(Modifiers::NONE, Key::Space);
}

/// The face's controls are the table's: a band's kind is a chooser in All
/// parameters, and turning a band off takes its shape off the curve.
#[test]
fn a_band_is_turned_off_and_on_again_from_its_chip() {
    let (mut lumen, device) = launched("eq-chip");
    // Band 2 is on at its defaults; a double-click on its chip turns it off.
    let chip = at_label(&lumen, "Band 2 (bell)");
    lumen.harness.hover_at(chip);
    lumen.frames(2);
    for _ in 0..2 {
        lumen.harness.event(Event::PointerButton {
            pos: chip,
            button: eframe::egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        lumen.harness.event(Event::PointerButton {
            pos: chip,
            button: eframe::egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        lumen.frames(1);
    }
    lumen.frames(10);
    assert_eq!(
        param(&lumen, device, on_of(2)),
        Some(0.0),
        "the chip turned the band off"
    );
    assert_eq!(lumen.app().undo_label(), Some("Change 2 On"));
    // Its point says so, and its kind is still the table's.
    assert!(lumen.shows("Band 2, bell, off, 100 Hz, 0.0 dB, Q 1.00"));
    assert_eq!(
        param(&lumen, device, type_of(2)),
        Some(at(type_of(2), 0.0)),
        "a bell"
    );
    assert_eq!(
        lumen.app().device_choices(device, type_of(2)),
        ["Bell", "Shelf low", "Shelf high", "Notch"]
    );
    let _ = (q_of(2), eq::ANALYZER);
}

/// A point dragged across and up moves its frequency and its gain together,
/// and one undo puts both back: the drag is one step, not one for each.
#[test]
fn a_point_dragged_across_and_up_is_one_undo_step() {
    let (mut lumen, device) = launched("eq-drag-both");
    let freq = param(&lumen, device, freq_of(3)).expect("a frequency");
    let gain = param(&lumen, device, gain_of(3)).expect("a gain");
    let point = at_label(&lumen, "Band 3, bell");
    lumen.drag(point, point + vec2(60.0, -30.0), Modifiers::NONE);
    lumen.frames(10);
    let (freq_after, gain_after) = (
        param(&lumen, device, freq_of(3)).unwrap(),
        param(&lumen, device, gain_of(3)).unwrap(),
    );
    assert!(freq_after > freq + 0.05, "across: {freq} then {freq_after}");
    assert!(gain_after > gain + 0.05, "up: {gain} then {gain_after}");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(20);
    assert_eq!(param(&lumen, device, freq_of(3)), Some(freq));
    assert_eq!(param(&lumen, device, gain_of(3)), Some(gain));
}

/// The card's three dials belong to the picked band: a pill picks one, and the
/// dials are then that band's Freq, Gain and Q (a cut has a Slope and no Q).
#[test]
fn a_pill_picks_the_band_the_dials_are_aimed_at() {
    let (mut lumen, device) = launched("eq-pick");
    let dial = |lumen: &Lumen, name: &str| lumen.harness.query_by_label_contains(name).is_some();
    // Band 2 is the first band that is on.
    assert!(dial(&lumen, "2 Freq") && dial(&lumen, "2 Gain") && dial(&lumen, "2 Q"));
    lumen.harness.get_by_label("Band 5 (bell)").click();
    lumen.frames(10);
    assert!(dial(&lumen, "5 Freq") && dial(&lumen, "5 Gain") && dial(&lumen, "5 Q"));
    assert!(!dial(&lumen, "2 Freq"), "the dials left band 2");
    // Picking is a view setting, not an edit: nothing to undo.
    assert_eq!(lumen.app().undo_label(), Some("Add Iris"));
    // A cut's second dial is its slope, and it has no Q to aim the third at.
    lumen.harness.get_by_label("Band 1 (low cut)").click();
    lumen.frames(10);
    assert!(dial(&lumen, "1 Freq") && dial(&lumen, "1 Slope"));
    assert!(!dial(&lumen, "1 Q"));
    // The dials set the picked band.
    let before = param(&lumen, device, freq_of(1)).unwrap();
    lumen.harness.get_by_label_contains("1 Freq").focus();
    lumen.key(Modifiers::NONE, Key::ArrowUp);
    lumen.frames(10);
    assert!(
        param(&lumen, device, freq_of(1)).unwrap() > before,
        "the Freq dial moved band 1"
    );
    // Clicking a point picks its band as well.
    lumen.harness.get_by_label_contains("Band 7, bell").click();
    lumen.frames(10);
    assert!(dial(&lumen, "7 Freq"));
}
