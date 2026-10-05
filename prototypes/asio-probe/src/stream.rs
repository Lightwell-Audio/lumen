//! Streaming test: real callbacks from a real driver, measured.
//!
//! ASIO callbacks carry no user pointer, so the active stream lives in a
//! process-wide slot. That is inherent to ASIO: one driver per process.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use engine_core::{Engine, Graph, test_source, test_tracks};

use crate::driver::{ComApartment, Driver, HiddenWindow, pump_messages};
use crate::ffi::{self, AsioBool, AsioBufferInfo, AsioCallbacks, AsioSampleType, AsioTime};
use crate::registry;

#[derive(Debug)]
pub struct StreamOptions {
    /// Registry key name (case-insensitive) or index from `list`.
    pub driver: String,
    pub seconds: f64,
    /// Play a -24 dBFS 440 Hz tone on the first two outputs. Off by default.
    pub tone: bool,
    /// Run the P-ENGINE workload with this many tracks inside the callback.
    pub engine_tracks: u32,
    /// Override the driver's preferred buffer size.
    pub buffer_size: Option<i32>,
    /// Channels opened per direction.
    pub channels: usize,
}

static CALLBACKS: AsioCallbacks = AsioCallbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

static ACTIVE: AtomicPtr<State> = AtomicPtr::new(ptr::null_mut());
static RESET_REQUESTS: AtomicU32 = AtomicU32::new(0);
static RESYNC_REQUESTS: AtomicU32 = AtomicU32::new(0);
static LATENCY_CHANGES: AtomicU32 = AtomicU32::new(0);
static OVERLOADS: AtomicU32 = AtomicU32::new(0);
static RATE_CHANGES: AtomicU32 = AtomicU32::new(0);
static TIME_INFO_CALLBACKS: AtomicU64 = AtomicU64::new(0);
static PLAIN_CALLBACKS: AtomicU64 = AtomicU64::new(0);

struct Channel {
    buffers: [*mut c_void; 2],
    sample_type: AsioSampleType,
}

/// Owned by the callback while streaming; by the main thread otherwise.
struct State {
    driver: *mut ffi::IAsio,
    frames: usize,
    sample_rate: f64,
    inputs: Vec<Channel>,
    outputs: Vec<Channel>,
    use_output_ready: bool,
    tone_step: Option<f64>,
    tone_phase: f64,
    tone_buffer: Vec<f32>,
    engine: Option<Engine>,
    scratch_left: Vec<f32>,
    scratch_right: Vec<f32>,
    intervals_ns: Vec<u32>,
    durations_ns: Vec<u32>,
    last_entry: Option<Instant>,
    expected_position: Option<u64>,
    discontinuities: u64,
    positions_missing: u64,
    input_peak: f32,
    latencies: Option<(i32, i32)>,
    sample_types: Vec<String>,
}

unsafe extern "C" fn buffer_switch(index: i32, _direct_process: AsioBool) {
    PLAIN_CALLBACKS.fetch_add(1, Ordering::Relaxed);
    let state = ACTIVE.load(Ordering::Acquire);
    if state.is_null() {
        return;
    }
    // SAFETY: the main thread does not touch `State` while it is active.
    let state = unsafe { &mut *state };
    let mut position = ffi::AsioSamples::default();
    let mut stamp = ffi::AsioTimeStamp::default();
    // SAFETY: `state.driver` is the live driver that is calling us.
    let ok = unsafe {
        ((*(*state.driver).vtbl).get_sample_position)(state.driver, &mut position, &mut stamp)
    } == ffi::ASE_OK;
    process(state, index, ok.then(|| position.get()));
}

unsafe extern "C" fn buffer_switch_time_info(
    params: *mut AsioTime,
    index: i32,
    _direct_process: AsioBool,
) -> *mut AsioTime {
    TIME_INFO_CALLBACKS.fetch_add(1, Ordering::Relaxed);
    let state = ACTIVE.load(Ordering::Acquire);
    if state.is_null() {
        return params;
    }
    let position = if params.is_null() {
        None
    } else {
        // SAFETY: the driver passes a valid AsioTime for the duration of the call.
        let info = unsafe { (*params).time_info };
        let flags = info.flags;
        let sample_position = info.sample_position;
        (flags & ffi::K_SAMPLE_POSITION_VALID != 0).then(|| sample_position.get())
    };
    // SAFETY: as in `buffer_switch`.
    process(unsafe { &mut *state }, index, position);
    params
}

