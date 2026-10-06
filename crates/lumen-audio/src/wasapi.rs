//! WASAPI backend (Windows): shared and exclusive mode, event-driven, output
//! only for now (input arrives with the device selector, M2 slice 8).
//!
//! Each stream runs on its own render thread with the "Pro Audio" MMCSS
//! class. Shared mode runs at the rate and period Windows chooses; Lumen
//! never resamples behind the user's back, so asking for another rate is an
//! `Unsupported` error that names the fix. Exclusive mode opens the device at
//! the requested rate and buffer size.
//!
//! Exclusive mode starts event-driven (one block of latency) and checks the
//! device clock while playing silence. Some USB drivers signal events far too
//! slowly in exclusive mode (a Focusrite ran at 60% of real time, MT-AUDIO-4);
//! those devices are reopened in polling mode, which keeps at most two blocks
//! queued.
//!
//! Device loss shows up either as an error from the audio client (reported
//! as a reset request; the reopen then fails and the supervisor reports
//! `Lost`) or as a device that stops consuming audio (reported as a stall).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wasapi::{
    AudioClient, AudioRenderClient, Device, DeviceEnumerator, DeviceState, Direction, Handle,
    SampleType, StreamMode, WasapiError, WaveFormat, calculate_period_100ns,
};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
};

use crate::backend::{Backend, OpenError, Stream, StreamEvent};
use crate::convert::{SampleFormat, interleave_bytes};
use crate::types::{
    AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind, StreamConfig,
    StreamInfo,
};

// HRESULTs from audioclient.h and winerror.h.
const DEVICE_INVALIDATED: u32 = 0x8889_0004;
const UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const DEVICE_IN_USE: u32 = 0x8889_000A;
const EXCLUSIVE_MODE_NOT_ALLOWED: u32 = 0x8889_000E;
const BUFFER_SIZE_ERROR: u32 = 0x8889_0016;
const BUFFER_SIZE_NOT_ALIGNED: u32 = 0x8889_0019;
const INVALID_DEVICE_PERIOD: u32 = 0x8889_0020;
const NOT_FOUND: u32 = 0x8007_0490;
// ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND: seen when a USB device is
// unplugged mid-stream (MT-AUDIO-4).
const FILE_NOT_FOUND: u32 = 0x8007_0002;
const PATH_NOT_FOUND: u32 = 0x8007_0003;

/// Rates offered in exclusive mode when the device accepts them.
const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Exclusive-mode formats in order of preference:
/// (format, container bits, valid bits, sample type).
const FORMATS: [(SampleFormat, usize, usize, SampleType); 5] = [
    (SampleFormat::F32, 32, 32, SampleType::Float),
    (SampleFormat::I32, 32, 32, SampleType::Int),
    (SampleFormat::I32, 32, 24, SampleType::Int),
    (SampleFormat::I24, 24, 24, SampleType::Int),
    (SampleFormat::I16, 16, 16, SampleType::Int),
];

/// Largest exclusive-mode buffer offered (about 170 ms at 48 kHz).
const MAX_EXCLUSIVE_FRAMES: u32 = 8192;

fn hresult(error: &WasapiError) -> Option<u32> {
    match error {
        WasapiError::Windows(e) => Some(e.code().0 as u32),
        _ => None,
    }
}

fn map_error(error: &WasapiError, name: &str) -> AudioError {
    match hresult(error) {
        Some(DEVICE_IN_USE | EXCLUSIVE_MODE_NOT_ALLOWED) => AudioError::DeviceBusy(name.into()),
        Some(DEVICE_INVALIDATED | NOT_FOUND | FILE_NOT_FOUND | PATH_NOT_FOUND) => {
            AudioError::DeviceNotPresent(name.into())
        }
        Some(UNSUPPORTED_FORMAT | BUFFER_SIZE_ERROR | INVALID_DEVICE_PERIOD) => {
            AudioError::Unsupported(format!("{name}: {error}"))
        }
        _ => AudioError::Driver(format!("{name}: {error}")),
    }
}

/// 100-ns units to whole frames, rounded to nearest.
fn hns_to_frames(hns: i64, rate: u32) -> u32 {
    ((hns as f64 * f64::from(rate) / 1e7).round()) as u32
}

