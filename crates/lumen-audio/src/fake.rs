//! A scriptable backend for tests: unplug, replug, reset, stall or reject
//! settings on demand. Each `poll` runs one callback block, standing in for
//! the driver's thread.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::backend::{Backend, OpenError, Stream, StreamEvent};
use crate::types::{
    AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind, StreamConfig,
    StreamInfo,
};

#[derive(Debug)]
pub struct FakeDevice {
    pub present: bool,
    pub reset_pending: bool,
    pub stalled: bool,
    /// Opens succeed even while unplugged (the stream then fails at once),
    /// as some drivers do while their hardware is disappearing.
    pub phantom: bool,
    /// Deliver a block only every Nth poll (1 = always): a starving
    /// driver.
    pub deliver_every: u32,
    /// Driver sample positions jump ahead by this many frames once (a
    /// skipped buffer), then reset to 0.
    pub skip_frames: u64,
    /// The driver sample position a new stream starts at. Some drivers
    /// (the Focusrite's ASIO) keep counting through a reset, so a reopened
    /// stream starts far past where the last one stopped.
    pub start_position: u64,
    /// The next block reports no position and later ones jump this far
    /// ahead: a driver that lost sync and resynced (ASIO's resync request).
    pub resync_jump: Option<u64>,
    pub opens: u32,
    pub buffer_sizes: BufferSizes,
    /// Output channels the device has (4: a main and a cue pair).
    pub outputs: usize,
    /// Input channels the device has (2).
    pub inputs: usize,
    /// What each input channel hears, as a steady level (a DC signal), by
    /// the device's channel: a channel past the list hears silence. Read at
    /// every block, so a test changes it while the stream runs.
    pub input_levels: Vec<f32>,
    /// Keep a pace: each poll runs every block the time since the stream
    /// opened owes (up to [`CATCH_UP`]), so a poll that came late is made up
    /// for, as a hardware driver's clock would be. `None`, a poll runs one
    /// block however long it took, which a busy machine turns into a device
    /// that runs slow and falls ever further behind the wall clock.
    pub pace: Option<Pace>,
}

/// How a paced device counts the blocks it owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// Real time at the stream's sample rate, as a hardware driver runs.
    RealTime,
    /// A block per interval of wall time: a test device's slow motion (a
    /// small block every few milliseconds), kept steady however late the
    /// polls come.
    Every(Duration),
}

/// The most blocks a paced device runs in one poll.
pub const CATCH_UP: u64 = 16;

/// Shared handle the test uses to script the device.
#[derive(Debug, Clone)]
pub struct FakeControl(pub Arc<Mutex<FakeDevice>>);

impl FakeControl {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeDevice {
            present: true,
            reset_pending: false,
            stalled: false,
            phantom: false,
            deliver_every: 1,
            skip_frames: 0,
            start_position: 0,
            resync_jump: None,
            opens: 0,
            outputs: 4,
            inputs: 2,
            input_levels: Vec::new(),
            pace: None,
            buffer_sizes: BufferSizes {
                min: 32,
                max: 2048,
                preferred: 128,
                granularity: None,
            },
        })))
    }

    pub fn set(&self, f: impl FnOnce(&mut FakeDevice)) {
        f(&mut self.0.lock().expect("fake device lock"));
    }

    pub fn opens(&self) -> u32 {
        self.0.lock().expect("fake device lock").opens
    }
}

impl Default for FakeControl {
    fn default() -> Self {
        Self::new()
    }
}

pub fn fake_device_id() -> DeviceId {
    DeviceId {
        kind: DriverKind::Fake,
        id: "fake".into(),
    }
}

pub struct FakeBackend(pub FakeControl);