unsafe extern "C" fn sample_rate_did_change(_rate: f64) {
    RATE_CHANGES.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn asio_message(
    selector: i32,
    value: i32,
    _message: *mut c_void,
    _opt: *mut f64,
) -> i32 {
    match selector {
        ffi::K_ASIO_SELECTOR_SUPPORTED => i32::from(matches!(
            value,
            ffi::K_ASIO_ENGINE_VERSION
                | ffi::K_ASIO_RESET_REQUEST
                | ffi::K_ASIO_RESYNC_REQUEST
                | ffi::K_ASIO_LATENCIES_CHANGED
                | ffi::K_ASIO_SUPPORTS_TIME_INFO
                | ffi::K_ASIO_OVERLOAD
        )),
        ffi::K_ASIO_ENGINE_VERSION => 2,
        // Handled on the main thread: stop, dispose, recreate, start.
        ffi::K_ASIO_RESET_REQUEST => {
            RESET_REQUESTS.fetch_add(1, Ordering::Release);
            1
        }
        ffi::K_ASIO_RESYNC_REQUEST => {
            RESYNC_REQUESTS.fetch_add(1, Ordering::Relaxed);
            1
        }
        ffi::K_ASIO_LATENCIES_CHANGED => {
            LATENCY_CHANGES.fetch_add(1, Ordering::Relaxed);
            1
        }
        ffi::K_ASIO_SUPPORTS_TIME_INFO => 1,
        ffi::K_ASIO_OVERLOAD => {
            OVERLOADS.fetch_add(1, Ordering::Relaxed);
            0
        }
        // Buffer-size changes without a reset are declined; drivers then send a reset.
        ffi::K_ASIO_BUFFER_SIZE_CHANGE | ffi::K_ASIO_SUPPORTS_TIME_CODE => 0,
        _ => 0,
    }
}

/// The audio callback body. Real-time code: checked by the allocation guard.
fn process(state: &mut State, index: i32, position: Option<u64>) {
    lumen_rt::rt_section(|| {
        let entered = Instant::now();
        if let Some(last) = state.last_entry {
            push_bounded(&mut state.intervals_ns, (entered - last).as_nanos() as u32);
        }
        state.last_entry = Some(entered);

        match position {
            Some(position) => {
                if state
                    .expected_position
                    .is_some_and(|expected| expected != position)
                {
                    state.discontinuities += 1;
                }
                state.expected_position = Some(position + state.frames as u64);
            }
            None => state.positions_missing += 1,
        }

        let half = (index & 1) as usize;
        let frames = state.frames;
        for channel in &state.inputs {
            // SAFETY: driver buffers hold `frames` samples of `sample_type`.
            let peak = unsafe { channel_peak(channel.buffers[half], channel.sample_type, frames) };
            state.input_peak = state.input_peak.max(peak);
        }

        if let Some(engine) = state.engine.as_mut() {
            engine.process(&mut state.scratch_left, &mut state.scratch_right);
            std::hint::black_box(&state.scratch_left);
        }

        if let Some(step) = state.tone_step {
            for sample in &mut state.tone_buffer {
                *sample = (state.tone_phase.sin() * 0.063) as f32;
                state.tone_phase = (state.tone_phase + step) % std::f64::consts::TAU;
            }
        }
        for (i, channel) in state.outputs.iter().enumerate() {
            let source = (state.tone_step.is_some() && i < 2).then_some(&state.tone_buffer[..]);
            // SAFETY: as above, for output buffers.
            unsafe { write_channel(channel.buffers[half], channel.sample_type, frames, source) };
        }

        if state.use_output_ready {
            // SAFETY: live driver; outputReady is allowed from the callback.
            unsafe { ((*(*state.driver).vtbl).output_ready)(state.driver) };
        }
        push_bounded(&mut state.durations_ns, entered.elapsed().as_nanos() as u32);
    });
}

/// Appends only within preallocated capacity, so it never allocates.
fn push_bounded(values: &mut Vec<u32>, value: u32) {
    if values.len() < values.capacity() {
        values.push(value);
    }
}

fn bytes_per_sample(sample_type: AsioSampleType) -> Option<usize> {
    match sample_type {
        ffi::ST_INT16_LSB | 0 => Some(2),
        ffi::ST_INT24_LSB | 1 => Some(3),
        ffi::ST_INT32_LSB | ffi::ST_FLOAT32_LSB | 2 | 3 => Some(4),
        ffi::ST_INT32_LSB16..=ffi::ST_INT32_LSB24 | 8..=11 => Some(4),
        ffi::ST_FLOAT64_LSB | 4 => Some(8),
        _ => None,
    }
}

/// Writes `source` converted to `sample_type`, or silence if `None`.
///
/// # Safety
/// `buffer` must hold `frames` samples of `sample_type`.
unsafe fn write_channel(
    buffer: *mut c_void,
    sample_type: AsioSampleType,
    frames: usize,
    source: Option<&[f32]>,
) {
    let Some(width) = bytes_per_sample(sample_type) else {
        return;
    };
    let Some(source) = source else {
        // SAFETY: all-zero bits are silence in every PCM and float format.
        unsafe { ptr::write_bytes(buffer.cast::<u8>(), 0, frames * width) };
        return;
    };
    let scale = |bits: u32| f64::from((1_u32 << (bits - 1)) - 1);
    for (i, &x) in source.iter().take(frames).enumerate() {
        let x = f64::from(x.clamp(-1.0, 1.0));
        // SAFETY: `i < frames`, so every write stays inside the buffer.
        unsafe {
            match sample_type {
                ffi::ST_FLOAT32_LSB => buffer.cast::<f32>().add(i).write_unaligned(x as f32),
                ffi::ST_FLOAT64_LSB => buffer.cast::<f64>().add(i).write_unaligned(x),
                ffi::ST_INT16_LSB => buffer
                    .cast::<i16>()
                    .add(i)
                    .write_unaligned((x * scale(16)) as i16),
                ffi::ST_INT32_LSB => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * scale(32)) as i32),
                ffi::ST_INT32_LSB16 => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * scale(16)) as i32),
                ffi::ST_INT32_LSB18 => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * scale(18)) as i32),
                ffi::ST_INT32_LSB20 => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * scale(20)) as i32),
                ffi::ST_INT32_LSB24 => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * scale(24)) as i32),
                ffi::ST_INT24_LSB => {
                    let v = ((x * scale(24)) as i32).to_le_bytes();
                    ptr::copy_nonoverlapping(v.as_ptr(), buffer.cast::<u8>().add(i * 3), 3);
                }
                // Big-endian formats: silence (not used by Windows drivers in practice).
                _ => ptr::write_bytes(buffer.cast::<u8>().add(i * width), 0, width),
            }
        }
    }
}

