//! The loopback helper (M9 slice R2b, ADR-0110), end to end on the input
//! test device with a cable scripted from an output to an input: the command
//! opens the audio settings at the helper's card, Measure plays the clicks
//! and finds the round trip, and one click makes the difference from what
//! the driver reports the recording offset, remembering the setup it was
//! measured on. Without a cable, the card says nothing came back and what to
//! check. They share the process-wide input test device, so they take turns.

use egui_kittest::kittest::{NodeT, Queryable};
use lumen_app::testing::InputDeviceTurn;
use lumen_audio::fake::{Cable, Pace};
use lumen_project::testing::TempDir;

use super::Lumen;
use super::midi_play::palette;

/// The cable's length in frames. The test device reports 128 + 32 frames
/// in and 128 + 64 out at its 128-frame buffer: 352 in all.
const DELAY: u32 = 600;
/// (600 − 352) frames at 48 kHz, to a tenth of a millisecond.
const OFFSET: &str = "Use +5.2 ms";

/// Starts on the input device at real-time pace, a cable `delay` frames
/// long from output 1 to input 2 if there is one, and opens the helper
/// from the command palette.
fn launch(name: &str, delay: Option<u32>) -> (Lumen, InputDeviceTurn) {
    launch_at(name, delay, super::SIZE)
}

fn launch_at(name: &str, delay: Option<u32>, size: eframe::egui::Vec2) -> (Lumen, InputDeviceTurn) {
    let turn = lumen_app::testing::input_device_turn();
    turn.set(|d| {
        d.pace = Some(Pace::RealTime);
        d.cable = delay.map(|delay| Cable {
            output: 0,
            input: 1,
            delay,
            gain: 0.5,
        });
    });
    let dir = TempDir::new(&format!("e2e-{name}"));
    let mut lumen = Lumen::start_with(dir, |l| l.with_input_device());
    lumen.wait_for("the device's inputs", |app| {
        app.audio_running() && app.open_inputs() == 2
    });
    lumen.harness.set_size(size);
    palette(&mut lumen, "Measure recording latency");
    lumen.run_until("the helper's card", |l| l.shows("Cable to: 1  In 1"));
    (lumen, turn)
}

/// Scrolls to the control named `label` and clicks it until `done` holds:
/// a click while the page still scrolls lands where the control was.
#[track_caller]
fn press(lumen: &mut Lumen, label: &str, done: impl Fn(&Lumen) -> bool) {
    let started = std::time::Instant::now();
    while !done(lumen) {
        assert!(
            started.elapsed() < super::PATIENCE,
            "{label} never answered"
        );
        lumen.harness.get_by_label(label).scroll_to_me();
        lumen.frames(3);
        lumen.click(label);
    }
}

/// The card is revealed with an animated scroll. Seeing its labels does
/// not mean a click at their current position will land: keep bringing
/// Measure into view until the card acknowledges it. A very fast result
/// can already offer Measure again by the next frame.
fn measure(lumen: &mut Lumen) {
    press(lumen, "Measure", |l| {
        l.shows("Listening for the clicks") || l.shows("Measure again")
    });
}

/// Plugs the card's input end into input 2.
fn to_input_2(lumen: &mut Lumen) {
    lumen.click("Cable to: 1  In 1");
    lumen.click("2  In 2");
    lumen.run_until("input 2 chosen", |l| l.shows("Cable to: 2  In 2"));
}