impl Backend for FakeBackend {
    fn devices(&mut self) -> Vec<DeviceInfo> {
        let device = self.0.0.lock().expect("fake device lock");
        vec![DeviceInfo {
            id: fake_device_id(),
            name: "Fake device".into(),
            input_channels: (1..=device.inputs).map(|n| format!("In {n}")).collect(),
            output_channels: (1..=device.outputs).map(|n| format!("Out {n}")).collect(),
            sample_rates: vec![44_100, 48_000],
            current_sample_rate: Some(48_000),
            buffer_sizes: Some(device.buffer_sizes),
        }]
    }

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        let mut device = self.0.0.lock().expect("fake device lock");
        let frames = config
            .buffer_frames
            .unwrap_or(device.buffer_sizes.preferred);
        let error = if config.device != fake_device_id() {
            Some(AudioError::DeviceNotFound(config.device.id.clone()))
        } else if !device.present && !device.phantom {
            Some(AudioError::DeviceNotPresent("fake device unplugged".into()))
        } else if !device.buffer_sizes.allows(frames) {
            Some(AudioError::Unsupported(format!("buffer size {frames}")))
        } else {
            None
        };
        if let Some(error) = error {
            return Err(OpenError { error, callback });
        }
        device.opens += 1;
        device.reset_pending = false;
        let input_map = config.inputs_on(device.inputs);
        let (inputs, outputs) = (input_map.len(), config.output_channels.len());
        Ok(Box::new(FakeStream {
            control: self.0.clone(),
            callback: Some(callback),
            info: StreamInfo {
                sample_rate: config.sample_rate.unwrap_or(48_000),
                buffer_frames: frames,
                input_latency: frames + 32,
                output_latency: frames + 64,
                input_channels: inputs,
                output_channels: outputs,
            },
            inputs: vec![0.0; inputs * frames as usize],
            input_map,
            outputs: vec![0.0; outputs * frames as usize],
            position: device.start_position,
            polls: 0,
            opened: Instant::now(),
            ran: 0,
        }))
    }
}

struct FakeStream {
    control: FakeControl,
    callback: Option<Box<dyn AudioCallback>>,
    info: StreamInfo,
    inputs: Vec<f32>,
    /// The device channel each input of the callback is.
    input_map: Vec<usize>,
    outputs: Vec<f32>,
    position: u64,
    polls: u32,
    /// When the stream opened, and how many blocks it has run since: what
    /// a paced device measures real time by.
    opened: Instant,
    ran: u64,
}

impl FakeStream {
    /// The blocks that `pace` owes since the stream opened, not past
    /// [`CATCH_UP`]; what is owed beyond it waits for the next poll.
    fn owed(&mut self, pace: Pace) -> u64 {
        let every = match pace {
            Pace::RealTime => {
                u128::from(self.info.buffer_frames.max(1)) * 1_000_000_000
                    / u128::from(self.info.sample_rate.max(1))
            }
            Pace::Every(interval) => interval.as_nanos(),
        };
        let due = (self.opened.elapsed().as_nanos() / every.max(1)) as u64;
        let owed = due.saturating_sub(self.ran).min(CATCH_UP);
        self.ran += owed;
        owed
    }
}

impl Stream for FakeStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        let (present, reset, stalled, every, skip, resync, pace) = {
            let device = self.control.0.lock().expect("fake device lock");
            let frames = self.info.buffer_frames as usize;
            for (i, &channel) in self.input_map.iter().enumerate() {
                let level = device.input_levels.get(channel).copied().unwrap_or(0.0);
                self.inputs[i * frames..(i + 1) * frames].fill(level);
            }
            drop(device);
            let mut device = self.control.0.lock().expect("fake device lock");
            let reset = std::mem::take(&mut device.reset_pending);
            let skip = std::mem::take(&mut device.skip_frames);
            (
                device.present,
                reset,
                device.stalled,
                device.deliver_every.max(1),
                skip,
                device.resync_jump.take(),
                device.pace,
            )
        };
        self.position += skip + resync.unwrap_or(0);
        self.polls += 1;
        if !self.polls.is_multiple_of(every) {
            return None;
        }
        if stalled {
            return Some(StreamEvent::Stalled);
        }
        // Real drivers ask for a reset when their hardware disappears.
        if reset || !present {
            return Some(StreamEvent::ResetRequested);
        }
        let blocks = pace.map_or(1, |pace| self.owed(pace));
        if let Some(callback) = self.callback.as_mut() {
            for n in 0..blocks {
                self.outputs.fill(0.0);
                let mut block = Block {
                    sample_rate: self.info.sample_rate,
                    frames: self.info.buffer_frames as usize,
                    inputs: &self.inputs,
                    outputs: &mut self.outputs,
                    input_channels: self.info.input_channels,
                    output_channels: self.info.output_channels,
                    position: (resync.is_none() || n > 0).then_some(self.position),
                };
                callback.process(&mut block);
                self.position += u64::from(self.info.buffer_frames);
            }
        }
        None
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        self.callback.take().expect("callback present until close")
    }
}
