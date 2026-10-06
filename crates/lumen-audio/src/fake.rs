//! A scriptable backend for tests: unplug, replug, reset, stall or reject
//! settings on demand. Each `poll` runs one callback block, standing in for
//! the driver's thread.

use std::sync::{Arc, Mutex};

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
    pub opens: u32,
    pub buffer_sizes: BufferSizes,
}

/// Shared handle the test uses to script the device.
#[derive(Debug, Clone)]
pub struct FakeControl(pub Arc<Mutex<FakeDevice>>);

impl FakeControl {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeDevice {
            present: true,
            reset_pending: false,
            stalled: false,
            opens: 0,
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
            input_channels: vec!["In 1".into(), "In 2".into()],
            output_channels: vec!["Out 1".into(), "Out 2".into()],
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
        } else if !device.present {
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
        let (inputs, outputs) = (config.input_channels.len(), config.output_channels.len());
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
            outputs: vec![0.0; outputs * frames as usize],
            position: 0,
        }))
    }
}

struct FakeStream {
    control: FakeControl,
    callback: Option<Box<dyn AudioCallback>>,
    info: StreamInfo,
    inputs: Vec<f32>,
    outputs: Vec<f32>,
    position: u64,
}

impl Stream for FakeStream {
    fn info(&self) -> StreamInfo {
        self.info.clone()
    }

    fn poll(&mut self) -> Option<StreamEvent> {
        let (present, reset, stalled) = {
            let mut device = self.control.0.lock().expect("fake device lock");
            let reset = std::mem::take(&mut device.reset_pending);
            (device.present, reset, device.stalled)
        };
        if stalled {
            return Some(StreamEvent::Stalled);
        }
        // Real drivers ask for a reset when their hardware disappears.
        if reset || !present {
            return Some(StreamEvent::ResetRequested);
        }
        if let Some(callback) = self.callback.as_mut() {
            self.outputs.fill(0.0);
            let mut block = Block {
                sample_rate: self.info.sample_rate,
                frames: self.info.buffer_frames as usize,
                inputs: &self.inputs,
                outputs: &mut self.outputs,
                input_channels: self.info.input_channels,
                output_channels: self.info.output_channels,
                position: Some(self.position),
            };
            callback.process(&mut block);
            self.position += u64::from(self.info.buffer_frames);
        }
        None
    }

    fn close(mut self: Box<Self>) -> Box<dyn AudioCallback> {
        self.callback.take().expect("callback present until close")
    }
}
