use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverKind {
    Asio,
    WasapiShared,
    WasapiExclusive,
    CoreAudio,
    Alsa,
    Jack,
    PipeWire,
    Fake,
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub input_channels: Vec<String>,
    pub output_channels: Vec<String>,
    pub sample_rates: Vec<u32>,
    pub current_sample_rate: Option<u32>,
    pub buffer_sizes: Option<BufferSizes>,
}

pub const ALL_INPUTS: usize = usize::MAX;

pub const MOST_INPUTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfig {
    pub device: DeviceId,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub input_channels: Vec<usize>,
    pub output_channels: Vec<usize>,
}

impl StreamConfig {
    pub fn wants_all_inputs(&self) -> bool {
        self.input_channels == [ALL_INPUTS]
    }

    pub fn inputs_on(&self, available: usize) -> Vec<usize> {
        if self.wants_all_inputs() {
            (0..available.min(MOST_INPUTS)).collect()
        } else {
            self.input_channels.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub input_latency: u32,
    pub output_latency: u32,
    pub input_channels: usize,
    pub output_channels: usize,
    pub silent_inputs: Vec<String>,
}

impl StreamInfo {
    pub fn round_trip_ms(&self) -> f64 {
        f64::from(self.input_latency + self.output_latency) / f64::from(self.sample_rate) * 1e3
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    DeviceNotFound(String),
    DeviceNotPresent(String),
    DeviceBusy(String),
    Unsupported(String),
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

pub struct Block<'a> {
    pub sample_rate: u32,
    pub frames: usize,
    pub inputs: &'a [f32],
    pub outputs: &'a mut [f32],
    pub input_channels: usize,
    pub output_channels: usize,
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

pub trait AudioCallback: Send {
    fn process(&mut self, block: &mut Block<'_>);
}

impl<F: FnMut(&mut Block<'_>) + Send> AudioCallback for F {
    fn process(&mut self, block: &mut Block<'_>) {
        self(block);
    }
}
