//! Portable backend through cpal: CoreAudio on macOS, ALSA on Linux. Output
//! only for now (input arrives with the device selector, M2 slice 8).
//!
//! Compiled and unit-tested in CI on macOS and Linux; not yet tested on
//! hardware there (Windows is the first-class platform).
//!
//! cpal owns the audio thread and wants the callback by value, so the
//! callback lives in a shared slot. The audio thread takes it with
//! `try_lock`, which never blocks: the only other user is `close`, after the
//! stream has been dropped.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, SupportedBufferSize};

use crate::backend::{Backend, OpenError, Stream, StreamEvent};
use crate::convert::interleave_f32;
use crate::types::{
    AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind, StreamConfig,
    StreamInfo,
};

const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Buffer size used when the user has not chosen one.
const DEFAULT_FRAMES: u32 = 256;

fn kind() -> DriverKind {
    if cfg!(target_os = "macos") {
        DriverKind::CoreAudio
    } else {
        DriverKind::Alsa
    }
}

fn map_error(error: &cpal::Error, name: &str) -> AudioError {
    match error.kind() {
        ErrorKind::DeviceNotAvailable => AudioError::DeviceNotPresent(name.into()),
        ErrorKind::DeviceBusy => AudioError::DeviceBusy(name.into()),
        ErrorKind::UnsupportedConfig | ErrorKind::InvalidInput => {
            AudioError::Unsupported(format!("{name}: {error}"))
        }
        _ => AudioError::Driver(format!("{name}: {error}")),
    }
}

