//! The silent test devices (`--audio test:fake`, the E2E suite): they run
//! the engine without sound hardware, and some have a driver's faults for
//! the device-change scenarios (ADR-0113). None is in `DRIVERS`, so users
//! never see them.

use std::time::Duration;

use lumen_audio::Backend;
use lumen_audio::fake::Pace;

use super::MakeBackend;

/// The silent test devices, listed instead of [`DRIVERS`](super::DRIVERS) while one of
/// them runs (the E2E suite), so scenarios choose them as a user would and
/// never touch the machine's hardware: (key, label).
pub(super) const TEST_DRIVERS: &[(&str, &str)] = &[
    (TEST_DRIVER, "Test device"),
    (TEST_PACED_DRIVER, "Test device in real time"),
    (TEST_INPUT_DRIVER, "Test device with inputs"),
    (TEST_REFUSES_DRIVER, "Test device that refuses"),
    (TEST_SLOW_DRIVER, "Slow test device"),
    (TEST_HOLDS_DRIVER, "Test device that holds on"),
    (TEST_LATE_DRIVER, "Test device that answers late"),
];

/// The backend of test driver `driver`.
pub(super) fn backend(driver: &str) -> Option<MakeBackend> {
    match driver {
        // A silent device for automated tests (`--audio test:fake`, the E2E
        // suite): it runs the engine a block every few milliseconds without
        // sound hardware. Not in `DRIVERS`, so users never see it.
        TEST_DRIVER => Some(|| test_backend(Pace::Every(TEST_BLOCK_EVERY))),
        // The same, in real time (see `FakeDevice::pace`): for scenarios that
        // map wall-clock stamps to frames (recording).
        TEST_PACED_DRIVER => Some(|| test_backend(Pace::RealTime)),
        // The silent test device whose inputs a scenario plays (one per
        // process: the scenarios that use it take turns).
        TEST_INPUT_DRIVER => Some(|| Box::new(lumen_audio::fake::FakeBackend(input_device()))),
        // Test devices with a driver's faults, for the device-change
        // scenarios (ADR-0113): one that will not start, one slow to start,
        // one that holds on to the audio when left, and one that does not
        // answer within `NOT_ANSWERING`.
        TEST_REFUSES_DRIVER => Some(|| faulty_test_backend(|d| d.refuse_open = true)),
        TEST_SLOW_DRIVER => Some(|| faulty_test_backend(|d| d.open_delay = TEST_SLOW_OPEN)),
        TEST_HOLDS_DRIVER => Some(|| faulty_test_backend(|d| d.close_delay = TEST_HOLD_CLOSE)),
        TEST_LATE_DRIVER => Some(|| faulty_test_backend(|d| d.open_delay = TEST_LATE_OPEN)),
        _ => None,
    }
}

/// How long the slow test device takes to start.
const TEST_SLOW_OPEN: Duration = Duration::from_secs(3);
/// How long the late test device takes to start: past [`NOT_ANSWERING`](super::NOT_ANSWERING).
const TEST_LATE_OPEN: Duration = Duration::from_secs(20);
/// How long the holding test device takes to close: far past the handover.
const TEST_HOLD_CLOSE: Duration = Duration::from_secs(60);

/// The silent test device with `fault`.
fn faulty_test_backend(fault: fn(&mut lumen_audio::fake::FakeDevice)) -> Box<dyn Backend> {
    let control = lumen_audio::fake::FakeControl::new();
    control.set(|device| {
        device.pace = Some(Pace::Every(TEST_BLOCK_EVERY));
        fault(device);
    });
    Box::new(lumen_audio::fake::FakeBackend(control))
}

/// How often the silent test device runs a block (the supervisor polls every
/// 10 ms): a busy machine that polls late gets the blocks it missed.
const TEST_BLOCK_EVERY: Duration = Duration::from_millis(10);

