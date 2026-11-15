use egui_kittest::kittest::{NodeT, Queryable};
use lumen_app::testing::InputDeviceTurn;
use lumen_audio::fake::{Cable, Pace};
use lumen_project::testing::TempDir;

use super::Lumen;
use super::midi_play::palette;

const DELAY: u32 = 600;
const OFFSET: &str = "Use +5.2 ms";

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

#[track_caller]
fn press(lumen: &mut Lumen, label: &str, done: impl Fn(&Lumen) -> bool) {
    let started = std::time::Instant::now();
    while !done(lumen) {
        assert!(
            started.elapsed() < super::patience(),
            "{label} never answered"
        );
        lumen.harness.get_by_label(label).scroll_to_me();
        lumen.frames(3);
        lumen.click(label);
    }
}

fn measure(lumen: &mut Lumen) {
    press(lumen, "Measure", |l| {
        l.shows("Listening for the clicks") || l.shows("Measure again")
    });
}

fn to_input_2(lumen: &mut Lumen) {
    lumen.pick("Cable to: 1  In 1", "2  In 2", |l| {
        l.shows("Cable to: 2  In 2")
    });
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
    assert_eq!(lumen.app().record_offset(), (0.0, None));
    lumen.click(OFFSET);
    lumen.wait_for("the measured offset", |app| app.record_offset().1.is_some());
    let (ms, with) = lumen.app().record_offset();
    assert!((ms - 5.2).abs() < 1e-4, "{ms}");
    assert_eq!(with, Some(("test-input".into(), 48_000, 128)));
    assert!(lumen.shows("The recording offset is +5.2 ms"));
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

#[test]
fn a_result_from_another_setup_asks_to_measure_again() {
    let (mut lumen, _turn) = launch("measure-changed", Some(DELAY));
    to_input_2(&mut lumen);
    measure(&mut lumen);
    lumen.run_until("the round trip", |l| l.shows(OFFSET));
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
