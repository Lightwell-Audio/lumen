//! Recovery behaviour of the audio supervisor, against the scriptable backend.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lumen_audio::fake::{FakeBackend, FakeControl, fake_device_id};
use lumen_audio::{AudioError, AudioSystem, Block, DeviceEvent, State, StreamConfig, Timing};

const WAIT: Duration = Duration::from_secs(5);

fn fast() -> Timing {
    Timing {
        retry_interval: Duration::from_millis(20),
        poll_interval: Duration::from_millis(2),
        restore_confirm: Duration::from_millis(30),
    }
}

fn config(buffer: Option<u32>) -> StreamConfig {
    StreamConfig {
        device: fake_device_id(),
        sample_rate: None,
        buffer_frames: buffer,
        input_channels: vec![0, 1],
        output_channels: vec![0, 1],
    }
}

/// A callback that writes a tone and counts blocks.
fn system(control: &FakeControl) -> (AudioSystem, Arc<AtomicU64>) {
    let blocks = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&blocks);
    let callback = move |block: &mut Block<'_>| {
        for sample in block.output(0) {
            *sample = 0.25;
        }
        counter.fetch_add(1, Ordering::Relaxed);
    };
    let backend_control = control.clone();
    let audio = AudioSystem::spawn(
        move || FakeBackend(backend_control),
        Box::new(callback),
        fast(),
    );
    (audio, blocks)
}

fn wait_for_blocks(blocks: &AtomicU64, more_than: u64) {
    let started = Instant::now();
    while blocks.load(Ordering::Relaxed) <= more_than {
        assert!(started.elapsed() < WAIT, "audio did not run");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn starts_and_runs_the_callback() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(Some(256)));
    match audio.wait_event(WAIT) {
        Some(DeviceEvent::Started(info)) => assert_eq!(info.buffer_frames, 256),
        other => panic!("expected Started, got {other:?}"),
    }
    wait_for_blocks(&blocks, 10);
    assert_eq!(audio.status().state(), State::Running);
    assert!(audio.status().callbacks() > 10);
}

#[test]
fn a_driver_reset_reopens_the_stream() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    control.set(|d| d.reset_pending = true);
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Reset(_))
    ));
    let before = blocks.load(Ordering::Relaxed);
    wait_for_blocks(&blocks, before + 5);
    assert_eq!(control.opens(), 2);
}

#[test]
fn an_unplugged_device_is_reported_lost_and_restored_when_it_returns() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));

    control.set(|d| d.present = false);
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Lost(AudioError::DeviceNotPresent(_)))
    ));
    assert_eq!(audio.status().state(), State::Lost);
    // While lost, retries keep failing quietly and no audio runs.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(audio.try_event(), None);
    let frozen = blocks.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(blocks.load(Ordering::Relaxed), frozen);

    control.set(|d| d.present = true);
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Restored(_))
    ));
    assert_eq!(audio.status().state(), State::Running);
    wait_for_blocks(&blocks, frozen + 5);
}

/// MT-AUDIO-1 on a Focusrite interface: the driver accepted a reopen while
/// the hardware was still gone, then failed at once. That must not show up
/// as a Restored/Lost flip-flop.
#[test]
fn a_reopen_that_fails_at_once_is_not_reported_restored() {
    let control = FakeControl::new();
    let (audio, _) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    control.set(|d| d.present = false);
    assert!(matches!(audio.wait_event(WAIT), Some(DeviceEvent::Lost(_))));

    control.set(|d| d.phantom = true);
    let opens = control.opens();
    let started = Instant::now();
    while control.opens() < opens + 3 {
        assert!(started.elapsed() < WAIT, "retries stopped");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(audio.try_event(), None, "phantom reopens must stay quiet");
    assert_eq!(audio.status().state(), State::Lost);

    control.set(|d| d.present = true);
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Restored(_))
    ));
    assert_eq!(audio.status().state(), State::Running);
}

#[test]
fn a_stalled_driver_counts_as_lost_and_recovers() {
    let control = FakeControl::new();
    let (audio, _) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    control.set(|d| d.stalled = true);
    assert!(matches!(audio.wait_event(WAIT), Some(DeviceEvent::Lost(_))));
    control.set(|d| d.stalled = false);
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Restored(_))
    ));
}

#[test]
fn a_bad_configuration_falls_back_to_the_working_one() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(Some(256)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    audio.configure(config(Some(100))); // not a power of two: unsupported
    match audio.wait_event(WAIT) {
        Some(DeviceEvent::ConfigFailed {
            error: AudioError::Unsupported(_),
            reverted: true,
        }) => {}
        other => panic!("expected a reverted ConfigFailed, got {other:?}"),
    }
    assert_eq!(audio.status().state(), State::Running);
    let before = blocks.load(Ordering::Relaxed);
    wait_for_blocks(&blocks, before + 5);
}

#[test]
fn a_first_configuration_on_a_missing_device_fails_cleanly() {
    let control = FakeControl::new();
    control.set(|d| d.present = false);
    let (audio, _) = system(&control);
    audio.configure(config(None));
    match audio.wait_event(WAIT) {
        Some(DeviceEvent::ConfigFailed {
            error: AudioError::DeviceNotPresent(_),
            reverted: false,
        }) => {}
        other => panic!("expected ConfigFailed, got {other:?}"),
    }
    assert_eq!(audio.status().state(), State::Stopped);
}

#[test]
fn audio_off_stops_the_callback() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    wait_for_blocks(&blocks, 3);
    audio.stop();
    assert_eq!(audio.wait_event(WAIT), Some(DeviceEvent::Stopped));
    let stopped_at = blocks.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(blocks.load(Ordering::Relaxed), stopped_at);
    assert_eq!(audio.status().state(), State::Stopped);
}

#[test]
fn load_is_measured() {
    let control = FakeControl::new();
    let (audio, blocks) = system(&control);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    wait_for_blocks(&blocks, 20);
    let load = audio.status().take_peak_load();
    assert!(
        (0.0..1.0).contains(&load),
        "a trivial callback should use well under the budget: {load}"
    );
    assert_eq!(audio.status().overloads(), 0);
}
