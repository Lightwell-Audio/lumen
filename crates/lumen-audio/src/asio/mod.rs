//! Native ASIO backend (Windows, x86-64), grown from P-ASIO.
//!
//! The interface definitions in `ffi` are clean-room; shipping requires the
//! signed Steinberg ASIO SDK agreement (ADR-0006, owner action).
//!
//! ASIO callbacks carry no user pointer, so the running stream lives in a
//! process-wide slot: one ASIO stream per process, which is how ASIO works.

mod driver;
mod ffi;
mod inflight;
mod registry;

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::backend::{Backend, OpenError, Stream, StreamEvent};
use crate::types::{
    AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind, StreamConfig,
    StreamInfo,
};
use driver::{ComApartment, Driver, HiddenWindow, pump_messages};
use ffi::{AsioBool, AsioBufferInfo, AsioCallbacks, AsioSampleType, AsioTime};
use inflight::InFlight;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

static CALLBACKS: AsioCallbacks = AsioCallbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

/// The running stream's state, owned by the driver thread while active.
static ACTIVE: AtomicPtr<StreamState> = AtomicPtr::new(ptr::null_mut());
/// The driver callbacks running now: a stream frees its state only once this
/// is zero, so a late callback never reads freed memory.
static IN_FLIGHT: InFlight = InFlight::new();
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
/// The driver lost sync (its sample position jumps): the next block reports
/// no position, so the jump is not counted as dropouts.
static RESYNC_REQUESTED: AtomicBool = AtomicBool::new(false);
/// A callback panicked and its block was silenced: the next poll reports it.
static FAULTED: AtomicBool = AtomicBool::new(false);

struct Channel {
    buffers: [*mut c_void; 2],
    sample_type: AsioSampleType,
}

struct StreamState {
    driver: *mut ffi::IAsio,
    frames: usize,
    sample_rate: u32,
    inputs: Vec<Channel>,
    outputs: Vec<Channel>,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    callback: Box<dyn AudioCallback>,
    use_output_ready: bool,
    /// Nanoseconds since `epoch` at the last callback, for the watchdog.
    last_callback_ns: AtomicU64,
    epoch: Instant,
}

unsafe extern "C" fn buffer_switch(index: i32, _direct: AsioBool) {
    // Counted before the state is looked at: `close` waits for this count to
    // reach zero before it frees anything, so a callback that finds a state
    // keeps it alive until it is done with it.
    let _running = IN_FLIGHT.enter();
    let state = ACTIVE.load(Ordering::SeqCst);
    if state.is_null() {
        return;
    }
    // SAFETY: only this driver thread touches the state while it is active.
    let state = unsafe { &mut *state };
    let mut position = ffi::AsioSamples::default();
    let mut stamp = ffi::AsioTimeStamp::default();
    // SAFETY: live driver calling us.
    let ok = unsafe {
        ((*(*state.driver).vtbl).get_sample_position)(state.driver, &mut position, &mut stamp)
    } == ffi::ASE_OK;
    process(state, index, ok.then(|| position.get()));
}