/// The platform's default cpal host.
pub struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    /// The system's default output device.
    pub fn default_output() -> Option<DeviceId> {
        let device = cpal::default_host().default_output_device()?;
        Some(DeviceId {
            kind: kind(),
            id: device.id().ok()?.to_string(),
        })
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn describe(device: &cpal::Device) -> Option<DeviceInfo> {
    let id = device.id().ok()?.to_string();
    let default = device.default_output_config().ok()?;
    let configs: Vec<_> = device.supported_output_configs().ok()?.collect();
    let channels = usize::from(default.channels());
    Some(DeviceInfo {
        id: DeviceId { kind: kind(), id },
        name: device.to_string(),
        input_channels: Vec::new(),
        output_channels: (1..=channels).map(|c| format!("Out {c}")).collect(),
        sample_rates: RATES
            .into_iter()
            .filter(|&r| {
                configs
                    .iter()
                    .any(|c| c.min_sample_rate() <= r && r <= c.max_sample_rate())
            })
            .collect(),
        current_sample_rate: Some(default.sample_rate()),
        buffer_sizes: match *default.buffer_size() {
            SupportedBufferSize::Range { min, max } => Some(BufferSizes {
                min,
                max,
                preferred: DEFAULT_FRAMES.clamp(min, max),
                granularity: Some(1),
            }),
            SupportedBufferSize::Unknown => None,
        },
    })
}

impl Backend for CpalBackend {
    fn devices(&mut self) -> Vec<DeviceInfo> {
        match self.host.output_devices() {
            Ok(devices) => devices.filter_map(|d| describe(&d)).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        let slot = Arc::new(Mutex::new(Some(callback)));
        match self.start(config, &slot) {
            Ok(stream) => Ok(stream),
            Err(error) => Err(OpenError {
                error,
                callback: take(&slot),
            }),
        }
    }
}

/// Takes the callback back once no stream can be using it.
fn take(slot: &Mutex<Option<Box<dyn AudioCallback>>>) -> Box<dyn AudioCallback> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .expect("the callback is in its slot")
}

impl CpalBackend {
    fn start(
        &self,
        config: &StreamConfig,
        slot: &Arc<Mutex<Option<Box<dyn AudioCallback>>>>,
    ) -> Result<Box<dyn Stream>, AudioError> {
        let id = &config.device.id;
        if config.device.kind != kind() {
            return Err(AudioError::DeviceNotFound(id.clone()));
        }
        if !config.input_channels.is_empty() {
            return Err(AudioError::Unsupported(
                "audio input is not available yet; it arrives with the device selector".into(),
            ));
        }
        let cpal_id: cpal::DeviceId = id
            .parse()
            .map_err(|_| AudioError::DeviceNotFound(id.clone()))?;
        // An unplugged device is simply not listed, so "absent" counts as
        // not present: the supervisor keeps retrying.
        let device = self
            .host
            .device_by_id(&cpal_id)
            .ok_or_else(|| AudioError::DeviceNotPresent(id.clone()))?;
        let name = device.to_string();
        let default = device
            .default_output_config()
            .map_err(|e| map_error(&e, &name))?;
        let device_channels = usize::from(default.channels());
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
        let sample_rate = config.sample_rate.unwrap_or(default.sample_rate());
        let buffer_size = match (config.buffer_frames, *default.buffer_size()) {
            (Some(frames), _) => BufferSize::Fixed(frames),
            (None, SupportedBufferSize::Range { min, max }) => {
                BufferSize::Fixed(DEFAULT_FRAMES.clamp(min, max))
            }
            (None, SupportedBufferSize::Unknown) => BufferSize::Default,
        };
        let frames = match buffer_size {
            BufferSize::Fixed(frames) => frames,
            // Unknown until the stream runs; blocks are sized per callback.
            BufferSize::Default => 0,
        };

        let shared = Arc::new(Shared {
            reset: AtomicBool::new(false),
            last_callback_ns: AtomicU64::new(0),
            epoch: Instant::now(),
        });
        let capacity = (frames as usize).max(4096);
        let mut render = Render {
            slot: Arc::clone(slot),
            shared: Arc::clone(&shared),
            map: config.output_channels.clone(),
            device_channels,
            sample_rate,
            outputs: vec![0.0; config.output_channels.len() * capacity],
            capacity,
            position: 0,
        };
        let errors = Arc::clone(&shared);
        let stream = device
            .build_output_stream::<f32, _, _>(
                cpal::StreamConfig {
                    channels: default.channels(),
                    sample_rate,
                    buffer_size,
                },
                move |data: &mut [f32], _| render.fill(data),
                move |error: cpal::Error| {
                    // Route changes are handled by cpal and xruns are
                    // dropouts, not failures; anything else needs a reopen.
                    if !matches!(error.kind(), ErrorKind::DeviceChanged | ErrorKind::Xrun) {
                        errors.reset.store(true, Ordering::Release);
                    }
                },
                Some(Duration::from_secs(2)),
            )
            .map_err(|e| map_error(&e, &name))?;
        stream.play().map_err(|e| map_error(&e, &name))?;
        let watchdog = Duration::from_secs_f64(
            (50.0 * f64::from(frames) / f64::from(sample_rate.max(1))).max(0.5),
        );
        Ok(Box::new(CpalStream {
            stream: Some(stream),
            slot: Arc::clone(slot),
            shared,
            watchdog,
            info: StreamInfo {
                sample_rate,
                buffer_frames: frames,
                input_latency: 0,
                output_latency: frames,
                input_channels: 0,
                output_channels: config.output_channels.len(),
            },
        }))
    }
}

struct Shared {
    reset: AtomicBool,
    /// Nanoseconds since `epoch` at the last callback, for the watchdog.
    last_callback_ns: AtomicU64,
    epoch: Instant,
}

/// State owned by cpal's audio thread.
struct Render {
    slot: Arc<Mutex<Option<Box<dyn AudioCallback>>>>,
    shared: Arc<Shared>,
    map: Vec<usize>,
    device_channels: usize,
    sample_rate: u32,
    outputs: Vec<f32>,
    /// Largest block the callback sees; bigger device buffers are split.
    capacity: usize,
    position: u64,
}

impl Render {
    fn fill(&mut self, data: &mut [f32]) {
        let Ok(mut guard) = self.slot.try_lock() else {
            data.fill(0.0);
            return;
        };
        let Some(callback) = guard.as_mut() else {
            data.fill(0.0);
            return;
        };
        let channels = self.device_channels;
        let total = data.len() / channels;
        let mut done = 0;
        while done < total {
            let frames = (total - done).min(self.capacity);
            let outputs = &mut self.outputs[..self.map.len() * frames];
            outputs.fill(0.0);
            let mut block = Block {
                sample_rate: self.sample_rate,
                frames,
                inputs: &[],
                outputs,
                input_channels: 0,
                output_channels: self.map.len(),
                position: Some(self.position),
            };
            callback.process(&mut block);
            interleave_f32(
                block.outputs,
                frames,
                &self.map,
                channels,
                &mut data[done * channels..(done + frames) * channels],
            );
            self.position += frames as u64;
            done += frames;
        }
        let now = self.shared.epoch.elapsed().as_nanos() as u64;
        self.shared.last_callback_ns.store(now, Ordering::Relaxed);
    }
}

struct CpalStream {
    stream: Option<cpal::Stream>,
    slot: Arc<Mutex<Option<Box<dyn AudioCallback>>>>,
    shared: Arc<Shared>,
    watchdog: Duration,
    info: StreamInfo,
}

impl Stream for CpalStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        if self.shared.reset.swap(false, Ordering::AcqRel) {
            return Some(StreamEvent::ResetRequested);
        }
        let last = Duration::from_nanos(self.shared.last_callback_ns.load(Ordering::Relaxed));
        (self.shared.epoch.elapsed().saturating_sub(last) > self.watchdog)
            .then_some(StreamEvent::Stalled)
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        // Dropping the stream stops it; the audio thread no longer runs.
        drop(self.stream.take());
        take(&self.slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI runners usually have no audio devices; this checks that
    /// enumeration is safe and consistent wherever it runs.
    #[test]
    fn enumeration_is_safe_and_consistent() {
        let mut backend = CpalBackend::new();
        for device in backend.devices() {
            assert_eq!(device.id.kind, kind());
            assert!(!device.output_channels.is_empty(), "{device:?}");
            if let Some(sizes) = device.buffer_sizes {
                assert!(sizes.allows(sizes.preferred), "{device:?}");
            }
        }
    }

    #[test]
    fn an_unknown_device_fails_cleanly_and_returns_the_callback() {
        let mut backend = CpalBackend::new();
        let config = StreamConfig {
            device: DeviceId {
                kind: kind(),
                id: "no-such-host:no-such-device".into(),
            },
            sample_rate: None,
            buffer_frames: None,
            input_channels: Vec::new(),
            output_channels: vec![0, 1],
        };
        let failed = backend
            .open(&config, Box::new(|_: &mut Block<'_>| {}))
            .err()
            .expect("an unknown device cannot open");
        assert!(matches!(
            failed.error,
            AudioError::DeviceNotFound(_) | AudioError::DeviceNotPresent(_)
        ));
    }
}
