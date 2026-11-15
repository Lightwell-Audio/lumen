use wasapi::{
    AudioCaptureClient, AudioClient, Device, DeviceEnumerator, DeviceState, Direction, SampleType,
    StreamMode, WasapiError, WaveFormat, calculate_period_100ns,
};

use crate::drift::Corrected;
use crate::types::{AudioError, StreamConfig};
use crate::wasapi::{hns_to_frames, map_error};

pub(crate) struct Endpoint {
    device: Device,
    name: String,
    id: String,
    channels: usize,
}

pub(crate) fn endpoints(enumerator: &DeviceEnumerator, render: &Device) -> Vec<Endpoint> {
    let Some(adapter) = render
        .get_interface_friendlyname()
        .ok()
        .filter(|a| !a.is_empty())
    else {
        return Vec::new();
    };
    let Ok(collection) = enumerator.get_device_collection(&Direction::Capture) else {
        return Vec::new();
    };
    let mut found: Vec<Endpoint> = (&collection)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|d| matches!(d.get_state(), Ok(DeviceState::Active)))
        .filter(|d| d.get_interface_friendlyname().ok().as_deref() == Some(adapter.as_str()))
        .filter_map(|device| {
            let channels = device
                .get_iaudioclient()
                .ok()?
                .get_mixformat()
                .ok()
                .map(|f| usize::from(f.get_nchannels()))?;
            Some(Endpoint {
                name: device.get_friendlyname().ok()?,
                id: device.get_id().ok()?,
                device,
                channels,
            })
        })
        .collect();
    found.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    found
}

pub(crate) fn channel_count(enumerator: &DeviceEnumerator, render: &Device) -> usize {
    endpoints(enumerator, render)
        .iter()
        .map(|e| e.channels)
        .sum()
}

struct Tap {
    live: Option<Live>,
    channels: usize,
    wanted: Vec<(usize, usize)>,
}

struct Live {
    client: AudioClient,
    capture: AudioCaptureClient,
    queue: Corrected,
    bytes: Vec<u8>,
    samples: Vec<f32>,
    block_align: usize,
}

impl Live {
    fn open(
        endpoint: &Endpoint,
        rate: u32,
        period: usize,
        packet: usize,
    ) -> Result<Self, AudioError> {
        let fail = |e: WasapiError| map_error(&e, &endpoint.name);
        let mut client = endpoint.device.get_iaudioclient().map_err(fail)?;
        let (default_period, _) = client.get_device_period().map_err(fail)?;
        let wave = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            rate as usize,
            endpoint.channels,
            None,
        );
        let render_period = calculate_period_100ns(period as i64, i64::from(rate));
        let buffer = (4 * default_period).max(4 * render_period);
        let mode = StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: buffer,
        };
        client
            .initialize_client(&wave, &Direction::Capture, &mode)
            .map_err(fail)?;
        let capture = client.get_audiocaptureclient().map_err(fail)?;
        let frames = client.get_buffer_size().map_err(fail)? as usize;
        let block_align = wave.get_blockalign() as usize;
        let queue = Corrected::new(endpoint.channels, period, period + packet)
            .ok_or_else(|| AudioError::Unsupported(format!("buffer size {period} for input")))?;
        Ok(Self {
            client,
            capture,
            queue,
            bytes: vec![0; frames * block_align],
            samples: vec![0.0; frames * endpoint.channels],
            block_align,
        })
    }

    fn drain(&mut self, channels: usize) -> Result<(), WasapiError> {
        while let Some(frames) = self.capture.get_next_packet_size()? {
            let frames = (frames as usize).min(self.bytes.len() / self.block_align);
            if frames == 0 {
                break;
            }
            let bytes = &mut self.bytes[..frames * self.block_align];
            let (read, info) = self.capture.read_from_device(bytes)?;
            let read = read as usize;
            let samples = &mut self.samples[..read * channels];
            if info.flags.silent {
                samples.fill(0.0);
            } else {
                let (raw, _) = bytes.as_chunks::<4>();
                for (sample, raw) in samples.iter_mut().zip(raw) {
                    *sample = f32::from_le_bytes(*raw);
                }
            }
            self.queue.push(samples);
        }
        Ok(())
    }
}