unsafe extern "C" fn buffer_switch_time_info(
    params: *mut AsioTime,
    index: i32,
    _direct: AsioBool,
) -> *mut AsioTime {
    let _running = IN_FLIGHT.enter();
    let state = ACTIVE.load(Ordering::SeqCst);
    if state.is_null() {
        return params;
    }
    let position = if params.is_null() {
        None
    } else {
        // SAFETY: valid for the duration of the call.
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
    // A rate change invalidates the stream; treat it like a reset request.
    RESET_REQUESTED.store(true, Ordering::Release);
}

unsafe extern "C" fn asio_message(selector: i32, value: i32, _m: *mut c_void, _o: *mut f64) -> i32 {
    match selector {
        ffi::K_ASIO_SELECTOR_SUPPORTED => i32::from(matches!(
            value,
            ffi::K_ASIO_ENGINE_VERSION
                | ffi::K_ASIO_RESET_REQUEST
                | ffi::K_ASIO_RESYNC_REQUEST
                | ffi::K_ASIO_LATENCIES_CHANGED
                | ffi::K_ASIO_SUPPORTS_TIME_INFO
        )),
        ffi::K_ASIO_ENGINE_VERSION => 2,
        ffi::K_ASIO_RESET_REQUEST => {
            RESET_REQUESTED.store(true, Ordering::Release);
            1
        }
        ffi::K_ASIO_RESYNC_REQUEST => {
            RESYNC_REQUESTED.store(true, Ordering::Release);
            1
        }
        ffi::K_ASIO_LATENCIES_CHANGED | ffi::K_ASIO_SUPPORTS_TIME_INFO => 1,
        _ => 0,
    }
}

/// The audio callback body. Real-time code.
fn process(state: &mut StreamState, index: i32, position: Option<u64>) {
    let position = position.filter(|_| !RESYNC_REQUESTED.swap(false, Ordering::AcqRel));
    let half = (index & 1) as usize;
    let frames = state.frames;
    for (i, channel) in state.inputs.iter().enumerate() {
        // SAFETY: driver buffers hold `frames` samples of `sample_type`.
        unsafe {
            read_samples(
                channel.buffers[half],
                channel.sample_type,
                &mut state.input_buffer[i * frames..(i + 1) * frames],
            );
        }
    }
    state.output_buffer.fill(0.0);
    let mut block = Block {
        sample_rate: state.sample_rate,
        frames,
        inputs: &state.input_buffer,
        outputs: &mut state.output_buffer,
        input_channels: state.inputs.len(),
        output_channels: state.outputs.len(),
        position,
    };
    state.callback.process(&mut block);
    for (i, channel) in state.outputs.iter().enumerate() {
        // SAFETY: as above, for output buffers.
        unsafe {
            write_samples(
                channel.buffers[half],
                channel.sample_type,
                &state.output_buffer[i * frames..(i + 1) * frames],
            );
        }
    }
    if state.use_output_ready {
        // SAFETY: live driver; allowed from the callback.
        unsafe { ((*(*state.driver).vtbl).output_ready)(state.driver) };
    }
    state
        .last_callback_ns
        .store(state.epoch.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

/// Formats Lumen converts; anything else is rejected at open time.
fn supported(sample_type: AsioSampleType) -> bool {
    matches!(
        sample_type,
        ffi::ST_INT16_LSB
            | ffi::ST_INT24_LSB
            | ffi::ST_INT32_LSB
            | ffi::ST_FLOAT32_LSB
            | ffi::ST_FLOAT64_LSB
            | ffi::ST_INT32_LSB16
            | ffi::ST_INT32_LSB18
            | ffi::ST_INT32_LSB20
            | ffi::ST_INT32_LSB24
    )
}

/// Full-scale value of a format's integer range.
fn scale(sample_type: AsioSampleType) -> f64 {
    let bits = match sample_type {
        ffi::ST_INT16_LSB | ffi::ST_INT32_LSB16 => 16,
        ffi::ST_INT32_LSB18 => 18,
        ffi::ST_INT32_LSB20 => 20,
        ffi::ST_INT24_LSB | ffi::ST_INT32_LSB24 => 24,
        _ => 32,
    };
    f64::from((1_u32 << (bits - 1)) - 1)
}

/// Converts `out.len()` driver samples to `f32`.
///
/// # Safety
/// `buffer` must hold `out.len()` samples of `sample_type`.
unsafe fn read_samples(buffer: *const c_void, sample_type: AsioSampleType, out: &mut [f32]) {
    let s = scale(sample_type);
    for (i, sample) in out.iter_mut().enumerate() {
        // SAFETY: `i < out.len()`, within the buffer.
        *sample = unsafe {
            match sample_type {
                ffi::ST_FLOAT32_LSB => buffer.cast::<f32>().add(i).read_unaligned(),
                ffi::ST_FLOAT64_LSB => buffer.cast::<f64>().add(i).read_unaligned() as f32,
                ffi::ST_INT16_LSB => {
                    (f64::from(buffer.cast::<i16>().add(i).read_unaligned()) / s) as f32
                }
                ffi::ST_INT24_LSB => {
                    let p = buffer.cast::<u8>().add(i * 3);
                    let v =
                        i32::from_le_bytes([0, p.read(), p.add(1).read(), p.add(2).read()]) >> 8;
                    (f64::from(v) / s) as f32
                }
                ffi::ST_INT32_LSB
                | ffi::ST_INT32_LSB16
                | ffi::ST_INT32_LSB18
                | ffi::ST_INT32_LSB20
                | ffi::ST_INT32_LSB24 => {
                    (f64::from(buffer.cast::<i32>().add(i).read_unaligned()) / s) as f32
                }
                _ => 0.0,
            }
        };
    }
}

/// Converts `f32` samples to the driver's format, clipping to ±1.
///
/// # Safety
/// `buffer` must hold `source.len()` samples of `sample_type`.
unsafe fn write_samples(buffer: *mut c_void, sample_type: AsioSampleType, source: &[f32]) {
    let s = scale(sample_type);
    for (i, &x) in source.iter().enumerate() {
        let x = f64::from(x.clamp(-1.0, 1.0));
        // SAFETY: `i < source.len()`, within the buffer.
        unsafe {
            match sample_type {
                ffi::ST_FLOAT32_LSB => buffer.cast::<f32>().add(i).write_unaligned(x as f32),
                ffi::ST_FLOAT64_LSB => buffer.cast::<f64>().add(i).write_unaligned(x),
                ffi::ST_INT16_LSB => buffer
                    .cast::<i16>()
                    .add(i)
                    .write_unaligned((x * s).round() as i16),
                ffi::ST_INT24_LSB => {
                    let v = ((x * s).round() as i32).to_le_bytes();
                    ptr::copy_nonoverlapping(v.as_ptr(), buffer.cast::<u8>().add(i * 3), 3);
                }
                ffi::ST_INT32_LSB
                | ffi::ST_INT32_LSB16
                | ffi::ST_INT32_LSB18
                | ffi::ST_INT32_LSB20
                | ffi::ST_INT32_LSB24 => buffer
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned((x * s).round() as i32),
                _ => {}
            }
        }
    }
}

/// How a failed driver `init` reads to the user (sweep M5c).
///
/// ASIO's `init` answers only true or false, and the driver's own message is
/// all it offers: one that names the device as taken means another
/// application holds it, so it says so instead of reading as a missing
/// device. Anything else is a plain driver error carrying that message.
fn init_error(message: String) -> AudioError {
    // The driver's words only: ASIO hands back no code to tell these apart.
    let said = message.to_lowercase();
    let busy = ["in use", "busy", "already open"]
        .iter()
        .any(|phrase| said.contains(phrase));
    if busy {
        AudioError::DeviceBusy(message)
    } else {
        AudioError::Driver(message)
    }
}

/// ASIO devices. Construct on the audio control thread: it enters a COM
/// apartment that the drivers live in.
pub struct AsioBackend {
    window: HiddenWindow,
    _com: ComApartment,
}

impl AsioBackend {
    pub fn new() -> Result<Self, String> {
        let com = ComApartment::enter()?;
        let window = HiddenWindow::create()?;
        Ok(Self { window, _com: com })
    }

    /// Registered drivers, without loading any (loading can hang or crash;
    /// full capability queries run sandboxed, see P-ASIO).
    pub fn registered() -> Vec<(DeviceId, String)> {
        registry::installed_drivers()
            .into_iter()
            .map(|d| {
                (
                    DeviceId {
                        kind: DriverKind::Asio,
                        id: d.clsid,
                    },
                    d.key_name,
                )
            })
            .collect()
    }

    fn load(&self, clsid: &str) -> Result<Driver, AudioError> {
        let driver = Driver::create(clsid).map_err(AudioError::DeviceNotFound)?;
        driver.init(self.window.0).map_err(init_error)?;
        Ok(driver)
    }

    /// Loads one driver and reports what it offers. Loading runs driver
    /// code, which can hang or crash when its hardware is missing: the app
    /// runs this in a helper process (M2 slice 8).
    pub fn probe(&self, id: &DeviceId, name: String) -> Result<DeviceInfo, AudioError> {
        let mut info = DeviceInfo {
            id: id.clone(),
            name,
            input_channels: Vec::new(),
            output_channels: Vec::new(),
            sample_rates: Vec::new(),
            current_sample_rate: None,
            buffer_sizes: None,
        };
        let driver = self.load(&id.id)?;
        if let Ok((inputs, outputs)) = driver.channels() {
            let names = |count: i32, input: bool| {
                (0..count)
                    .map(|c| {
                        driver
                            .channel_info(c, input)
                            .map(|i| ffi::c_string(&i.name))
                            .unwrap_or_default()
                    })
                    .collect()
            };
            info.input_channels = names(inputs, true);
            info.output_channels = names(outputs, false);
        }
        info.sample_rates = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000]
            .into_iter()
            .filter(|&r| driver.can_sample_rate(f64::from(r)))
            .collect();
        info.current_sample_rate = driver.sample_rate().ok().map(|r| r as u32);
        info.buffer_sizes = driver.buffer_sizes().ok().map(|b| BufferSizes {
            min: b.min.max(1) as u32,
            max: b.max.max(1) as u32,
            preferred: b.preferred.max(1) as u32,
            granularity: if b.granularity < 0 {
                None
            } else {
                Some(b.granularity as u32)
            },
        });
        Ok(info)
    }
}

impl Backend for AsioBackend {
    /// Loads each driver in-process. The device selector (slice 8) will run
    /// this sandboxed, as P-ASIO did.
    fn devices(&mut self) -> Vec<DeviceInfo> {
        Self::registered()
            .into_iter()
            .filter_map(|(id, name)| self.probe(&id, name).ok())
            .collect()
    }

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        match self.open_stream(config) {
            Ok(prepared) => Ok(prepared.start(callback)),
            Err(error) => Err(OpenError { error, callback }),
        }
    }

    fn pump(&mut self) {
        pump_messages();
    }
}

/// A driver with buffers created but not yet started.
struct Prepared {
    driver: Driver,
    info: StreamInfo,
    inputs: Vec<Channel>,
    outputs: Vec<Channel>,
    use_output_ready: bool,
}

/// Runs `dispose` when dropped, unless disarmed: every way out of an open that
/// has created the driver's buffers gives them back, so the driver is not left
/// holding them for the next open to find busy (sweep M5b).
struct DisposeOnDrop<F: FnMut()> {
    dispose: Option<F>,
}

impl<F: FnMut()> DisposeOnDrop<F> {
    fn new(dispose: F) -> Self {
        Self {
            dispose: Some(dispose),
        }
    }

    /// Keeps the buffers: the stream owns them from here.
    fn disarm(mut self) {
        self.dispose = None;
    }
}

impl<F: FnMut()> Drop for DisposeOnDrop<F> {
    fn drop(&mut self) {
        if let Some(mut dispose) = self.dispose.take() {
            dispose();
        }
    }
}

impl AsioBackend {
    fn open_stream(&self, config: &StreamConfig) -> Result<Prepared, AudioError> {
        let asio = |e: ffi::AsioError| match e {
            ffi::ASE_NOT_PRESENT => AudioError::DeviceNotPresent(ffi::error_name(e).into()),
            other => AudioError::Driver(ffi::error_name(other).into()),
        };
        let driver = self.load(&config.device.id)?;
        let (input_count, output_count) = driver.channels().map_err(asio)?;
        let input_channels = config.inputs_on(input_count.max(0) as usize);
        if let Some(&bad) = input_channels
            .iter()
            .find(|&&c| c >= input_count.max(0) as usize)
            .or_else(|| {
                config
                    .output_channels
                    .iter()
                    .find(|&&c| c >= output_count.max(0) as usize)
            })
        {
            return Err(AudioError::Unsupported(format!(
                "channel {bad} does not exist"
            )));
        }
        if let Some(rate) = config.sample_rate {
            let current = driver.sample_rate().map_err(asio)?;
            if (current - f64::from(rate)).abs() > 0.5 {
                if !driver.can_sample_rate(f64::from(rate)) {
                    return Err(AudioError::Unsupported(format!("{rate} Hz")));
                }
                driver.set_sample_rate(f64::from(rate)).map_err(asio)?;
            }
        }
        let sizes = driver.buffer_sizes().map_err(asio)?;
        let frames = config.buffer_frames.map_or(sizes.preferred, |f| f as i32);
        let allowed = BufferSizes {
            min: sizes.min.max(1) as u32,
            max: sizes.max.max(1) as u32,
            preferred: sizes.preferred.max(1) as u32,
            granularity: if sizes.granularity < 0 {
                None
            } else {
                Some(sizes.granularity as u32)
            },
        };
        if !allowed.allows(frames as u32) {
            return Err(AudioError::Unsupported(format!("buffer size {frames}")));
        }
        let mut infos: Vec<AsioBufferInfo> = input_channels
            .iter()
            .map(|&c| (1, c))
            .chain(config.output_channels.iter().map(|&c| (0, c)))
            .map(|(is_input, c)| AsioBufferInfo {
                is_input,
                channel_num: c as i32,
                buffers: [ptr::null_mut(); 2],
            })
            .collect();
        driver
            .create_buffers(&mut infos, frames, &CALLBACKS)
            .map_err(asio)?;
        // From here a failure leaves the driver without buffers, so the next
        // open does not find it busy with the ones this one made (sweep M5b).
        let created = DisposeOnDrop::new(|| {
            let _ = driver.dispose_buffers();
        });
        let channel = |info: &AsioBufferInfo| -> Result<Channel, AudioError> {
            let sample_type = driver
                .channel_info(info.channel_num, info.is_input == 1)
                .map_err(asio)?
                .sample_type;
            if !supported(sample_type) {
                return Err(AudioError::Unsupported(format!(
                    "sample format {}",
                    ffi::sample_type_name(sample_type)
                )));
            }
            Ok(Channel {
                buffers: info.buffers,
                sample_type,
            })
        };
        let split = input_channels.len();
        let inputs = infos[..split]
            .iter()
            .map(&channel)
            .collect::<Result<Vec<_>, _>>();
        let outputs = infos[split..]
            .iter()
            .map(&channel)
            .collect::<Result<Vec<_>, _>>();
        let (inputs, outputs) = (inputs?, outputs?);
        for output in &outputs {
            for half in output.buffers {
                // SAFETY: fresh driver buffers of `frames` samples.
                unsafe { write_samples(half, output.sample_type, &vec![0.0; frames as usize]) };
            }
        }
        let (input_latency, output_latency) = driver.latencies().unwrap_or((frames, frames));
        let sample_rate = driver.sample_rate().map_err(asio)? as u32;
        let use_output_ready = driver.output_ready() == ffi::ASE_OK;
        created.disarm();
        Ok(Prepared {
            info: StreamInfo {
                sample_rate,
                buffer_frames: frames as u32,
                input_latency: input_latency.max(0) as u32,
                output_latency: output_latency.max(0) as u32,
                input_channels: inputs.len(),
                output_channels: outputs.len(),
                silent_inputs: Vec::new(),
            },
            driver,
            inputs,
            outputs,
            use_output_ready,
        })
    }
}

impl Prepared {
    fn start(self, callback: Box<dyn AudioCallback>) -> Box<dyn Stream> {
        let frames = self.info.buffer_frames as usize;
        let epoch = Instant::now();
        let state = Box::into_raw(Box::new(StreamState {
            driver: self.driver.raw(),
            frames,
            sample_rate: self.info.sample_rate,
            input_buffer: vec![0.0; self.inputs.len() * frames],
            output_buffer: vec![0.0; self.outputs.len() * frames],
            inputs: self.inputs,
            outputs: self.outputs,
            callback,
            use_output_ready: self.use_output_ready,
            last_callback_ns: AtomicU64::new(0),
            epoch,
        }));
        RESET_REQUESTED.store(false, Ordering::Release);
        RESYNC_REQUESTED.store(false, Ordering::Release);
        FAULTED.store(false, Ordering::Release);
        ACTIVE.store(state, Ordering::Release);
        let started = self.driver.start().is_ok();
        Box::new(AsioStream {
            driver: Some(self.driver),
            state,
            info: self.info,
            started_at: epoch,
            failed_to_start: !started,
        })
    }
}

struct AsioStream {
    driver: Option<Driver>,
    state: *mut StreamState,
    info: StreamInfo,
    started_at: Instant,
    failed_to_start: bool,
}

impl AsioStream {
    /// No callback for this long means the driver has stopped delivering.
    fn watchdog(&self) -> Duration {
        let period = f64::from(self.info.buffer_frames) / f64::from(self.info.sample_rate.max(1));
        Duration::from_secs_f64((period * 50.0).max(0.5))
    }

    /// Stops the driver and reclaims the state, waiting for a callback that is
    /// still running to end first. Returns the callback; a silent one when a
    /// callback would not end within the bound, in which case its state is
    /// leaked rather than freed under it (sweep M5a).
    ///
    /// Runs once: the driver is taken, and a second call has nothing to do.
    fn shutdown(&mut self) -> Box<dyn AudioCallback> {
        let Some(driver) = self.driver.take() else {
            return crate::backend::silent();
        };
        // A stop that fails is not fatal: the wait below is the proof that no
        // callback is running, and a driver that would not stop is remembered
        // in the fault flag the stream reports, rather than trusted silently.
        if driver.stop().is_err() {
            FAULTED.store(true, Ordering::SeqCst);
        }
        // No callback that starts now can reach the state. One that already
        // did is counted in `IN_FLIGHT`, and the wait lets it finish.
        ACTIVE.store(ptr::null_mut(), Ordering::SeqCst);
        // A few periods is as long as a driver has ever taken to finish its
        // last block; past it the state is leaked, which is safe, where
        // freeing it is a use-after-free.
        let period = f64::from(self.info.buffer_frames) / f64::from(self.info.sample_rate.max(1));
        let bound = Duration::from_secs_f64(period * 3.0) + Duration::from_millis(10);
        if !IN_FLIGHT.wait(bound) {
            // A callback is still running: its state, the driver's buffers and
            // the driver itself are left alone for good. Leaking a few
            // kilobytes keeps the session up; freeing them would crash the
            // process. The callback cannot be handed back (a running block is
            // using it), so a silent one stands in.
            std::mem::forget(driver);
            self.state = ptr::null_mut();
            return crate::backend::silent();
        }
        let _ = driver.dispose_buffers();
        drop(driver);
        // SAFETY: the state came from `Box::into_raw`, no callback can reach
        // it now, and it is taken out exactly once.
        let state = unsafe { Box::from_raw(self.state) };
        self.state = ptr::null_mut();
        state.callback
    }
}

impl Stream for AsioStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn control_panel(&mut self) -> Result<(), String> {
        let driver = self.driver.as_ref().ok_or("the driver is not loaded")?;
        driver
            .control_panel()
            .map_err(|e| format!("the driver has no control panel ({e:?})"))
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        if FAULTED.swap(false, Ordering::SeqCst) {
            // A callback panicked, or the driver would not stop: the stream is
            // not to be trusted, and the supervisor closes and reopens it.
            return Some(StreamEvent::Stalled);
        }
        if RESET_REQUESTED.swap(false, Ordering::AcqRel) {
            return Some(StreamEvent::ResetRequested);
        }
        if self.failed_to_start {
            return Some(StreamEvent::Stalled);
        }
        // SAFETY: the atomic may be read from any thread.
        let last_ns = unsafe { (*self.state).last_callback_ns.load(Ordering::Relaxed) };
        let since_start = self.started_at.elapsed();
        let since_last = since_start.saturating_sub(Duration::from_nanos(last_ns));
        (since_last > self.watchdog()).then_some(StreamEvent::Stalled)
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        self.shutdown()
    }
}

impl Drop for AsioStream {
    fn drop(&mut self) {
        // A stream dropped without `close` (an early return, a failed open
        // that already started, a panic) still stops its driver and reclaims
        // its state instead of leaving the device running (sweep M5d). After
        // `close` the driver is gone, so this has nothing left to do.
        drop(self.shutdown());
    }
}

#[cfg(test)]
mod tests {
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
}
