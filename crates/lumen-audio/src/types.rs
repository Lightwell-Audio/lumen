//! Device, configuration and callback types shared by every backend.

use std::fmt;

/// Which driver model a device belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverKind {
    Asio,
    WasapiShared,
    WasapiExclusive,
    CoreAudio,
    Alsa,
    Jack,
    PipeWire,
    /// Test backend.
    Fake,
}

/// Stable identity of a device within its backend (an ASIO CLSID, a
/// WASAPI endpoint id, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId {
    pub kind: DriverKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferSizes {
    pub min: u32,
    pub max: u32,
    pub preferred: u32,
    /// Step between allowed sizes; 0 means only `preferred`, `None` means powers of two.
    pub granularity: Option<u32>,
}

impl BufferSizes {
    pub fn allows(&self, frames: u32) -> bool {
        if frames < self.min || frames > self.max {
            return false;
        }
        match self.granularity {
            None => frames.is_power_of_two(),
            Some(0) => frames == self.preferred,
            Some(step) => (frames - self.min).is_multiple_of(step),
        }
    }
}

/// What a device offers, as shown in the device selector.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    /// User-facing name.
    pub name: String,
    pub input_channels: Vec<String>,
    pub output_channels: Vec<String>,
    pub sample_rates: Vec<u32>,
    pub current_sample_rate: Option<u32>,
    pub buffer_sizes: Option<BufferSizes>,
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    pub device: DeviceId,
    /// `None` keeps the device's current rate (ASIO devices often share it
    /// with other applications).
    pub sample_rate: Option<u32>,
    /// `None` uses the device's preferred size.
    pub buffer_frames: Option<u32>,
    /// Device channel indices, in the order the callback sees them.
    pub input_channels: Vec<usize>,
    pub output_channels: Vec<usize>,
}

/// What the device actually granted.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    /// Driver-reported latencies, in frames (include driver safety buffers).
    pub input_latency: u32,
    pub output_latency: u32,
    pub input_channels: usize,
    pub output_channels: usize,
}

impl StreamInfo {
    pub fn round_trip_ms(&self) -> f64 {
        f64::from(self.input_latency + self.output_latency) / f64::from(self.sample_rate) * 1e3
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// No such device is registered.
    DeviceNotFound(String),
    /// Registered, but the hardware is not there (unplugged, powered off).
    DeviceNotPresent(String),
    /// Another application holds it exclusively.
    DeviceBusy(String),
    /// The device cannot do what was asked (rate, buffer size, channels).
    Unsupported(String),
    /// Anything else the driver reported.
    Driver(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceNotFound(m) => write!(f, "Audio device not found: {m}"),
            AudioError::DeviceNotPresent(m) => write!(f, "Audio device is not connected: {m}"),
            AudioError::DeviceBusy(m) => {
                write!(f, "Audio device is in use by another application: {m}")
            }
            AudioError::Unsupported(m) => {
                write!(f, "Audio device does not support this setting: {m}")
            }
            AudioError::Driver(m) => write!(f, "Audio driver error: {m}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// One block of audio for the callback: channel-major `f32` buffers.
pub struct Block<'a> {
    pub sample_rate: u32,
    pub frames: usize,
    /// `input_channels × frames` samples, channel after channel.
    pub inputs: &'a [f32],
    /// `output_channels × frames` samples, channel after channel; zeroed
    /// before the callback runs.
    pub outputs: &'a mut [f32],
    pub input_channels: usize,
    pub output_channels: usize,
    /// The driver's running sample position, when it reports one.
    pub position: Option<u64>,
}

impl Block<'_> {
    pub fn input(&self, channel: usize) -> &[f32] {
        &self.inputs[channel * self.frames..(channel + 1) * self.frames]
    }

    pub fn output(&mut self, channel: usize) -> &mut [f32] {
        let frames = self.frames;
        &mut self.outputs[channel * frames..(channel + 1) * frames]
    }
}

/// The real-time audio callback. Runs on the driver's thread: it must not
/// allocate, lock, block or panic (ADR-0010).
pub trait AudioCallback: Send {
    fn process(&mut self, block: &mut Block<'_>);
}

impl<F: FnMut(&mut Block<'_>) + Send> AudioCallback for F {
    fn process(&mut self, block: &mut Block<'_>) {
        self(block);
    }
}
