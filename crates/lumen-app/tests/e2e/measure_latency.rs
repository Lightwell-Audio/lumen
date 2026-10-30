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

/// Starts on the input device at real-time pace, a cable from output 1 to
/// input 2 if `cable`, and opens the helper from the command palette.
fn launch(name: &str, cable: bool) -> (Lumen, InputDeviceTurn) {
    let turn = lumen_app::testing::input_device_turn();
    turn.set(|d| {
        d.pace = Some(Pace::RealTime);
        d.cable = cable.then_some(Cable {
            output: 0,
            input: 1,
            delay: DELAY,
            gain: 0.5,
        });
    });
    let dir = TempDir::new(&format!("e2e-{name}"));
    let mut lumen = Lumen::start_with(dir, |l| l.with_input_device());
    lumen.wait_for("the device's inputs", |app| {
        app.audio_running() && app.open_inputs() == 2
    });
    palette(&mut lumen, "Measure recording latency");
    lumen.run_until("the helper's card", |l| l.shows("Cable to: 1  In 1"));
    (lumen, turn)
}

/// Plugs the card's input end into input 2.
fn to_input_2(lumen: &mut Lumen) {
    lumen.click("Cable to: 1  In 1");
    lumen.click("2  In 2");
    lumen.run_until("input 2 chosen", |l| l.shows("Cable to: 2  In 2"));
}

#[test]
fn a_cable_measures_the_round_trip_and_one_click_makes_it_the_offset() {
    let (mut lumen, _turn) = launch("measure-latency", true);
    assert_eq!(lumen.app().record_offset(), (0.0, None));
    to_input_2(&mut lumen);
    lumen.click("Measure");
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
    let (mut lumen, _turn) = launch("measure-no-cable", false);
    lumen.click("Measure");
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
    let (mut lumen, _turn) = launch("measure-held", true);
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