#[test]
fn a_cable_measures_the_round_trip_and_one_click_makes_it_the_offset() {
    let (mut lumen, _turn) = launch("measure-latency", Some(DELAY));
    assert_eq!(lumen.app().record_offset(), (0.0, None));
    to_input_2(&mut lumen);
    measure(&mut lumen);
    lumen.run_until("the round trip", |l| l.shows(OFFSET));
    assert!(lumen.shows("Returned clicks: 5 of 5 agree"));
    assert!(
        lumen
            .harness
            .query_by_label_contains("600 samples · driver says 7.3 ms")
            .is_some()
    );
    // Nothing changes until the offer is taken.
    assert_eq!(lumen.app().record_offset(), (0.0, None));
    lumen.click(OFFSET);
    lumen.wait_for("the measured offset", |app| app.record_offset().1.is_some());
    let (ms, with) = lumen.app().record_offset();
    assert!((ms - 5.2).abs() < 1e-4, "{ms}");
    assert_eq!(with, Some(("test-input".into(), 48_000, 128)));
    assert!(lumen.shows("The recording offset is +5.2 ms"));
    // Measured on the setup running: nothing to say about it.
    assert!(
        lumen
            .harness
            .query_by_label_contains("Measured with")
            .is_none()
    );
}

#[test]
fn without_a_cable_the_card_says_what_to_check() {
    let (mut lumen, _turn) = launch("measure-no-cable", None);
    no_cable(&mut lumen);
}

#[test]
fn without_a_cable_in_a_short_window_the_card_says_what_to_check() {
    let (mut lumen, _turn) = launch_at(
        "measure-no-cable-short",
        None,
        eframe::egui::vec2(960.0, 540.0),
    );
    no_cable(&mut lumen);
}

fn no_cable(lumen: &mut Lumen) {
    measure(lumen);
    lumen.run_until("the failure", |l| {
        l.harness
            .query_by_label_contains("Nothing came back on input 1 (In 1)")
            .is_some()
    });
    assert_eq!(lumen.app().record_offset(), (0.0, None));
    assert!(lumen.shows("Measure again"));
}

#[test]
fn measuring_waits_for_playback_to_stop() {
    let (mut lumen, _turn) = launch("measure-held", Some(DELAY));
    lumen.key(eframe::egui::Modifiers::NONE, eframe::egui::Key::Space);
    lumen.run_until("the hold", |l| l.shows("Stop playback to measure"));
    assert!(
        lumen
            .harness
            .get_by_label("Measure")
            .accesskit_node()
            .is_disabled()
    );
    lumen.click("Measure");
    lumen.frames(10);
    assert!(!lumen.shows(OFFSET), "nothing measured while playing");
}

/// A round trip just as the driver reports it asks for no offset, but is
/// still offered: taking it remembers the setup the offset holds for.
#[test]
fn a_measured_offset_equal_to_the_current_one_is_still_offered() {
    let (mut lumen, _turn) = launch("measure-same", Some(352));
    to_input_2(&mut lumen);
    measure(&mut lumen);
    lumen.run_until("the round trip", |l| l.shows("Use +0.0 ms"));
    lumen.click("Use +0.0 ms");
    lumen.wait_for("the measured setup", |app| app.record_offset().1.is_some());
    assert_eq!(lumen.app().record_offset().0, 0.0);
    lumen.run_until("the offer taken", |l| {
        l.shows("The recording offset is +0.0 ms")
    });
}

/// A round trip measured before the buffer changed is not offered for the
/// new one: the card asks to measure again, and measuring again offers it.
#[test]
fn a_result_from_another_setup_asks_to_measure_again() {
    let (mut lumen, _turn) = launch("measure-changed", Some(DELAY));
    to_input_2(&mut lumen);
    measure(&mut lumen);
    lumen.run_until("the round trip", |l| l.shows(OFFSET));
    // Up the page.
    press(&mut lumen, "Buffer size: 128 samples", |l| {
        l.shows("64 samples")
    });
    lumen.click("64 samples");
    lumen.wait_for("the device at 64 samples", |app| {
        app.audio_running() && app.open_inputs() == 2
    });
    lumen.run_until("the card to ask again", |l| {
        l.harness
            .query_by_label_contains("changed since this was measured")
            .is_some()
    });
    assert!(!lumen.shows(OFFSET), "the old offer is gone");
    press(&mut lumen, "Measure again", |l| {
        l.shows("Listening for the clicks") || l.harness.query_by_label_contains("Use +").is_some()
    });
    lumen.run_until("an offer for this setup", |l| {
        l.harness.query_by_label_contains("Use +").is_some()
    });
}
