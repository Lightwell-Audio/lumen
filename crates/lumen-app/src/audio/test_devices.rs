use std::time::Duration;

use lumen_audio::Backend;
use lumen_audio::fake::Pace;

use super::MakeBackend;

pub(super) const TEST_DRIVERS: &[(&str, &str)] = &[
    (TEST_DRIVER, "Test device"),
    (TEST_PACED_DRIVER, "Test device in real time"),
    (TEST_INPUT_DRIVER, "Test device with inputs"),
    (TEST_REFUSES_DRIVER, "Test device that refuses"),
    (TEST_SLOW_DRIVER, "Slow test device"),
    (TEST_HOLDS_DRIVER, "Test device that holds on"),
    (TEST_LATE_DRIVER, "Test device that answers late"),
    (
        TEST_BLOCKED_INPUT_DRIVER,
        "Test device with a blocked input",
    ),
];

pub(super) fn backend(driver: &str) -> Option<MakeBackend> {
    match driver {
        TEST_DRIVER => Some(|| test_backend(Pace::Every(TEST_BLOCK_EVERY))),
        TEST_PACED_DRIVER => Some(|| test_backend(Pace::RealTime)),
        TEST_INPUT_DRIVER => Some(|| Box::new(lumen_audio::fake::FakeBackend(input_device()))),
        TEST_REFUSES_DRIVER => Some(|| faulty_test_backend(|d| d.refuse_open = true)),
        TEST_SLOW_DRIVER => Some(|| faulty_test_backend(|d| d.open_delay = TEST_SLOW_OPEN)),
        TEST_HOLDS_DRIVER => Some(|| faulty_test_backend(|d| d.close_delay = TEST_HOLD_CLOSE)),
        TEST_LATE_DRIVER => Some(|| faulty_test_backend(|d| d.open_delay = TEST_LATE_OPEN)),
        TEST_BLOCKED_INPUT_DRIVER => {
            Some(|| faulty_test_backend(|d| d.silent_inputs = vec![BLOCKED_INPUT.to_owned()]))
        }
        _ => None,
    }
}

const TEST_SLOW_OPEN: Duration = Duration::from_secs(3);
const TEST_LATE_OPEN: Duration = Duration::from_secs(20);
const TEST_HOLD_CLOSE: Duration = Duration::from_secs(60);

fn faulty_test_backend(fault: fn(&mut lumen_audio::fake::FakeDevice)) -> Box<dyn Backend> {
    let control = lumen_audio::fake::FakeControl::new();
    control.set(|device| {
        device.pace = Some(Pace::Every(TEST_BLOCK_EVERY));
        fault(device);
    });
    Box::new(lumen_audio::fake::FakeBackend(control))
}

const TEST_BLOCK_EVERY: Duration = Duration::from_millis(10);

fn test_backend(pace: Pace) -> Box<dyn Backend> {
    let control = lumen_audio::fake::FakeControl::new();
    control.set(|device| device.pace = Some(pace));
    Box::new(lumen_audio::fake::FakeBackend(control))
}

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

fn as_a_test_device(control: &lumen_audio::fake::FakeControl) {
    control.set(|device| device.pace = Some(Pace::Every(TEST_BLOCK_EVERY)));
}

pub struct InputDeviceTurn {
    device: lumen_audio::fake::FakeControl,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl InputDeviceTurn {
    pub fn set(&self, f: impl FnOnce(&mut lumen_audio::fake::FakeDevice)) {
        self.device.set(f);
    }
}

pub fn input_device_turn() -> InputDeviceTurn {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let device = input_device();
    device.reset();
    as_a_test_device(&device);
    InputDeviceTurn {
        device,
        _turn: turn,
    }
}

pub(super) fn is_test(driver: &str) -> bool {
    TEST_DRIVERS.iter().any(|(key, _)| *key == driver)
}

pub const TEST_DRIVER: &str = "test";
pub const TEST_PACED_DRIVER: &str = "test-paced";
pub const TEST_INPUT_DRIVER: &str = "test-input";
pub const TEST_REFUSES_DRIVER: &str = "test-refuses";
pub const TEST_SLOW_DRIVER: &str = "test-slow";
pub const TEST_HOLDS_DRIVER: &str = "test-holds";
pub const TEST_LATE_DRIVER: &str = "test-late";
pub const TEST_BLOCKED_INPUT_DRIVER: &str = "test-blocked-input";
pub const BLOCKED_INPUT: &str = "Test microphone";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_on_the_input_device_starts_from_a_clean_one() {
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
        let turn = input_device_turn();
        let device = input_device();
        let mut seen = None;
        device.set(|d| seen = Some((d.input_ramp, d.input_levels.clone(), d.present, d.inputs)));
        assert_eq!(seen, Some((false, Vec::new(), true, 2)));
        device.set(|d| assert_eq!(d.pace, Some(Pace::Every(TEST_BLOCK_EVERY))));
        drop(turn);
    }
}
