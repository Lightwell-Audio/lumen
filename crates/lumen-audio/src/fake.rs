//! A scriptable backend for tests: unplug, replug, reset, stall or reject
//! settings on demand, and refuse, delay or hold on to a stream as a driver
//! that does not answer would. Each `poll` runs one callback block, standing in for
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
    /// Every input channel hears a ramp instead of `input_levels`: the
    /// stream's frame count (counted from its start position) times a
    /// millionth, wrapping at a million, so a recording shows to the sample
    /// where each frame came from ([`ramp_at`]).
    pub input_ramp: bool,
    /// Keep a pace: each poll runs every block the time since the stream
    /// opened owes (up to [`CATCH_UP`]), so a poll that came late is made up
    /// for, as a hardware driver's clock would be. `None`, a poll runs one
    /// block however long it took, which a busy machine turns into a device
    /// that runs slow and falls ever further behind the wall clock.
    pub pace: Option<Pace>,
    /// A cable from an output to an input (M9 slice R2b): what the output
    /// played comes back on the input `delay` frames later, times `gain`,
    /// in place of `input_levels`. Delays shorter than a block read as
    /// silence, as no real device is that quick.
    pub cable: Option<Cable>,
    /// Every open is refused, as a driver that will not start.
    pub refuse_open: bool,
    /// Each open takes this long before it answers, as a driver that is
    /// slow to start (or, for long enough, one that does not answer).
    pub open_delay: Duration,
    /// Closing a stream takes this long, so a driver switch's handover
    /// does not come in time: a driver that holds on to the audio.
    pub close_delay: Duration,
}

/// A loopback cable on the fake device, by device channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cable {
    pub output: usize,
    pub input: usize,
    pub delay: u32,
    pub gain: f32,
}

/// Frames of output a fake cable remembers.
const CABLE_FRAMES: usize = 1 << 18;

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

/// What the ramp input holds at stream frame `frame`.
pub fn ramp_at(frame: u64) -> f32 {
    (frame % 1_000_000) as f32 * 1e-6
}

/// The most blocks a paced device runs in one poll.
pub const CATCH_UP: u64 = 16;

/// Shared handle the test uses to script the device.
#[derive(Debug, Clone)]
pub struct FakeControl(pub Arc<Mutex<FakeDevice>>);

impl FakeDevice {
    /// The device as it first is: present, silent, four outputs and two
    /// inputs, with nothing scripted.
    fn initial() -> Self {
        FakeDevice {
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
            input_ramp: false,
            pace: None,
            cable: None,
            refuse_open: false,
            open_delay: Duration::ZERO,
            close_delay: Duration::ZERO,
            buffer_sizes: BufferSizes {
                min: 32,
                max: 2048,
                preferred: 128,
                granularity: None,
            },
        }
    }
}

impl FakeControl {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeDevice::initial())))
    }

    /// Puts the device back as [`FakeControl::new`] made it, whatever a
    /// scenario scripted: for a device shared by scenarios that take turns.
    pub fn reset(&self) {
        *self.0.lock().expect("fake device lock") = FakeDevice::initial();
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
        // Waits as a slow driver would, with the device free meanwhile.
        let delay = self.0.0.lock().expect("fake device lock").open_delay;
        std::thread::sleep(delay);
        let mut device = self.0.0.lock().expect("fake device lock");
        let frames = config
            .buffer_frames
            .unwrap_or(device.buffer_sizes.preferred);
        let error = if device.refuse_open {
            Some(AudioError::Driver(
                "the test device refused to start".into(),
            ))
        } else if config.device != fake_device_id() {
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
            output_map: config.output_channels.clone(),
            line: Vec::new(),
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
    /// The device channel each output of the callback is.
    output_map: Vec<usize>,
    /// What the cable's output played, by stream frame modulo its length;
    /// made when a cable is first plugged in.
    line: Vec<f32>,
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

impl FakeStream {
    /// The cable's input hears what its output played `delay` frames ago.
    fn cable_in(&mut self, cable: Cable) {
        let Some(i) = self.input_map.iter().position(|&c| c == cable.input) else {
            return;
        };
        let frames = self.info.buffer_frames as usize;
        let delay = u64::from(cable.delay);
        for (n, sample) in self.inputs[i * frames..(i + 1) * frames]
            .iter_mut()
            .enumerate()
        {
            let at = self.position + n as u64;
            *sample = if delay < frames as u64 || at < delay || delay >= CABLE_FRAMES as u64 {
                0.0
            } else {
                self.line[((at - delay) % CABLE_FRAMES as u64) as usize] * cable.gain
            };
        }
    }

    /// Remembers what the cable's output played in this block.
    fn cable_out(&mut self, cable: Cable) {
        let frames = self.info.buffer_frames as usize;
        let played = self.output_map.iter().position(|&c| c == cable.output);
        for n in 0..frames {
            let at = ((self.position + n as u64) % CABLE_FRAMES as u64) as usize;
            self.line[at] = played.map_or(0.0, |o| self.outputs[o * frames + n]);
        }
    }
}

impl Stream for FakeStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        let (present, reset, stalled, every, skip, resync, pace, ramp, cable) = {
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
                device.input_ramp,
                device.cable,
            )
        };
        if cable.is_some() && self.line.is_empty() {
            self.line = vec![0.0; CABLE_FRAMES];
        }
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
        // Held out while it runs, so the cable can use the stream meanwhile.
        if let Some(mut callback) = self.callback.take() {
            for n in 0..blocks {
                if ramp {
                    let frames = self.info.buffer_frames as usize;
                    for channel in self.inputs.chunks_mut(frames) {
                        for (i, sample) in channel.iter_mut().enumerate() {
                            *sample = ramp_at(self.position + i as u64);
                        }
                    }
                }
                if let Some(cable) = cable {
                    self.cable_in(cable);
                }
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
                if let Some(cable) = cable {
                    self.cable_out(cable);
                }
                self.position += u64::from(self.info.buffer_frames);
            }
            self.callback = Some(callback);
        }
        None
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        let delay = self.control.0.lock().expect("fake device lock").close_delay;
        std::thread::sleep(delay);
        self.callback.take().expect("callback present until close")
    }
}