fn test_backend(pace: Pace) -> Box<dyn Backend> {
    let control = lumen_audio::fake::FakeControl::new();
    control.set(|device| device.pace = Some(pace));
    Box::new(lumen_audio::fake::FakeBackend(control))
}

/// The one device behind [`TEST_INPUT_DRIVER`]: a scenario sets its
/// `input_levels` while the stream runs. Scenarios that script it take
/// [`input_device_turn`], which hands it over clean.
pub fn input_device() -> lumen_audio::fake::FakeControl {
    static DEVICE: std::sync::OnceLock<lumen_audio::fake::FakeControl> = std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let control = lumen_audio::fake::FakeControl::new();
            as_a_test_device(&control);
            control
        })
        .clone()
}

/// What makes a fake device the test input device: its slow, steady pace.
fn as_a_test_device(control: &lumen_audio::fake::FakeControl) {
    control.set(|device| device.pace = Some(Pace::Every(TEST_BLOCK_EVERY)));
}

/// A scenario's turn on the input test device, which every scenario that
/// plays it shares (it is one device per process). Held for the scenario's
/// life: the next one waits for it to be dropped.
pub struct InputDeviceTurn {
    device: lumen_audio::fake::FakeControl,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl InputDeviceTurn {
    /// Scripts the device (`FakeDevice::input_levels`, `input_ramp`, ...).
    pub fn set(&self, f: impl FnOnce(&mut lumen_audio::fake::FakeDevice)) {
        self.device.set(f);
    }
}

/// Waits for the input test device and takes it, put back as it first was:
/// nothing a scenario before this one scripted (a ramp, levels, a missing
/// device) is left on it, whichever scenario ran before and whatever it set.
pub fn input_device_turn() -> InputDeviceTurn {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A scenario that failed holds the turn while it unwinds; the device is
    // reset below, so a poisoned turn is as good as a free one.
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let device = input_device();
    device.reset();
    as_a_test_device(&device);
    InputDeviceTurn {
        device,
        _turn: turn,
    }
}

/// Whether `driver` is one of the silent test devices.
pub(super) fn is_test(driver: &str) -> bool {
    TEST_DRIVERS.iter().any(|(key, _)| *key == driver)
}

/// The silent test device's driver key.
pub const TEST_DRIVER: &str = "test";
/// The silent test device that keeps to real time.
pub const TEST_PACED_DRIVER: &str = "test-paced";
/// The silent test device with inputs a scenario plays ([`input_device`]).
pub const TEST_INPUT_DRIVER: &str = "test-input";
/// The silent test device that refuses to start.
pub const TEST_REFUSES_DRIVER: &str = "test-refuses";
/// The silent test device that takes [`TEST_SLOW_OPEN`] to start.
pub const TEST_SLOW_DRIVER: &str = "test-slow";
/// The silent test device that does not let go of the audio in time.
pub const TEST_HOLDS_DRIVER: &str = "test-holds";
/// The silent test device that takes [`TEST_LATE_OPEN`] to start.
pub const TEST_LATE_DRIVER: &str = "test-late";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_on_the_input_device_starts_from_a_clean_one() {
        // A scenario scripts everything it can and ends, as one that fails does.
        {
            let turn = input_device_turn();
            turn.set(|d| {
                d.input_ramp = true;
                d.input_levels = vec![0.5, 0.25];
                d.present = false;
                d.inputs = 8;
                d.pace = None;
            });
        }
        // The next one finds none of it.
        let turn = input_device_turn();
        let device = input_device();
        let mut seen = None;
        device.set(|d| seen = Some((d.input_ramp, d.input_levels.clone(), d.present, d.inputs)));
        assert_eq!(seen, Some((false, Vec::new(), true, 2)));
        // And it is still the slow, steady test device.
        device.set(|d| assert_eq!(d.pace, Some(Pace::Every(TEST_BLOCK_EVERY))));
        drop(turn);
    }
}
