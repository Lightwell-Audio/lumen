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
    pub phantom: bool,
    pub deliver_every: u32,
    pub skip_frames: u64,
    pub start_position: u64,
    pub resync_jump: Option<u64>,
    pub opens: u32,
    pub buffer_sizes: BufferSizes,
    pub outputs: usize,
    pub inputs: usize,
    pub input_levels: Vec<f32>,
    pub input_ramp: bool,
    pub pace: Option<Pace>,
    pub allowance: Option<u64>,
    pub cable: Option<Cable>,
    pub refuse_open: bool,
    pub silent_inputs: Vec<String>,
    pub open_delay: Duration,
    pub close_delay: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cable {
    pub output: usize,
    pub input: usize,
    pub delay: u32,
    pub gain: f32,
}

const CABLE_FRAMES: usize = 1 << 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    RealTime,
    Every(Duration),
}

pub fn ramp_at(frame: u64) -> f32 {
    (frame % 1_000_000) as f32 * 1e-6
}

pub const CATCH_UP: u64 = 16;

#[derive(Debug, Clone)]
pub struct FakeControl(pub Arc<Mutex<FakeDevice>>);

impl FakeDevice {
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
            allowance: None,
            cable: None,
            refuse_open: false,
            silent_inputs: Vec::new(),
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
                silent_inputs: if inputs > 0 {
                    device.silent_inputs.clone()
                } else {
                    Vec::new()
                },
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
            realtime: false,
        }))
    }
}

struct FakeStream {
    control: FakeControl,
    callback: Option<Box<dyn AudioCallback>>,
    info: StreamInfo,
    inputs: Vec<f32>,
    input_map: Vec<usize>,
    output_map: Vec<usize>,
    line: Vec<f32>,
    outputs: Vec<f32>,
    position: u64,
    polls: u32,
    opened: Instant,
    ran: u64,
    realtime: bool,
}

impl FakeStream {
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
        if reset || !present {
            return Some(StreamEvent::ResetRequested);
        }
        if pace == Some(Pace::RealTime) && !self.realtime {
            self.realtime = true;
            let block =
                f64::from(self.info.buffer_frames) / f64::from(self.info.sample_rate.max(1));
            lumen_rt::realtime_thread(Duration::from_secs_f64(block));
        }
        let mut blocks = pace.map_or(1, |pace| self.owed(pace));
        if let Some(left) = &mut self.control.0.lock().expect("fake device lock").allowance {
            blocks = blocks.min(*left);
            *left -= blocks;
        }
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
