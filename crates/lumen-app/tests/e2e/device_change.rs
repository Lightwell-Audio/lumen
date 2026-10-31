//! A device change never stops the window (ADR-0113): the lookup and a
//! switch of driver model run on a worker, and the device controls are
//! disabled until the device answers. Test devices stand in for drivers
//! that refuse to start, are slow to start, hold on to the audio when left,
//! or do not answer at all.

use std::time::{Duration, Instant};

use eframe::App as _;
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_app::testing::{TEST_DRIVER, TEST_HOLDS_DRIVER, TEST_LATE_DRIVER, TEST_SLOW_DRIVER};
use lumen_project::testing::TempDir;

use super::hosting::timing_sensitive;
use super::{Lumen, PATIENCE};

/// The longest a frame (or a click's few frames) may take while a change
/// runs. A blocked window took the whole handover, ten seconds; a loaded
/// machine draws a debug frame in well under this. The scenarios that bound
/// it take the `timing_sensitive` turn, so no plugin host starves them.
const LONGEST_FRAME: Duration = Duration::from_secs(2);

/// How long a driver gets before the row says it is not answering (and a
/// switch gives up on its handover), less a little for the clock's grain.
const NOT_ANSWERING: Duration = Duration::from_millis(9_500);

/// Clicks `label`, failing if the window stopped drawing meanwhile.
#[track_caller]
fn click_drawing(lumen: &mut Lumen, label: &str) {
    let started = Instant::now();
    lumen.click(label);
    let took = started.elapsed();
    assert!(took < LONGEST_FRAME, "clicking {label:?} took {took:?}");
}

/// Picks `option` from the dropdown named `button`, opening its list until
/// the option shows (a click while the window lays out can miss).
#[track_caller]
fn pick(lumen: &mut Lumen, button: &str, option: &str) {
    lumen.run_until(&format!("the {button:?} menu"), |l| l.shows(button));
    let started = Instant::now();
    while !lumen.shows(option) {
        lumen.click(button);
        assert!(started.elapsed() < PATIENCE, "{option:?} was never offered");
        std::thread::sleep(Duration::from_millis(5));
    }
    click_drawing(lumen, option);
}

/// Chooses the test device on the driver labelled `driver` in the Audio
/// page, as a user does: the driver, then the device.
#[track_caller]
fn choose(lumen: &mut Lumen, shown: &str, driver: &str) {
    pick(lumen, &format!("Driver: {shown}"), driver);
    pick(lumen, "Device: Choose a device", "Fake device");
}

/// Runs frames until `done`, failing if any frame took longer than
/// [`LONGEST_FRAME`]: the window kept drawing throughout. Returns how long
/// it took.
#[track_caller]
fn drawing_until(lumen: &mut Lumen, what: &str, done: impl Fn(&Lumen) -> bool) -> Duration {
    let started = Instant::now();
    loop {
        let frame = Instant::now();
        lumen.frames(1);
        let took = frame.elapsed();
        assert!(
            took < LONGEST_FRAME,
            "a frame took {took:?} while waiting for {what}"
        );
        if done(lumen) {
            return started.elapsed();
        }
        assert!(started.elapsed() < PATIENCE, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether the control whose name starts with `prefix` is disabled.
#[track_caller]
fn disabled(lumen: &Lumen, prefix: &str) -> bool {
    lumen
        .harness
        .get_by_label_contains(prefix)
        .accesskit_node()
        .is_disabled()
}

/// Quits as the window's close does (eframe calls `on_exit`, then drops the
/// app), and says how long it took.
fn quit(mut lumen: Lumen) -> Duration {
    let started = Instant::now();
    lumen.app_mut().on_exit();
    drop(lumen);
    started.elapsed()
}

#[test]
fn a_driver_that_refuses_to_start_goes_back_to_the_setup_before() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start("change-refused");
    lumen.open_audio_settings();
    choose(&mut lumen, "Test device", "Test device that refuses");
    drawing_until(&mut lumen, "the setup before to run again", |l| {
        l.app().audio_running()
            && l.app().audio_driver() == TEST_DRIVER
            && l.app().audio_can_change()
    });
    let notice = "Fake device could not start. Still using Fake device.";
    assert_eq!(lumen.app().audio_notice(), Some(notice));
    lumen.run_until("the notice on the page", |l| l.shows(notice));
    assert!(
        !disabled(&lumen, "Device: "),
        "the device can be chosen again"
    );
}

#[test]
fn a_slow_driver_keeps_the_window_drawing_and_refuses_a_second_choice() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start("change-slow");
    lumen.open_audio_settings();
    choose(&mut lumen, "Test device", "Slow test device");
    // The device takes three seconds to start. Meanwhile the controls are
    // disabled, and a second choice below them is refused.
    drawing_until(&mut lumen, "the device controls to be disabled", |l| {
        disabled(l, "Device: ") && disabled(l, "Driver: ")
    });
    assert!(!lumen.app().audio_can_change());
    assert!(
        !lumen.app_mut().choose_audio_driver(TEST_DRIVER),
        "a second change waits for the first"
    );
    lumen.run_until("the change in progress shown", |l| {
        l.shows("Starting Fake device…")
    });
    drawing_until(&mut lumen, "the slow device to run", |l| {
        l.app().audio_running() && l.app().audio_driver() == TEST_SLOW_DRIVER
    });
    drawing_until(&mut lumen, "the controls enabled again", |l| {
        !disabled(l, "Device: ")
    });
    assert!(lumen.app().audio_can_change());
    assert_eq!(lumen.app().audio_notice(), None);
}

#[test]
fn a_driver_that_holds_on_to_the_audio_says_so_after_ten_seconds() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start_with(TempDir::new("e2e-change-holds"), |l| {
        l.on_audio_driver(TEST_HOLDS_DRIVER)
    });
    lumen.open_audio_settings();
    choose(&mut lumen, "Test device that holds on", "Test device");
    lumen.run_until("the old driver closing", |l| {
        l.shows("Closing Test device that holds on…")
    });
    let message = "Test device that holds on did not let go of the audio. Save your work and \
                   restart Lumen.";
    let took = drawing_until(&mut lumen, "the handover to give up", |l| {
        l.app().audio_off_reason() == Some(message)
    });
    assert!(took >= NOT_ANSWERING, "it gave up after {took:?}");
    lumen.run_until("the message on the page", |l| l.shows(message));
    // Audio cannot come back in this run: nothing offers to change it.
    assert!(!lumen.app().audio_can_change());
    assert!(!lumen.app_mut().choose_audio_driver(TEST_DRIVER));
    assert!(disabled(&lumen, "Driver: "));
}