pub(crate) struct Capture {
    taps: Vec<Tap>,
    pulled: Vec<f32>,
    channels: usize,
    latency: u32,
    silent: Vec<String>,
}

impl Capture {
    pub(crate) fn open(
        config: &StreamConfig,
        enumerator: &DeviceEnumerator,
        render: &Device,
        rate: u32,
        period: usize,
    ) -> Result<Option<Self>, AudioError> {
        if config.input_channels.is_empty() {
            return Ok(None);
        }
        let found = endpoints(enumerator, render);
        let available: usize = found.iter().map(|e| e.channels).sum();
        let all = config.wants_all_inputs();
        let wanted = config.inputs_on(available);
        if let Some(&bad) = wanted.iter().find(|&&c| c >= available) {
            return Err(AudioError::Unsupported(format!(
                "input {} (the device has {available})",
                bad + 1
            )));
        }
        if wanted.is_empty() {
            return Ok(None);
        }
        let mut taps = Vec::new();
        let mut silent = Vec::new();
        let mut first = 0;
        let mut packet_frames = 0;
        for endpoint in &found {
            let mine: Vec<(usize, usize)> = wanted
                .iter()
                .enumerate()
                .filter(|&(_, &c)| (first..first + endpoint.channels).contains(&c))
                .map(|(seen, &c)| (c - first, seen))
                .collect();
            first += endpoint.channels;
            if mine.is_empty() {
                continue;
            }
            let packet = endpoint
                .device
                .get_iaudioclient()
                .and_then(|c| c.get_device_period())
                .map(|(default, _)| hns_to_frames(default, rate) as usize)
                .unwrap_or(period);
            packet_frames = packet_frames.max(packet);
            let live = match Live::open(endpoint, rate, period, packet) {
                Ok(live) => Some(live),
                Err(_) if all => {
                    silent.push(endpoint.name.clone());
                    None
                }
                Err(error) => return Err(error),
            };
            taps.push(Tap {
                live,
                channels: endpoint.channels,
                wanted: mine,
            });
        }
        let widest = taps.iter().map(|t| t.channels).max().unwrap_or(0);
        let queued = taps
            .iter()
            .filter_map(|t| t.live.as_ref())
            .map(|l| l.queue.target())
            .max()
            .unwrap_or(period);
        Ok(Some(Self {
            taps,
            pulled: vec![0.0; widest * period],
            channels: wanted.len(),
            latency: (queued + packet_frames) as u32,
            silent,
        }))
    }

    pub(crate) fn silent(&self) -> &[String] {
        &self.silent
    }

    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    pub(crate) fn latency(&self) -> u32 {
        self.latency
    }

    pub(crate) fn start(&mut self) -> Result<(), WasapiError> {
        for live in self.taps.iter().filter_map(|t| t.live.as_ref()) {
            live.client.start_stream()?;
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        for live in self.taps.iter().filter_map(|t| t.live.as_ref()) {
            let _ = live.client.stop_stream();
        }
    }

    pub(crate) fn fill(&mut self, inputs: &mut [f32], period: usize) -> Result<(), WasapiError> {
        for tap in &mut self.taps {
            let block = &mut self.pulled[..tap.channels * period];
            match tap.live.as_mut() {
                Some(live) => {
                    live.drain(tap.channels)?;
                    live.queue.pull(block);
                }
                None => block.fill(0.0),
            }
            for &(from, to) in &tap.wanted {
                inputs[to * period..(to + 1) * period]
                    .copy_from_slice(&block[from * period..(from + 1) * period]);
            }
        }
        Ok(())
    }
}