fn exclusive_format(
    client: &AudioClient,
    rate: u32,
    channels: usize,
) -> Option<(WaveFormat, SampleFormat)> {
    FORMATS.iter().find_map(|(format, store, valid, kind)| {
        let wave = WaveFormat::new(*store, *valid, kind, rate as usize, channels, None);
        client
            .is_supported_exclusive_with_quirks(&wave)
            .ok()
            .map(|accepted| (accepted, *format))
    })
}

/// WASAPI in shared or exclusive mode. Create it on the thread that will use
/// it (the supervisor's control thread); it joins the multithreaded COM
/// apartment there.
pub struct WasapiBackend {
    exclusive: bool,
}

impl WasapiBackend {
    pub fn new(exclusive: bool) -> Self {
        // S_FALSE (already initialised) and RPC_E_CHANGED_MODE are both fine:
        // the audio interfaces are free-threaded.
        let _ = wasapi::initialize_mta();
        Self { exclusive }
    }

    /// The output Windows uses by default (Sound settings), in shared mode.
    /// Runs on its own short-lived thread, so it works from any thread
    /// whatever its COM apartment.
    pub fn default_output() -> Option<DeviceId> {
        std::thread::spawn(|| {
            let _ = wasapi::initialize_mta();
            let device = DeviceEnumerator::new()
                .ok()?
                .get_default_device(&Direction::Render)
                .ok()?;
            Some(DeviceId {
                kind: DriverKind::WasapiShared,
                id: device.get_id().ok()?,
            })
        })
        .join()
        .ok()
        .flatten()
    }

    fn kind(&self) -> DriverKind {
        if self.exclusive {
            DriverKind::WasapiExclusive
        } else {
            DriverKind::WasapiShared
        }
    }

    fn describe(&self, device: &Device) -> Option<DeviceInfo> {
        let id = device.get_id().ok()?;
        let name = device.get_friendlyname().ok()?;
        let client = device.get_iaudioclient().ok()?;
        let mix = client.get_mixformat().ok()?;
        let channels = usize::from(mix.get_nchannels());
        let (default_period, min_period) = client.get_device_period().ok()?;
        let (sample_rates, rate, buffer_sizes) = if self.exclusive {
            let rate = device
                .get_device_format()
                .map(|f| f.get_samplespersec())
                .unwrap_or(mix.get_samplespersec());
            let rates = RATES
                .into_iter()
                .filter(|&r| exclusive_format(&client, r, channels).is_some())
                .collect();
            let min = hns_to_frames(min_period, rate).max(1);
            let sizes = BufferSizes {
                min,
                max: MAX_EXCLUSIVE_FRAMES.max(min),
                preferred: hns_to_frames(default_period, rate).max(min),
                granularity: Some(1),
            };
            (rates, rate, sizes)
        } else {
            let rate = mix.get_samplespersec();
            let frames = hns_to_frames(default_period, rate);
            let sizes = BufferSizes {
                min: frames,
                max: frames,
                preferred: frames,
                granularity: Some(0),
            };
            (vec![rate], rate, sizes)
        };
        Some(DeviceInfo {
            id: DeviceId {
                kind: self.kind(),
                id,
            },
            name,
            input_channels: Vec::new(),
            output_channels: (1..=channels).map(|c| format!("Out {c}")).collect(),
            sample_rates,
            current_sample_rate: Some(rate),
            buffer_sizes: Some(buffer_sizes),
        })
    }
}