/// Peak absolute level of an input buffer, 0..=1.
///
/// # Safety
/// `buffer` must hold `frames` samples of `sample_type`.
unsafe fn channel_peak(buffer: *const c_void, sample_type: AsioSampleType, frames: usize) -> f32 {
    let mut peak = 0.0_f64;
    for i in 0..frames {
        // SAFETY: `i < frames`, so every read stays inside the buffer.
        let x = unsafe {
            match sample_type {
                ffi::ST_FLOAT32_LSB => f64::from(buffer.cast::<f32>().add(i).read_unaligned()),
                ffi::ST_FLOAT64_LSB => buffer.cast::<f64>().add(i).read_unaligned(),
                ffi::ST_INT16_LSB => {
                    f64::from(buffer.cast::<i16>().add(i).read_unaligned()) / 32_768.0
                }
                ffi::ST_INT32_LSB => {
                    f64::from(buffer.cast::<i32>().add(i).read_unaligned()) / 2_147_483_648.0
                }
                ffi::ST_INT32_LSB24 => {
                    f64::from(buffer.cast::<i32>().add(i).read_unaligned()) / 8_388_608.0
                }
                ffi::ST_INT24_LSB => {
                    let p = buffer.cast::<u8>().add(i * 3);
                    let v = i32::from_le_bytes([0, p.read(), p.add(1).read(), p.add(2).read()]);
                    f64::from(v) / 2_147_483_648.0
                }
                _ => 0.0,
            }
        };
        peak = peak.max(x.abs());
    }
    peak as f32
}

