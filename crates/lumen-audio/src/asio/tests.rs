//! The ASIO backend's tests: the callbacks, the teardown, the selector
//! mapping and the format conversion.

use super::*;

fn round_trip(sample_type: AsioSampleType, width: usize) -> f32 {
    let source: Vec<f32> = (0..64).map(|i| (i as f32 / 32.0 - 1.0) * 0.9).collect();
    let mut raw = vec![0_u8; source.len() * width];
    let mut back = vec![0.0_f32; source.len()];
    // SAFETY: `raw` holds 64 samples of `width` bytes.
    unsafe {
        write_samples(raw.as_mut_ptr().cast(), sample_type, &source);
        read_samples(raw.as_ptr().cast(), sample_type, &mut back);
    }
    source
        .iter()
        .zip(&back)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max)
}

#[test]
fn every_supported_format_round_trips_within_its_resolution() {
    assert!(round_trip(ffi::ST_FLOAT32_LSB, 4) == 0.0);
    assert!(round_trip(ffi::ST_FLOAT64_LSB, 8) < 1e-7);
    assert!(round_trip(ffi::ST_INT32_LSB, 4) < 1e-6);
    assert!(round_trip(ffi::ST_INT32_LSB24, 4) < 1e-6);
    assert!(round_trip(ffi::ST_INT24_LSB, 3) < 1e-6);
    assert!(round_trip(ffi::ST_INT32_LSB16, 4) < 1e-4);
    assert!(round_trip(ffi::ST_INT16_LSB, 2) < 1e-4);
}

#[test]
fn output_is_clipped_not_wrapped() {
    let mut raw = [0_i32; 2];
    // SAFETY: two Int32 samples.
    unsafe { write_samples(raw.as_mut_ptr().cast(), ffi::ST_INT32_LSB, &[2.0, -2.0]) };
    assert_eq!(raw, [i32::MAX, -i32::MAX]);
}

#[test]
fn big_endian_formats_are_reported_unsupported() {
    assert!(!supported(0)); // Int16MSB
    assert!(supported(ffi::ST_INT32_LSB));
}

/// The driver seam is a raw COM vtable with no fake behind it, so the
/// buffers guard is checked on its own: every return past `create_buffers`
/// disposes, and only the path that builds a stream keeps them (sweep M5b).
#[test]
fn an_open_that_fails_after_creating_buffers_disposes_them() {
    use std::cell::Cell;

    fn open(disposed: &Cell<u32>, fail: bool) -> Result<(), ()> {
        let created = DisposeOnDrop::new(|| disposed.set(disposed.get() + 1));
        let result = if fail { Err(()) } else { Ok(()) };
        result?;
        created.disarm();
        Ok(())
    }

    let disposed = Cell::new(0);
    assert!(open(&disposed, true).is_err());
    assert_eq!(disposed.get(), 1, "a failed open gives the buffers back");
    assert!(open(&disposed, false).is_ok());
    assert_eq!(disposed.get(), 1, "the stream keeps the buffers it opened");
}

/// Dropping the guard without disarming is the early-return path, and it
/// disposes exactly once; an explicit drop after disarming does nothing.
#[test]
fn the_buffers_guard_disposes_once_and_only_when_armed() {
    use std::cell::Cell;

    let disposed = Cell::new(0);
    drop(DisposeOnDrop::new(|| disposed.set(disposed.get() + 1)));
    assert_eq!(disposed.get(), 1);
    let created = DisposeOnDrop::new(|| disposed.set(disposed.get() + 1));
    created.disarm();
    assert_eq!(disposed.get(), 1, "disarmed: the stream owns them");
}

/// A driver another application holds must not read as a missing one: the
/// user is told it is busy, with a way forward (sweep M5c).
#[test]
fn a_driver_held_elsewhere_reads_as_busy_not_missing() {
    for message in [
        "The device is in use by another application",
        "ASIO driver busy",
        "Device already open",
    ] {
        let error = init_error(message.into());
        assert!(
            matches!(&error, AudioError::DeviceBusy(m) if m == message),
            "{error:?} for {message}"
        );
    }
    let other = init_error("No ASIO device found".into());
    assert!(
        matches!(&other, AudioError::Driver(m) if m == "No ASIO device found"),
        "{other:?}"
    );
    assert!(
        !matches!(other, AudioError::DeviceNotPresent(_)),
        "an init failure is not proof the device is absent"
    );
    assert!(other.to_string().contains("No ASIO device found"));
}

/// A stream is shut down once: a second call, which `Drop` makes after
/// `close`, finds the driver taken and frees nothing twice (sweep M5d).
#[test]
fn shutting_a_stream_down_twice_is_harmless() {
    let mut stream = AsioStream {
        driver: None,
        state: ptr::null_mut(),
        info: StreamInfo {
            sample_rate: 48_000,
            buffer_frames: 128,
            input_latency: 0,
            output_latency: 0,
            input_channels: 0,
            output_channels: 2,
            silent_inputs: Vec::new(),
        },
        started_at: Instant::now(),
        failed_to_start: false,
    };
    drop(stream.shutdown());
    drop(stream.shutdown());
}