impl Backend for WasapiBackend {
    fn devices(&mut self) -> Vec<DeviceInfo> {
        let Ok(enumerator) = DeviceEnumerator::new() else {
            return Vec::new();
        };
        let Ok(collection) = enumerator.get_device_collection(&Direction::Render) else {
            return Vec::new();
        };
        (&collection)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|d| matches!(d.get_state(), Ok(DeviceState::Active)))
            .filter_map(|d| self.describe(&d))
            .collect()
    }

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        if config.device.kind != self.kind() {
            return Err(OpenError {
                error: AudioError::DeviceNotFound(config.device.id.clone()),
                callback,
            });
        }
        if !config.input_channels.is_empty() {
            return Err(OpenError {
                error: AudioError::Unsupported(
                    "WASAPI input is not available yet; it arrives with the device selector".into(),
                ),
                callback,
            });
        }
        let flags = Arc::new(Flags::default());
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = {
            let config = config.clone();
            let flags = Arc::clone(&flags);
            let exclusive = self.exclusive;
            std::thread::Builder::new()
                .name("lumen-audio-wasapi".into())
                .spawn(move || render_thread(&config, exclusive, callback, &flags, &ready_tx))
                .expect("spawn WASAPI render thread")
        };
        match ready_rx.recv() {
            Ok(Ok(info)) => Ok(Box::new(WasapiStream {
                info,
                flags,
                thread: Some(thread),
            })),
            Ok(Err((error, callback))) => {
                let _ = thread.join();
                Err(OpenError { error, callback })
            }
            Err(_) => panic!("the WASAPI render thread died while opening the device"),
        }
    }
}

#[derive(Default)]
struct Flags {
    stop: AtomicBool,
    reset: AtomicBool,
    stalled: AtomicBool,
}

type Ready = SyncSender<Result<StreamInfo, (AudioError, Box<dyn AudioCallback>)>>;

