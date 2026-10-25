//! WASAPI input (M9 slice R1, ADR-0100).
//!
//! WASAPI has no stream that carries a device's inputs and outputs together:
//! an audio interface shows up as several capture endpoints ("Analogue 1 + 2",
//! "Analogue 3 + 4", ...) next to its render endpoints. The inputs of a
//! device are the capture endpoints on the same adapter as its render
//! endpoint, side by side in name order. Each one is a *tap*: a shared-mode
//! capture stream converted by Windows to the render rate, read every render
//! block, and passed through a drift-corrected queue ([`crate::drift`])
//! because its clock is not the render clock.
//!
//! Capture is always shared mode, also beside an exclusive render stream: it
//! costs the audio engine's latency (about one more period) but never keeps
//! another program from the device's inputs, and it needs no format search.
//! Reading happens on the render thread; there is no thread of its own.

use wasapi::{
    AudioCaptureClient, AudioClient, Device, DeviceEnumerator, DeviceState, Direction, SampleType,
    StreamMode, WasapiError, WaveFormat, calculate_period_100ns,
};

use crate::drift::Corrected;
use crate::types::{AudioError, StreamConfig};
use crate::wasapi::{hns_to_frames, map_error};

/// A capture endpoint on the adapter of a render endpoint.
pub(crate) struct Endpoint {
    device: Device,
    name: String,
    id: String,
    channels: usize,
}

/// The capture endpoints on the same adapter as `render`, in name order.
/// Empty when the adapter has none (an HDMI monitor, say) or `render` does
/// not name its adapter.
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

/// How many input channels the adapter of `render` has.
pub(crate) fn channel_count(enumerator: &DeviceEnumerator, render: &Device) -> usize {
    endpoints(enumerator, render)
        .iter()
        .map(|e| e.channels)
        .sum()
}

/// One capture endpoint being read.
struct Tap {
    /// `None` when the endpoint would not open for an "all inputs" request
    /// (a microphone blocked in Windows privacy settings, say): its channels
    /// stay in place and are silent, so the others keep their numbers.
    live: Option<Live>,
    channels: usize,
    /// (channel of this tap, channel the callback sees).
    wanted: Vec<(usize, usize)>,
}

struct Live {
    client: AudioClient,
    capture: AudioCaptureClient,
    queue: Corrected,
    /// One device packet, as the bytes WASAPI returns.
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
        // Room for several render blocks and device packets between reads.
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

    /// Moves everything the device has captured into the queue.
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

/// The inputs of a stream.
pub(crate) struct Capture {
    taps: Vec<Tap>,
    /// One tap's block, channel after channel.
    pulled: Vec<f32>,
    /// Channels the callback sees.
    channels: usize,
    /// Frames from the sound reaching the device to the callback: the queue
    /// and one device packet, a little below the true figure (the converter
    /// and driver add their own).
    latency: u32,
}

impl Capture {
    /// Opens the inputs `config` asks for, beside the render stream on
    /// `render` that runs `period` frames at a time at `rate`. `Ok(None)`
    /// when none are asked for, or "all" were and there are none.
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
                Err(_) if all => None,
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
        }))
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

    /// Fills `inputs` (`channels × period`, channel after channel) with the
    /// next block of every wanted channel.
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