/// A state for the callback tests: one output whose driver buffer is
/// `raw`, and nothing else.
fn state(raw: &mut [f32], callback: Box<dyn AudioCallback>) -> StreamState {
    let frames = raw.len();
    let pointer = raw.as_mut_ptr().cast();
    StreamState {
        driver: ptr::null_mut(),
        frames,
        sample_rate: 48_000,
        inputs: Vec::new(),
        outputs: vec![Channel {
            buffers: [pointer, pointer],
            sample_type: ffi::ST_FLOAT32_LSB,
        }],
        input_buffer: Vec::new(),
        output_buffer: vec![0.0; frames],
        callback,
        use_output_ready: false,
        last_callback_ns: AtomicU64::new(0),
        epoch: Instant::now(),
    }
}

/// The callback path allocates nothing and writes what the callback made
/// (sweep M5e).
#[test]
fn a_block_runs_without_allocating() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is this test binary's"
    );
    let mut raw = vec![0.0_f32; 32];
    let mut state = state(
        &mut raw,
        Box::new(|block: &mut Block<'_>| block.outputs.fill(0.5)),
    );
    let (_, allocated) = lumen_rt::count_rt_violations(|| guarded(&mut state, 0, Some(7)));
    assert_eq!(allocated, 0, "the driver callback allocates nothing");
    assert!(
        raw.iter().all(|&x| x == 0.5),
        "the block reached the driver"
    );
}

/// A callback that panics must not unwind into the driver (that ends the
/// process); the driver gets silence rather than the half-written block,
/// and the next poll reports the stall (sweep M5e).
#[test]
fn a_panicking_callback_is_contained_and_leaves_silence() {
    let mut raw = vec![0.0_f32; 32];
    let mut state = state(
        &mut raw,
        Box::new(|block: &mut Block<'_>| {
            block.outputs.fill(1.0);
            panic!("a user callback bug");
        }),
    );
    FAULTED.store(false, Ordering::SeqCst);
    guarded(&mut state, 0, None);
    assert!(
        raw.iter().all(|&x| x == 0.0),
        "the driver got silence, not the half-written block"
    );
    assert!(
        FAULTED.swap(false, Ordering::SeqCst),
        "the panic is remembered for the next poll"
    );
}

/// A driver message maps to one request: a buffer size change rebuilds the
/// stream like a reset, an overload is a missed buffer, and the selectors
/// this stream answers are exactly those it acts on (sweep M5f).
#[test]
fn driver_messages_map_to_requests() {
    assert_eq!(request(ffi::K_ASIO_RESET_REQUEST), Request::Reset);
    assert_eq!(
        request(ffi::K_ASIO_BUFFER_SIZE_CHANGE),
        Request::Reset,
        "a buffer size change needs the stream rebuilt"
    );
    assert_eq!(request(ffi::K_ASIO_RESYNC_REQUEST), Request::Resync);
    assert_eq!(request(ffi::K_ASIO_OVERLOAD), Request::Overload);
    assert_eq!(request(99), Request::Nothing);
    for selector in [
        ffi::K_ASIO_ENGINE_VERSION,
        ffi::K_ASIO_RESET_REQUEST,
        ffi::K_ASIO_BUFFER_SIZE_CHANGE,
        ffi::K_ASIO_RESYNC_REQUEST,
        ffi::K_ASIO_LATENCIES_CHANGED,
        ffi::K_ASIO_SUPPORTS_TIME_INFO,
        ffi::K_ASIO_OVERLOAD,
    ] {
        assert!(handles(selector), "selector {selector}");
    }
    assert!(
        !handles(8),
        "time code is not handled, so it is not claimed"
    );
}

/// The wiring behind the mapping: an overload message is counted for the
/// next poll, and a buffer size change asks for a reset (sweep M5f).
#[test]
fn an_overload_is_counted_and_a_buffer_size_change_resets() {
    OVERLOADS.store(0, Ordering::Relaxed);
    RESET_REQUESTED.store(false, Ordering::Release);
    // SAFETY: the callback touches only its atomics with these arguments.
    let taken = unsafe { asio_message(ffi::K_ASIO_OVERLOAD, 0, ptr::null_mut(), ptr::null_mut()) };
    assert_eq!(taken, 1, "an overload is accepted");
    assert_eq!(OVERLOADS.swap(0, Ordering::Relaxed), 1);
    // SAFETY: as above.
    let taken = unsafe {
        asio_message(
            ffi::K_ASIO_BUFFER_SIZE_CHANGE,
            256,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert_eq!(taken, 1);
    assert!(RESET_REQUESTED.swap(false, Ordering::AcqRel), "a reset");
}