#[test]
fn a_driver_that_does_not_answer_says_so_after_ten_seconds_and_quitting_does_not_wait() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start("change-late");
    lumen.open_audio_settings();
    choose(&mut lumen, "Test device", "Test device that answers late");
    let message = "Test device that answers late is not answering. Save your work and restart \
                   Lumen.";
    let took = drawing_until(&mut lumen, "the row to say it is not answering", |l| {
        l.shows(message)
    });
    assert!(took >= NOT_ANSWERING, "it said so after {took:?}");
    assert!(!lumen.app().audio_running());
    // Audio On/Off waits for the device too: a stop sent now would queue
    // behind the open and free the lock while the driver still works.
    assert!(!lumen.app().audio_can_switch());
    lumen.frames(1);
    assert!(lumen.shows(message) && !lumen.app().audio_can_change());
    // The device still has not answered: quitting leaves it behind.
    let took = quit(lumen);
    assert!(took < LONGEST_FRAME, "quitting took {took:?}");
}

#[test]
fn quitting_while_a_switch_waits_for_its_handover_does_not_wait() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start_with(TempDir::new("e2e-change-quit"), |l| {
        l.on_audio_driver(TEST_HOLDS_DRIVER)
    });
    lumen.open_audio_settings();
    choose(&mut lumen, "Test device that holds on", "Test device");
    lumen.run_until("the worker waiting for the handover", |l| {
        l.shows("Closing Test device that holds on…")
    });
    let took = quit(lumen);
    assert!(took < LONGEST_FRAME, "quitting took {took:?}");
}

#[test]
fn quitting_with_a_finished_switch_unread_does_not_wait_for_the_new_device() {
    let _turn = timing_sensitive();
    let mut lumen = Lumen::start("change-unread");
    lumen.wait_for("audio to run", |app| app.audio_running());
    // Below the page, so no frame reads the worker's answer: the switch is
    // done and the new device is still opening (for twenty seconds).
    assert!(lumen.app_mut().choose_audio_driver(TEST_LATE_DRIVER));
    let started = Instant::now();
    while !lumen.app_mut().audio_switched_unread() {
        assert!(started.elapsed() < PATIENCE, "the switch never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = quit(lumen);
    assert!(took < LONGEST_FRAME, "quitting took {took:?}");
}

#[test]
fn every_test_driver_with_a_fault_is_offered_beside_the_test_device() {
    let mut lumen = Lumen::start("change-offered");
    lumen.open_audio_settings();
    let button = "Driver: Test device";
    lumen.run_until("the driver menu", |l| l.shows(button));
    let started = Instant::now();
    while !lumen.shows("Slow test device") {
        lumen.click(button);
        assert!(started.elapsed() < PATIENCE, "the list never opened");
    }
    for label in [
        "Test device that refuses",
        "Slow test device",
        "Test device that holds on",
        "Test device that answers late",
    ] {
        assert!(lumen.shows(label), "{label} is offered");
    }
    // None of the machine's own drivers is, so no scenario reaches hardware.
    assert!(!lumen.shows("ASIO") && !lumen.shows("System audio"));
}