/// Owns everything COM-related for one stream; returns the callback when
/// the stream ends (`None` if it never started).
fn render_thread(
    config: &StreamConfig,
    exclusive: bool,
    mut callback: Box<dyn AudioCallback>,
    flags: &Flags,
    ready: &Ready,
) -> Option<Box<dyn AudioCallback>> {
    let _ = wasapi::initialize_mta();
    let _priority = Mmcss::join();
    let opened = Render::open(config, exclusive, Timing::Events).and_then(|mut r| {
        r.start()?;
        if !exclusive || r.keeps_time() {
            return Ok(r);
        }
        let _ = r.client.stop_stream();
        drop(r);
        let mut polled = Render::open(config, true, Timing::Polling)?;
        polled.start()?;
        Ok(polled)
    });
    let mut render = match opened {
        Ok(render) => render,
        Err(error) => {
            let _ = ready.send(Err((error, callback)));
            return None;
        }
    };
    let _ = ready.send(Ok(render.info.clone()));
    render.run(callback.as_mut(), flags);
    let _ = render.client.stop_stream();
    Some(callback)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Timing {
    /// Wait for the device's buffer event.
    Events,
    /// Poll the buffer level (exclusive mode on drivers whose events are
    /// unreliable).
    Polling,
}

/// How long exclusive mode plays silence to check the device clock.
const CALIBRATION: Duration = Duration::from_millis(250);

/// Whether a device clock that advanced `device_seconds` during
/// `wall_seconds` of real time is keeping time (within 10%).
fn keeps_real_time(device_seconds: f64, wall_seconds: f64) -> bool {
    wall_seconds > 0.0 && (0.9..1.1).contains(&(device_seconds / wall_seconds))
}

struct Render {
    client: AudioClient,
    render: AudioRenderClient,
    /// `None` in polling mode.
    event: Option<Handle>,
    /// Most frames to keep queued on the device.
    fill_limit: usize,
    name: String,
    format: SampleFormat,
    device_channels: usize,
    block_align: usize,
    /// Device channel for each callback output channel.
    map: Vec<usize>,
    /// Frames per callback block.
    period: usize,
    info: StreamInfo,
    outputs: Vec<f32>,
    bytes: Vec<u8>,
    position: u64,
}

impl Render {
    fn open(config: &StreamConfig, exclusive: bool, timing: Timing) -> Result<Self, AudioError> {
        let id = &config.device.id;
        let enumerator =
            DeviceEnumerator::new().map_err(|e| AudioError::Driver(format!("WASAPI: {e}")))?;
        let device = enumerator.get_device(id).map_err(|e| match hresult(&e) {
            Some(NOT_FOUND) => AudioError::DeviceNotFound(id.clone()),
            _ => map_error(&e, id),
        })?;
        let name = device.get_friendlyname().unwrap_or_else(|_| id.clone());
        if !matches!(device.get_state(), Ok(DeviceState::Active)) {
            return Err(AudioError::DeviceNotPresent(name));
        }
        let fail = |e: WasapiError| map_error(&e, &name);
        let mut client = device.get_iaudioclient().map_err(fail)?;
        let mix = client.get_mixformat().map_err(fail)?;
        let device_channels = usize::from(mix.get_nchannels());
        if let Some(&bad) = config
            .output_channels
            .iter()
            .find(|&&c| c >= device_channels)
        {
            return Err(AudioError::Unsupported(format!(
                "output {} ({name} has {device_channels})",
                bad + 1
            )));
        }
        let (default_period, min_period) = client.get_device_period().map_err(fail)?;

        let exclusive_mode = |period_hns: i64| match timing {
            Timing::Events => StreamMode::EventsExclusive { period_hns },
            Timing::Polling => StreamMode::PollingExclusive {
                buffer_duration_hns: 2 * period_hns,
                period_hns,
            },
        };
        // The period Windows actually uses, in 100-ns units.
        let mut period_hns = default_period;
        let (rate, format, block_align) = if exclusive {
            let rate = config.sample_rate.unwrap_or_else(|| {
                device
                    .get_device_format()
                    .map(|f| f.get_samplespersec())
                    .unwrap_or(mix.get_samplespersec())
            });
            let (wave, format) = exclusive_format(&client, rate, device_channels)
                .ok_or_else(|| AudioError::Unsupported(format!("{rate} Hz on {name}")))?;
            if let Some(frames) = config.buffer_frames {
                let wanted = calculate_period_100ns(i64::from(frames), i64::from(rate));
                if wanted < min_period {
                    return Err(AudioError::Unsupported(format!(
                        "buffer size {frames} (the minimum is {})",
                        hns_to_frames(min_period, rate)
                    )));
                }
                period_hns = client
                    .calculate_aligned_period_near(wanted, Some(128), &wave)
                    .map_err(fail)?;
            }
            let mode = exclusive_mode(period_hns);
            if let Err(e) = client.initialize_client(&wave, &Direction::Render, &mode) {
                if hresult(&e) != Some(BUFFER_SIZE_NOT_ALIGNED) {
                    return Err(fail(e));
                }
                // The documented recovery: ask for the size the device
                // aligned to, on a fresh client.
                let frames = client.get_buffer_size().map_err(fail)?;
                client = device.get_iaudioclient().map_err(fail)?;
                period_hns = calculate_period_100ns(i64::from(frames), i64::from(rate));
                client
                    .initialize_client(&wave, &Direction::Render, &exclusive_mode(period_hns))
                    .map_err(fail)?;
            }
            (rate, format, wave.get_blockalign() as usize)
        } else {
            let rate = mix.get_samplespersec();
            if let Some(wanted) = config.sample_rate.filter(|&r| r != rate) {
                return Err(AudioError::Unsupported(format!(
                    "{wanted} Hz: Windows runs {name} at {rate} Hz in shared mode. Change it in \
                     the Windows sound settings or use WASAPI exclusive mode"
                )));
            }
            let frames = hns_to_frames(default_period, rate);
            if let Some(wanted) = config.buffer_frames.filter(|&f| f != frames) {
                return Err(AudioError::Unsupported(format!(
                    "buffer size {wanted}: Windows uses {frames} in shared mode"
                )));
            }
            let wave = WaveFormat::new(
                32,
                32,
                &SampleType::Float,
                rate as usize,
                device_channels,
                None,
            );
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: default_period,
            };
            client
                .initialize_client(&wave, &Direction::Render, &mode)
                .map_err(fail)?;
            (rate, SampleFormat::F32, wave.get_blockalign() as usize)
        };

        let polling = exclusive && timing == Timing::Polling;
        let event = if polling {
            None
        } else {
            Some(client.set_get_eventhandle().map_err(fail)?)
        };
        let render = client.get_audiorenderclient().map_err(fail)?;
        let buffer = client.get_buffer_size().map_err(fail)? as usize;
        let period = if exclusive && !polling {
            buffer
        } else {
            (hns_to_frames(period_hns, rate) as usize).min(buffer)
        };
        // Polling-mode buffers are much larger than asked for (8 periods on
        // a Focusrite); queue only two blocks to keep latency low.
        let fill_limit = if polling {
            (2 * period).min(buffer)
        } else {
            buffer
        };
        let outputs = config.output_channels.len();
        Ok(Self {
            client,
            render,
            event,
            fill_limit,
            name,
            format,
            device_channels,
            block_align,
            map: config.output_channels.clone(),
            period,
            info: StreamInfo {
                sample_rate: rate,
                buffer_frames: period as u32,
                input_latency: 0,
                output_latency: fill_limit as u32,
                input_channels: 0,
                output_channels: outputs,
            },
            outputs: vec![0.0; outputs * period],
            bytes: vec![0; buffer * block_align],
            position: 0,
        })
    }

    /// Pre-fills the device buffer with silence and starts the stream.
    fn start(&mut self) -> Result<(), AudioError> {
        let name = self.name.clone();
        let fail = |e: WasapiError| map_error(&e, &name);
        let frames = (self.client.get_available_space_in_frames().map_err(fail)? as usize)
            .min(self.fill_limit);
        self.write_silence(frames).map_err(fail)?;
        self.client.start_stream().map_err(fail)
    }

    fn write_silence(&mut self, frames: usize) -> Result<(), WasapiError> {
        let silence = &mut self.bytes[..frames * self.block_align];
        silence.fill(0);
        self.render.write_to_device(frames, silence, None)
    }

    /// Plays silence for [`CALIBRATION`] and checks that the device clock
    /// keeps real time (within 10%). Event-driven streams only.
    fn keeps_time(&mut self) -> bool {
        let Some(event) = self.event.as_ref() else {
            return true;
        };
        let Ok(clock) = self.client.get_audioclock() else {
            return true;
        };
        let Ok(frequency) = clock.get_frequency() else {
            return true;
        };
        let timeout = (2 * CALIBRATION.as_millis()) as u32;
        if event.wait_for_event(timeout).is_err() {
            return false;
        }
        let Ok((first, _)) = clock.get_position() else {
            return true;
        };
        let started = Instant::now();
        while started.elapsed() < CALIBRATION {
            if self.write_silence(self.period).is_err() {
                return true; // a real failure; the run loop reports it
            }
            let Some(event) = self.event.as_ref() else {
                return true;
            };
            if event.wait_for_event(timeout).is_err() {
                return false;
            }
        }
        let Ok((last, _)) = clock.get_position() else {
            return true;
        };
        let device_seconds = last.saturating_sub(first) as f64 / frequency as f64;
        keeps_real_time(device_seconds, started.elapsed().as_secs_f64())
    }

    fn run(&mut self, callback: &mut dyn AudioCallback, flags: &Flags) {
        let period_ms = self.period as u64 * 1000 / u64::from(self.info.sample_rate.max(1));
        let timeout = (period_ms * 50).max(500) as u32;
        if self.event.is_none() {
            return self.poll(callback, flags, Duration::from_millis(u64::from(timeout)));
        }
        while !flags.stop.load(Ordering::Acquire) {
            let Some(event) = self.event.as_ref() else {
                return;
            };
            if event.wait_for_event(timeout).is_err() {
                flags.stalled.store(true, Ordering::Release);
                return;
            }
            let Ok(mut available) = self.client.get_available_space_in_frames() else {
                flags.reset.store(true, Ordering::Release);
                return;
            };
            while available as usize >= self.period {
                if self.render_block(callback).is_err() {
                    flags.reset.store(true, Ordering::Release);
                    return;
                }
                available -= self.period as u32;
            }
        }
    }

    /// Polling loop: keeps between one and two blocks queued, waking four
    /// times per block. A device whose queue stops draining for `stall` is
    /// reported as stalled.
    fn poll(&mut self, callback: &mut dyn AudioCallback, flags: &Flags, stall: Duration) {
        let block = self.period as f64 / f64::from(self.info.sample_rate.max(1));
        let tick = Duration::from_secs_f64((block / 4.0).max(0.0005));
        let mut last_padding = None;
        let mut last_progress = Instant::now();
        while !flags.stop.load(Ordering::Acquire) {
            std::thread::sleep(tick);
            let Ok(padding) = self.client.get_current_padding() else {
                flags.reset.store(true, Ordering::Release);
                return;
            };
            let mut queued = padding as usize;
            if last_padding != Some(padding) {
                last_progress = Instant::now();
            } else if last_progress.elapsed() > stall {
                flags.stalled.store(true, Ordering::Release);
                return;
            }
            while queued + self.period <= self.fill_limit {
                if self.render_block(callback).is_err() {
                    flags.reset.store(true, Ordering::Release);
                    return;
                }
                queued += self.period;
            }
            last_padding = Some(queued as u32);
        }
    }

    fn render_block(&mut self, callback: &mut dyn AudioCallback) -> Result<(), WasapiError> {
        self.outputs.fill(0.0);
        let mut block = Block {
            sample_rate: self.info.sample_rate,
            frames: self.period,
            inputs: &[],
            outputs: &mut self.outputs,
            input_channels: 0,
            output_channels: self.map.len(),
            position: Some(self.position),
        };
        callback.process(&mut block);
        let bytes = &mut self.bytes[..self.period * self.block_align];
        interleave_bytes(
            &self.outputs,
            self.period,
            &self.map,
            self.device_channels,
            self.format,
            bytes,
        );
        self.render.write_to_device(self.period, bytes, None)?;
        self.position += self.period as u64;
        Ok(())
    }
}

