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
        starvation_window: None,
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
    let time = audio.status().take_block_time();
    assert!(time.blocks >= 20, "every callback counted: {time:?}");
    assert!(
        time.block > Duration::ZERO && time.used < time.block,
        "{time:?}"
    );
}

/// The starvation check over 200 ms windows.
fn watching() -> Timing {
    Timing {
        starvation_window: Some(Duration::from_millis(200)),
        ..fast()
    }
}

fn spawn(control: &FakeControl, timing: Timing) -> AudioSystem {
    let backend_control = control.clone();
    AudioSystem::spawn(
        move || FakeBackend(backend_control),
        Box::new(|_: &mut Block<'_>| {}),
        timing,
    )
}

fn wait_for_dropouts(audio: &AudioSystem, at_least: u64) {
    let started = Instant::now();
    while audio.status().dropouts() < at_least {
        assert!(
            started.elapsed() < WAIT,
            "dropouts stayed at {}",
            audio.status().dropouts()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_driver_delivering_slower_than_real_time_counts_dropouts() {
    // 64 frames every 4th poll of 2 ms: about 8,000 frames a second at 48 kHz.
    let control = FakeControl::new();
    control.set(|d| d.deliver_every = 4);
    let audio = spawn(&control, watching());
    audio.configure(config(Some(64)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    wait_for_dropouts(&audio, 1);
    // Told apart from skipped buffers.
    assert_eq!(audio.status().slow_dropouts(), audio.status().dropouts());
}

#[test]
fn a_healthy_driver_counts_no_dropouts() {
    // 1,024 frames every poll: far faster than real time.
    let control = FakeControl::new();
    let audio = spawn(&control, watching());
    audio.configure(config(Some(1_024)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(audio.status().dropouts(), 0);
}

#[test]
fn skipped_driver_buffers_count_as_dropouts_and_reset_clears_them() {
    let control = FakeControl::new();
    let audio = spawn(&control, fast());
    audio.configure(config(Some(256)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    std::thread::sleep(Duration::from_millis(30));
    // The driver position jumps two buffers ahead: two buffers were skipped.
    control.set(|d| d.skip_frames = 512);
    wait_for_dropouts(&audio, 2);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(audio.status().dropouts(), 2);
    assert_eq!(audio.status().slow_dropouts(), 0);
    audio.status().reset_counters();
    assert_eq!(audio.status().dropouts(), 0);
}

#[test]
fn a_driver_reset_is_not_counted_as_dropouts() {
    // Owner (M4 slice 1): changing the Focusrite's buffer size added dozens
    // of dropouts each time. Its ASIO driver keeps counting samples through
    // the reset, so the reopened stream's position is far ahead of where
    // the last block ended; that gap is a restart, not lost audio.
    let control = FakeControl::new();
    let audio = spawn(&control, fast());
    audio.configure(config(Some(128)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    std::thread::sleep(Duration::from_millis(30));
    control.set(|d| {
        d.start_position = 10_000_000;
        d.reset_pending = true;
    });
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Reset(_))
    ));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(audio.status().dropouts(), 0);
    // Skipped buffers after the reset still count.
    control.set(|d| d.skip_frames = 256);
    wait_for_dropouts(&audio, 2);
}

#[test]
fn a_driver_resync_is_not_counted_as_dropouts() {
    // ASIO drivers ask the host to resync when their sample position jumps
    // (the block after reports none): a discontinuity, not lost audio.
    let control = FakeControl::new();
    let audio = spawn(&control, fast());
    audio.configure(config(Some(256)));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    std::thread::sleep(Duration::from_millis(30));
    control.set(|d| d.resync_jump = Some(5_000_000));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(audio.status().dropouts(), 0);
    control.set(|d| d.skip_frames = 512);
    wait_for_dropouts(&audio, 2);
}

#[test]
fn switching_drivers_moves_the_running_callback_to_the_new_one() {
    let (first, second) = (FakeControl::new(), FakeControl::new());
    let (mut audio, blocks) = system(&first);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    wait_for_blocks(&blocks, 5);

    let second_control = second.clone();
    audio
        .switch(
            move || FakeBackend(second_control),
            config(Some(512)),
            || {},
        )
        .unwrap();
    match audio.wait_event(WAIT) {
        Some(DeviceEvent::Started(info)) => assert_eq!(info.buffer_frames, 512),
        other => panic!("expected Started on the new driver, got {other:?}"),
    }
    // The same callback (its block counter) keeps running, now on the
    // second device.
    let before = blocks.load(Ordering::Relaxed);
    wait_for_blocks(&blocks, before + 5);
    assert_eq!(second.opens(), 1);
    assert_eq!(audio.status().state(), State::Running);
}

#[test]
fn after_a_switch_the_first_event_is_the_new_drivers() {
    let (first, second) = (FakeControl::new(), FakeControl::new());
    let (mut audio, _) = system(&first);
    // The old driver starts and says so; nobody reads it before the switch.
    audio.configure(config(Some(128)));
    let next = second.clone();
    audio
        .switch(move || FakeBackend(next), config(Some(512)), || {})
        .unwrap();
    match audio.wait_event(WAIT) {
        Some(DeviceEvent::Started(info)) => assert_eq!(info.buffer_frames, 512),
        other => panic!("expected Started on the new driver, got {other:?}"),
    }
    let left = audio.try_event();
    assert!(
        left.is_none(),
        "nothing of the old driver's is left: {left:?}"
    );
}

#[test]
fn a_switch_says_when_the_audio_is_handed_over_and_gives_up_on_a_driver_that_holds_on() {
    // A driver that lets go: `handed` is told before the new one opens.
    let (first, second) = (FakeControl::new(), FakeControl::new());
    let (mut audio, _) = system(&first);
    audio.configure(config(None));
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    let handed = std::cell::Cell::new(false);
    let next = second.clone();
    audio
        .switch(move || FakeBackend(next), config(None), || handed.set(true))
        .unwrap();
    assert!(handed.get());

    // One that does not: the switch gives up after the handover's deadline.
    assert!(matches!(
        audio.wait_event(WAIT),
        Some(DeviceEvent::Started(_))
    ));
    second.set(|d| d.close_delay = lumen_audio::HANDOVER_TIMEOUT + Duration::from_secs(1));
    let started = Instant::now();
    let third = FakeControl::new();
    let error = audio
        .switch(
            move || FakeBackend(third),
            config(None),
            || panic!("nothing was handed over"),
        )
        .unwrap_err();
    assert_eq!(error, lumen_audio::SwitchError::NoHandover);
    assert!(started.elapsed() >= lumen_audio::HANDOVER_TIMEOUT);
}