/// Allocates buffers and builds callback state. Main thread, not streaming.
fn open(driver: &Driver, options: &StreamOptions) -> Result<*mut State, String> {
    let asio = |e| format!("{} ({e})", ffi::error_name(e));
    let (inputs, outputs) = driver.channels().map_err(asio)?;
    let sizes = driver.buffer_sizes().map_err(asio)?;
    let frames = options.buffer_size.unwrap_or(sizes.preferred);
    let sample_rate = driver.sample_rate().map_err(asio)?;
    let input_count = (inputs.max(0) as usize).min(options.channels);
    let output_count = (outputs.max(0) as usize).min(options.channels);
    let mut infos: Vec<AsioBufferInfo> = (0..input_count)
        .map(|c| (1, c))
        .chain((0..output_count).map(|c| (0, c)))
        .map(|(is_input, c)| AsioBufferInfo {
            is_input,
            channel_num: c as i32,
            buffers: [ptr::null_mut(); 2],
        })
        .collect();
    driver
        .create_buffers(&mut infos, frames, &CALLBACKS)
        .map_err(|e| format!("createBuffers({frames}) failed: {}", asio(e)))?;

    let mut sample_types = Vec::new();
    let mut channel = |info: &AsioBufferInfo| {
        let sample_type = driver
            .channel_info(info.channel_num, info.is_input == 1)
            .map(|c| c.sample_type)
            .unwrap_or(-1);
        let name = ffi::sample_type_name(sample_type).to_string();
        if !sample_types.contains(&name) {
            sample_types.push(name);
        }
        Channel {
            buffers: info.buffers,
            sample_type,
        }
    };
    let input_channels: Vec<Channel> = infos[..input_count].iter().map(&mut channel).collect();
    let output_channels: Vec<Channel> = infos[input_count..].iter().map(&mut channel).collect();
    let frames = frames as usize;
    for output in &output_channels {
        for half in output.buffers {
            // SAFETY: freshly created driver buffers of `frames` samples.
            unsafe { write_channel(half, output.sample_type, frames, None) };
        }
    }

    let callbacks_expected = (options.seconds * sample_rate / frames as f64) as usize + 1_024;
    let engine = (options.engine_tracks > 0).then(|| {
        let graph = Graph::new(
            sample_rate as f32,
            test_source(sample_rate as usize * 4),
            &test_tracks(options.engine_tracks, 4_801, 10_000),
        );
        engine_core::engine(graph).1
    });
    let state = State {
        driver: driver.raw(),
        frames,
        sample_rate,
        inputs: input_channels,
        outputs: output_channels,
        use_output_ready: driver.output_ready() == ffi::ASE_OK,
        tone_step: options
            .tone
            .then(|| std::f64::consts::TAU * 440.0 / sample_rate),
        tone_phase: 0.0,
        tone_buffer: vec![0.0; frames],
        engine,
        scratch_left: vec![0.0; frames],
        scratch_right: vec![0.0; frames],
        intervals_ns: Vec::with_capacity(callbacks_expected),
        durations_ns: Vec::with_capacity(callbacks_expected),
        last_entry: None,
        expected_position: None,
        discontinuities: 0,
        positions_missing: 0,
        input_peak: 0.0,
        latencies: driver.latencies().ok(),
        sample_types,
    };
    Ok(Box::into_raw(Box::new(state)))
}

/// Stops the driver, detaches the state and frees driver buffers.
fn close(driver: &Driver, state: *mut State) -> Box<State> {
    let _ = driver.stop();
    // SAFETY: `state` came from Box::into_raw in `open`.
    let frames_per_second = unsafe { (*state).sample_rate / (*state).frames as f64 };
    // Let any in-flight callback finish before the state is touched again.
    std::thread::sleep(
        Duration::from_secs_f64(3.0 / frames_per_second) + Duration::from_millis(20),
    );
    ACTIVE.store(ptr::null_mut(), Ordering::Release);
    let _ = driver.dispose_buffers();
    // SAFETY: no callback can reach `state` any more; ownership returns here.
    unsafe { Box::from_raw(state) }
}