/// Joins the "Pro Audio" MMCSS class for the life of the guard.
struct Mmcss(HANDLE);

impl Mmcss {
    fn join() -> Option<Self> {
        let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
        let mut index = 0;
        // SAFETY: `task` is a NUL-terminated UTF-16 string that outlives the
        // call; `index` is a valid out-pointer.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for Mmcss {
    fn drop(&mut self) {
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this
        // thread and is reverted once.
        unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

struct WasapiStream {
    info: StreamInfo,
    flags: Arc<Flags>,
    thread: Option<JoinHandle<Option<Box<dyn AudioCallback>>>>,
}

impl Stream for WasapiStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        if self.flags.stalled.load(Ordering::Acquire) {
            Some(StreamEvent::Stalled)
        } else if self.flags.reset.swap(false, Ordering::AcqRel) {
            Some(StreamEvent::ResetRequested)
        } else {
            None
        }
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        self.flags.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .and_then(|t| t.join().ok().flatten())
            .expect("the WASAPI render thread returns the callback")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_convert_to_frames() {
        assert_eq!(hns_to_frames(100_000, 48_000), 480);
        assert_eq!(hns_to_frames(100_000, 44_100), 441);
        assert_eq!(hns_to_frames(30_000, 48_000), 144);
    }

    #[test]
    fn the_clock_check_catches_a_slow_driver() {
        // MT-AUDIO-4: the Focusrite's exclusive-mode events ran its clock at
        // 26,623 of 44,100 frames per second.
        assert!(!keeps_real_time(26_623.0 / 44_100.0, 1.0));
        assert!(keeps_real_time(44_070.0 / 44_100.0, 1.0));
        assert!(keeps_real_time(0.25, 0.25));
        assert!(!keeps_real_time(0.35, 0.25));
        assert!(!keeps_real_time(0.0, 0.0));
    }

    #[test]
    fn the_default_output_is_one_of_the_listed_devices() {
        // CI runners may have no audio device at all; then there is no default.
        let Some(default) = WasapiBackend::default_output() else {
            return;
        };
        let listed = WasapiBackend::new(false).devices();
        assert!(listed.iter().any(|d| d.id == default), "{default:?}");
    }

    /// Runs wherever Windows audio exists; CI runners may have no devices,
    /// so this only checks that enumeration is safe and consistent.
    #[test]
    fn enumeration_is_safe_and_consistent() {
        for exclusive in [false, true] {
            let mut backend = WasapiBackend::new(exclusive);
            for device in backend.devices() {
                assert_eq!(device.id.kind, backend.kind());
                assert!(!device.output_channels.is_empty(), "{device:?}");
                let sizes = device.buffer_sizes.expect("WASAPI reports buffer sizes");
                assert!(sizes.allows(sizes.preferred), "{device:?}");
            }
        }
    }
}