pub fn run(options: &StreamOptions) -> Result<(), String> {
    let drivers = registry::installed_drivers();
    let chosen = match options.driver.parse::<usize>() {
        Ok(index) => drivers.get(index),
        Err(_) => drivers
            .iter()
            .find(|d| d.key_name.eq_ignore_ascii_case(&options.driver)),
    }
    .ok_or_else(|| format!("no ASIO driver matches {:?}", options.driver))?;

    let _com = ComApartment::enter()?;
    let window = HiddenWindow::create()?;
    let driver = Driver::create(&chosen.clsid)?;
    driver
        .init(window.0)
        .map_err(|m| format!("init failed: {m}"))?;
    println!("Driver: {} (version {})", driver.name(), driver.version());

    let mut state = open(&driver, options)?;
    let mut segments = Vec::new();
    let mut resets_seen = RESET_REQUESTS.load(Ordering::Acquire);
    ACTIVE.store(state, Ordering::Release);
    driver
        .start()
        .map_err(|e| format!("start failed: {}", ffi::error_name(e)))?;

    let deadline = Instant::now() + Duration::from_secs_f64(options.seconds);
    while Instant::now() < deadline {
        pump_messages();
        let resets = RESET_REQUESTS.load(Ordering::Acquire);
        if resets != resets_seen {
            resets_seen = resets;
            println!("kAsioResetRequest received: recreating buffers");
            segments.push(close(&driver, state));
            state = open(&driver, options)?;
            ACTIVE.store(state, Ordering::Release);
            driver
                .start()
                .map_err(|e| format!("restart failed: {}", ffi::error_name(e)))?;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    segments.push(close(&driver, state));

    for (i, segment) in segments.iter().enumerate() {
        report(i, segment, options);
    }
    println!(
        "messages: resets {}, resyncs {}, latency changes {}, overloads {}, rate changes {}",
        RESET_REQUESTS.load(Ordering::Relaxed),
        RESYNC_REQUESTS.load(Ordering::Relaxed),
        LATENCY_CHANGES.load(Ordering::Relaxed),
        OVERLOADS.load(Ordering::Relaxed),
        RATE_CHANGES.load(Ordering::Relaxed),
    );
    println!(
        "callback kinds: bufferSwitchTimeInfo {}, bufferSwitch {}",
        TIME_INFO_CALLBACKS.load(Ordering::Relaxed),
        PLAIN_CALLBACKS.load(Ordering::Relaxed),
    );
    println!(
        "real-time heap operations inside callbacks: {}",
        lumen_rt::total_violations()
    );
    Ok(())
}

fn report(index: usize, state: &State, options: &StreamOptions) {
    let period_ms = state.frames as f64 / state.sample_rate * 1e3;
    println!(
        "\n-- segment {index}: {} frames @ {} Hz (period {period_ms:.3} ms)",
        state.frames, state.sample_rate
    );
    println!(
        "   channels: {} in / {} out, types: {}, outputReady: {}",
        state.inputs.len(),
        state.outputs.len(),
        state.sample_types.join(", "),
        state.use_output_ready
    );
    if let Some((input, output)) = state.latencies {
        let ms = |samples: i32| f64::from(samples) / state.sample_rate * 1e3;
        println!(
            "   reported latency: input {input} ({:.2} ms), output {output} ({:.2} ms), round trip {:.2} ms",
            ms(input),
            ms(output),
            ms(input + output)
        );
    }
    let callbacks = state.durations_ns.len();
    let expected = (options.seconds * state.sample_rate / state.frames as f64) as usize;
    println!("   callbacks: {callbacks} (about {expected} expected for the full run)");
    let stats = |values: &[u32]| -> String {
        if values.is_empty() {
            return "n/a".into();
        }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let at = |p: f64| f64::from(sorted[((sorted.len() - 1) as f64 * p) as usize]) / 1e3;
        format!(
            "min {:.1} / p50 {:.1} / p99 {:.1} / p99.9 {:.1} / max {:.1} µs",
            at(0.0),
            at(0.5),
            at(0.99),
            at(0.999),
            at(1.0)
        )
    };
    println!("   interval: {}", stats(&state.intervals_ns));
    println!("   callback time: {}", stats(&state.durations_ns));
    if let Some(max) = state.durations_ns.iter().max() {
        println!(
            "   worst callback used {:.2}% of the period",
            f64::from(*max) / 1e6 / period_ms * 100.0
        );
    }
    println!(
        "   sample-position discontinuities: {}, callbacks without position: {}",
        state.discontinuities, state.positions_missing
    );
    let peak_db = 20.0 * f64::from(state.input_peak.max(1e-9)).log10();
    println!("   input peak: {peak_db:.1} dBFS");
    if options.engine_tracks > 0 {
        println!(
            "   engine load: {} tracks processed in every callback",
            options.engine_tracks
        );
    }
}
