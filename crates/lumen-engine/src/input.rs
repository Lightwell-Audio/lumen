//! Audio input in the engine (M9 slice R1, ADR-0100).
//!
//! The device callback hands the engine each block's inputs
//! ([`Source::input`](crate::Source::input)). Two things use them:
//!
//! - **Meters** of every input channel the stream opened (peak since the UI
//!   last looked, RMS, and a clip light), kept in a lock-free bank like the
//!   mixer's. They run whenever the stream has inputs, whatever the transport
//!   does, so a level can be set before anything is armed.
//! - **Monitoring**: a track with an input route plays its input into its own
//!   bus, so the track's devices, fader and sends hear it as they hear a clip.
//!   A route says when it does (see [`Monitoring`]); the level glides
//!   over 5 ms when that changes.
//!
//! The block is copied into a buffer made at start-up, so rendering needs
//! nothing from the callback but the frame the block began on. A block larger
//! than the buffer is metered but not monitored, and counted.
//!
//! The UI replaces the routes whole through a queue, as clip lists are, and
//! gets the old list back to free. Levels glide on from list to list by track.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::capture::{CaptureReader, CaptureSink, capture};
use crate::mixer::{Buses, TrackSource};
use crate::source::{Inputs, Mode, RenderContext};

/// Device input channels metered and monitored; the rest are ignored.
pub const MAX_INPUTS: usize = 32;
/// Routes played at once; a longer list is cut.
pub const MAX_ROUTES: usize = 64;
/// Frames of input held for monitoring: more than any driver's buffer.
const MAX_BLOCK: usize = 8_192;
/// Route lists in flight.
const QUEUE: usize = 4;
/// A monitor level that changes glides over 1/200 s.
const GLIDE_PER_SECOND: u32 = 200;
/// How long the RMS reading averages over, as the mixer's meters.
pub const RMS_SECONDS: f32 = 0.3;
/// A sample at or above this has used the converter's whole range.
pub const CLIP_LEVEL: f32 = 0.9999;

/// When a route's input is heard on its track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Monitoring {
    /// Never (the track records it, or the track's monitor is off).
    Never,
    /// While the transport is stopped, counting in or recording, and not
    /// while playing back without recording (the monitor mode Auto, with the
    /// track armed).
    Armed,
    /// Always (the monitor mode In).
    Always,
}

/// An audio track's input and when it is heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputRoute {
    /// The track's node in the routing.
    pub track: u64,
    /// The first input channel, counted in the stream's open inputs.
    pub channel: usize,
    /// The channel after it feeds the right side; otherwise the one channel
    /// plays on both sides.
    pub stereo: bool,
    pub monitoring: Monitoring,
    /// The track records this input while the transport records: the engine
    /// keeps what arrives (see [`crate::capture`]).
    pub record: bool,
}

/// What an input channel read since it was last taken.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InputLevel {
    /// The highest absolute sample (linear).
    pub peak: f32,
    /// RMS over [`RMS_SECONDS`].
    pub rms: f32,
    /// A sample reached [`CLIP_LEVEL`].
    pub clipped: bool,
}

#[derive(Debug, Default)]
struct Slot {
    /// Highest absolute sample since the last read, as `f32` bits.
    peak: AtomicU32,
    /// Smoothed mean square, as `f32` bits.
    power: AtomicU32,
    clipped: AtomicBool,
}

/// Readings of every input channel, written by the audio thread.
#[derive(Debug)]
struct Meters {
    slots: [Slot; MAX_INPUTS],
    /// Blocks too large to monitor.
    oversized: AtomicU64,
}

/// UI side: replaces the routes and reads the meters. May allocate.
#[derive(Debug)]
pub struct InputHandle {
    incoming: rtrb::Producer<Box<[InputRoute]>>,
    retired: rtrb::Consumer<Box<[InputRoute]>>,
    meters: Arc<Meters>,
    capture: Option<CaptureReader>,
}

impl InputHandle {
    /// Sends a new list of routes (the first [`MAX_ROUTES`]). Returns false
    /// when the queue is full: send it again next frame.
    pub fn set_routes(&mut self, routes: &[InputRoute]) -> bool {
        let routes = &routes[..routes.len().min(MAX_ROUTES)];
        self.incoming.push(routes.into()).is_ok()
    }

    /// What input channel `channel` read since it was last taken: the peak and
    /// the clip since then, the RMS now. Channels the stream did not open read
    /// silence.
    pub fn take_level(&self, channel: usize) -> InputLevel {
        let Some(slot) = self.meters.slots.get(channel) else {
            return InputLevel::default();
        };
        InputLevel {
            peak: f32::from_bits(slot.peak.swap(0, Ordering::Relaxed)),
            rms: f32::from_bits(slot.power.load(Ordering::Relaxed)).sqrt(),
            clipped: slot.clipped.swap(false, Ordering::Relaxed),
        }
    }

    /// The reading end of what recording tracks keep, to hand to the worker
    /// that writes it. Once: later calls return `None`.
    pub fn take_capture(&mut self) -> Option<CaptureReader> {
        self.capture.take()
    }

    /// Blocks so far that were larger than the monitoring buffer.
    pub fn oversized_blocks(&self) -> u64 {
        self.meters.oversized.load(Ordering::Relaxed)
    }

    /// Frees route lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

/// A route being played, with its level.
#[derive(Debug, Clone, Copy)]
struct Playing {
    route: InputRoute,
    level: f32,
}

/// Audio side: the input meters, and monitoring into the tracks' buses.
pub struct InputPlayer {
    incoming: rtrb::Consumer<Box<[InputRoute]>>,
    retired: rtrb::Producer<Box<[InputRoute]>>,
    /// A replaced list that did not fit in the retired queue yet.
    stash: Option<Box<[InputRoute]>>,
    /// The routes in play and the list they are rebuilt into; both made at
    /// full size, so a new list never allocates.
    playing: Vec<Playing>,
    next: Vec<Playing>,
    meters: Arc<Meters>,
    /// The last block's inputs, channel after channel at `held_frames` apart.
    held: Box<[f32]>,
    held_channels: usize,
    held_frames: usize,
    /// The device frame the held block began on.
    held_at: u64,
    capture: CaptureSink,
}

/// Makes the pair. Allocates: call it off the audio thread.
pub fn input_player() -> (InputHandle, InputPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (reader, sink) = capture();
    let meters = Arc::new(Meters {
        slots: std::array::from_fn(|_| Slot::default()),
        oversized: AtomicU64::new(0),
    });
    (
        InputHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            meters: Arc::clone(&meters),
            capture: Some(reader),
        },
        InputPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            playing: Vec::with_capacity(MAX_ROUTES),
            next: Vec::with_capacity(MAX_ROUTES),
            meters,
            held: vec![0.0; MAX_INPUTS * MAX_BLOCK].into_boxed_slice(),
            held_channels: 0,
            held_frames: 0,
            held_at: 0,
            capture: sink,
        },
    )
}

impl InputPlayer {
    /// Takes the newest route list, carrying each track's level over.
    fn take_routes(&mut self) {
        if let Some(old) = self.stash.take()
            && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
        {
            self.stash = Some(old);
            return;
        }
        while self.stash.is_none()
            && let Ok(routes) = self.incoming.pop()
        {
            self.next.clear();
            for &route in routes.iter().take(MAX_ROUTES) {
                let level = self
                    .playing
                    .iter()
                    .find(|p| p.route.track == route.track)
                    .map_or(0.0, |p| p.level);
                self.next.push(Playing { route, level });
            }
            std::mem::swap(&mut self.playing, &mut self.next);
            // Handed back to be freed off this thread; if the queue is full
            // it waits here and the next list waits in the queue.
            if let Err(rtrb::PushError::Full(routes)) = self.retired.push(routes) {
                self.stash = Some(routes);
            }
        }
    }

    /// Keeps the part's frames of every route that records, stamped with the
    /// timeline sample heard at the part's first frame: the render position
    /// leads what is heard by the read ahead.
    fn keep(&mut self, cx: &RenderContext<'_>, from: usize, frames: usize) {
        let ahead = cx.stream.saturating_sub(cx.device) as i64;
        let at = cx.start - ahead;
        let held = self.held_frames;
        for playing in &self.playing {
            let route = &playing.route;
            if !route.record || !self.has(route) {
                continue;
            }
            let side = |channel: usize| &self.held[channel * held + from..][..frames];
            let right = route.stereo.then(|| side(route.channel + 1));
            self.capture
                .push(route.track, cx.device, at, side(route.channel), right);
        }
    }

    /// Whether the held block has the channels `route` reads.
    fn has(&self, route: &InputRoute) -> bool {
        route.channel + usize::from(route.stereo) < self.held_channels
    }
}

impl TrackSource for InputPlayer {
    fn input(&mut self, inputs: &Inputs<'_>) {
        let channels = inputs.channels.min(MAX_INPUTS);
        let frames = inputs.frames;
        self.held_channels = 0;
        self.held_frames = 0;
        self.take_routes();
        if frames == 0 || inputs.data.len() < channels * frames {
            return;
        }
        let fits = frames <= MAX_BLOCK;
        if !fits {
            self.meters.oversized.fetch_add(1, Ordering::Relaxed);
        }
        // How much of the mean square carries over a block.
        let keep = (-(frames as f32) / (RMS_SECONDS * inputs.sample_rate.max(1) as f32)).exp();
        for channel in 0..channels {
            let samples = &inputs.data[channel * frames..(channel + 1) * frames];
            let slot = &self.meters.slots[channel];
            let (peak, sum) = peak_and_sum(samples);
            if peak > 0.0 {
                slot.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
            if peak >= CLIP_LEVEL {
                slot.clipped.store(true, Ordering::Relaxed);
            }
            // A non-finite sample must not poison the average for good.
            let mean = sum / frames as f32;
            let mean = if mean.is_finite() { mean } else { 0.0 };
            let was = f32::from_bits(slot.power.load(Ordering::Relaxed));
            let now = was * keep + mean * (1.0 - keep);
            // Below about −150 dB, silence: no denormals left to decay.
            let now = if now < 1e-15 { 0.0 } else { now };
            slot.power.store(now.to_bits(), Ordering::Relaxed);
            if fits {
                self.held[channel * frames..(channel + 1) * frames].copy_from_slice(samples);
            }
        }
        if fits {
            self.held_channels = channels;
            self.held_frames = frames;
            self.held_at = inputs.device;
        }
    }

    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        if cx.tail || cx.mode == Mode::Scrub || self.held_frames == 0 {
            return;
        }
        let frames = buses.frames();
        let held = self.held_frames;
        // Where this part lies in the held block.
        let Some(from) = cx.device.checked_sub(self.held_at).map(|d| d as usize) else {
            return;
        };
        if from + frames > held {
            return;
        }
        if cx.recording && cx.mode == Mode::Play {
            self.keep(cx, from, frames);
        }
        let step = 1.0 / (cx.sample_rate / GLIDE_PER_SECOND).max(1) as f32;
        for index in 0..self.playing.len() {
            let Playing { route, level } = self.playing[index];
            let wanted = match route.monitoring {
                Monitoring::Never => false,
                Monitoring::Always => true,
                Monitoring::Armed => cx.mode == Mode::Ring || cx.recording,
            } && self.has(&route);
            if level == 0.0 && !wanted {
                continue;
            }
            let Some(mut bus) = buses.track(route.track) else {
                continue;
            };
            let target = f32::from(wanted);
            let side = |channel: usize| &self.held[channel * held + from..][..frames];
            let left = side(route.channel.min(self.held_channels.saturating_sub(1)));
            let right = if route.stereo && self.has(&route) {
                side(route.channel + 1)
            } else {
                left
            };
            let mut after = level;
            for (side, samples) in [left, right].into_iter().enumerate() {
                after = level;
                for (out, &sample) in bus.channel(side).iter_mut().zip(samples) {
                    after = glide(after, target, step);
                    *out += sample * after;
                }
            }
            self.playing[index].level = after;
        }
    }

    fn ringing(&self) -> bool {
        self.held_frames != 0
            && self.playing.iter().any(|p| {
                (p.route.monitoring != Monitoring::Never && self.has(&p.route)) || p.level > 0.0
            })
    }
}

/// `level` moved `step` toward `target`.
#[inline]
fn glide(level: f32, target: f32, step: f32) -> f32 {
    if level < target {
        (level + step).min(target)
    } else {
        (level - step).max(target)
    }
}

/// The highest absolute sample and the sum of squares.
#[inline]
fn peak_and_sum(samples: &[f32]) -> (f32, f32) {
    let (mut peak, mut sum) = (0.0f32, 0.0f32);
    for &x in samples {
        peak = peak.max(x.abs());
        sum += x * x;
    }
    (peak, sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::CHUNK;

    const RATE: u32 = 48_000;
    const TRACK: u64 = 7;

    fn route(channel: usize, stereo: bool, monitoring: Monitoring) -> InputRoute {
        InputRoute {
            track: TRACK,
            channel,
            stereo,
            monitoring,
            record: false,
        }
    }

    /// A player that was handed `routes` and one block of `channels` ramps
    /// (channel `c` holds `c + 1` times 0.01) of `frames`, at device frame 0.
    fn fed(routes: &[InputRoute], channels: usize, frames: usize) -> (InputHandle, InputPlayer) {
        let (mut handle, mut player) = input_player();
        assert!(handle.set_routes(routes));
        let data: Vec<f32> = (0..channels)
            .flat_map(|c| std::iter::repeat_n((c + 1) as f32 * 0.01, frames))
            .collect();
        player.input(&Inputs {
            device: 0,
            sample_rate: RATE,
            channels,
            frames,
            data: &data,
        });
        (handle, player)
    }

    /// Renders `frames` from device frame `device` and returns the track's
    /// two sides.
    fn render(
        player: &mut InputPlayer,
        mode: Mode,
        recording: bool,
        device: u64,
        frames: usize,
    ) -> [Vec<f32>; 2] {
        let timeline = crate::Timeline::default();
        let cx = RenderContext {
            timeline: &timeline,
            sample_rate: RATE,
            start: 0,
            tail: false,
            stopping: false,
            mode,
            stream: device,
            device,
            recording,
            looping: None,
            deadline: None,
        };
        let by_id = [(TRACK, 0u32)];
        let mut data = vec![0.0; 2 * CHUNK];
        player.render(&cx, &mut Buses::over(&by_id, &mut data, frames));
        [
            data[..frames].to_vec(),
            data[CHUNK..CHUNK + frames].to_vec(),
        ]
    }

    /// Long enough for a monitor level to finish gliding in.
    fn settled(player: &mut InputPlayer, mode: Mode, recording: bool) -> [Vec<f32>; 2] {
        for _ in 0..2 {
            let _ = render(player, mode, recording, 0, 128);
        }
        render(player, mode, recording, 0, 128)
    }

    #[test]
    fn a_mono_input_plays_on_both_sides_and_a_pair_on_its_own_side() {
        let (_, mut mono) = fed(&[route(1, false, Monitoring::Always)], 4, 128);
        let [l, r] = settled(&mut mono, Mode::Play, false);
        assert!(l.iter().chain(&r).all(|&x| (x - 0.02).abs() < 1e-6));
        let (_, mut pair) = fed(&[route(2, true, Monitoring::Always)], 4, 128);
        let [l, r] = settled(&mut pair, Mode::Play, false);
        assert!(l.iter().all(|&x| (x - 0.03).abs() < 1e-6));
        assert!(r.iter().all(|&x| (x - 0.04).abs() < 1e-6));
    }

    #[test]
    fn auto_monitoring_is_heard_stopped_and_recording_but_not_in_plain_playback() {
        let armed = [route(0, false, Monitoring::Armed)];
        let heard = |mode, recording| {
            let (_, mut player) = fed(&armed, 2, 128);
            settled(&mut player, mode, recording)[0][127] != 0.0
        };
        assert!(heard(Mode::Ring, false), "stopped");
        assert!(heard(Mode::Play, true), "recording");
        assert!(!heard(Mode::Play, false), "playing back");
        let (_, mut never) = fed(&[route(0, false, Monitoring::Never)], 2, 128);
        assert_eq!(settled(&mut never, Mode::Ring, true)[0][127], 0.0);
    }

    #[test]
    fn nothing_is_heard_of_a_scrub_a_tail_or_a_channel_the_stream_lacks() {
        let (_, mut player) = fed(&[route(0, false, Monitoring::Always)], 2, 128);
        assert_eq!(settled(&mut player, Mode::Scrub, false)[0][127], 0.0);
        let (_, mut absent) = fed(&[route(5, false, Monitoring::Always)], 2, 128);
        assert_eq!(settled(&mut absent, Mode::Play, false)[0][127], 0.0);
        assert!(!absent.ringing(), "a route to nothing keeps nothing awake");
        let (_, mut stereo_edge) = fed(&[route(1, true, Monitoring::Always)], 2, 128);
        assert_eq!(settled(&mut stereo_edge, Mode::Play, false)[0][127], 0.0);
    }

    #[test]
    fn a_part_reads_its_own_place_in_the_block() {
        let (mut handle, mut player) = input_player();
        handle.set_routes(&[route(0, false, Monitoring::Always)]);
        let data: Vec<f32> = (0..256).map(|i| i as f32 / 1000.0).collect();
        player.input(&Inputs {
            device: 1_000,
            sample_rate: RATE,
            channels: 1,
            frames: 256,
            data: &data,
        });
        // Let the level settle, then read the second half of the block.
        let _ = render(&mut player, Mode::Play, false, 1_000, 256.min(CHUNK));
        let [l, _] = render(&mut player, Mode::Play, false, 1_128, 64);
        assert!((l[0] - 0.128).abs() < 1e-6 && (l[63] - 0.191).abs() < 1e-6);
        // A part outside the held block, or before it, adds nothing.
        assert!(
            render(&mut player, Mode::Play, false, 1_256, 8)[0]
                .iter()
                .all(|&x| x == 0.0)
        );
        assert!(
            render(&mut player, Mode::Play, false, 999, 8)[0]
                .iter()
                .all(|&x| x == 0.0)
        );
    }

    /// A route that records.
    fn recording(channel: usize, stereo: bool, monitoring: Monitoring) -> InputRoute {
        InputRoute {
            record: true,
            ..route(channel, stereo, monitoring)
        }
    }

    /// Renders one part of `frames` at device frame `device`, where the
    /// timeline is at `start` and the stream leads the device by `lead`.
    fn part(
        player: &mut InputPlayer,
        cx: (Mode, bool, bool),
        start: i64,
        lead: u64,
        device: u64,
        frames: usize,
    ) {
        let (mode, recording, tail) = cx;
        let timeline = crate::Timeline::default();
        let cx = RenderContext {
            timeline: &timeline,
            sample_rate: RATE,
            start,
            tail,
            stopping: false,
            mode,
            stream: device + lead,
            device,
            recording,
            looping: None,
            deadline: None,
        };
        let by_id = [(TRACK, 0u32)];
        let mut data = vec![0.0; 2 * CHUNK];
        player.render(&cx, &mut Buses::over(&by_id, &mut data, frames));
    }

    fn kept(handle: &mut InputHandle) -> (Vec<crate::capture::Chunk>, Vec<Vec<f32>>) {
        let mut reader = handle.take_capture().unwrap();
        let (mut chunks, mut samples) = (Vec::new(), Vec::new());
        let mut buf = Vec::new();
        while let Some(chunk) = reader.next(&mut buf) {
            chunks.push(chunk);
            samples.push(buf.clone());
        }
        (chunks, samples)
    }

    #[test]
    fn recording_keeps_a_routes_frames_with_the_sample_heard_at_the_first() {
        // Monitoring Never: the take is kept all the same.
        let (mut handle, mut player) = fed(&[recording(1, false, Monitoring::Never)], 2, 128);
        // The render position is 1000 and leads what is heard by 300.
        part(&mut player, (Mode::Play, true, false), 1_000, 300, 0, 128);
        let (chunks, samples) = kept(&mut handle);
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0],
            crate::capture::Chunk {
                track: TRACK,
                device: 0,
                at: 700,
                frames: 128,
                channels: 1
            }
        );
        assert!(samples[0].iter().all(|&x| (x - 0.02).abs() < 1e-6));
    }

    #[test]
    fn a_pair_is_kept_as_two_channels_and_a_part_reads_its_place_in_the_block() {
        let (mut handle, mut player) = input_player();
        handle.set_routes(&[recording(0, true, Monitoring::Armed)]);
        let left: Vec<f32> = (0..256).map(|i| i as f32).collect();
        let right: Vec<f32> = (0..256).map(|i| -(i as f32)).collect();
        let data = [left.clone(), right.clone()].concat();
        player.input(&Inputs {
            device: 5_000,
            sample_rate: RATE,
            channels: 2,
            frames: 256,
            data: &data,
        });
        part(&mut player, (Mode::Play, true, false), 40, 0, 5_100, 64);
        let (chunks, samples) = kept(&mut handle);
        assert_eq!(
            (chunks[0].device, chunks[0].at, chunks[0].channels),
            (5_100, 40, 2)
        );
        assert_eq!(&samples[0][..64], &left[100..164]);
        assert_eq!(&samples[0][64..], &right[100..164]);
    }

    #[test]
    fn nothing_is_kept_unless_the_transport_records_a_playing_part_for_a_route_that_records() {
        let armed = [recording(0, false, Monitoring::Armed)];
        for (name, cx) in [
            ("not recording", (Mode::Play, false, false)),
            ("stopped", (Mode::Ring, true, false)),
            ("scrubbing", (Mode::Scrub, true, false)),
            ("a tail", (Mode::Play, true, true)),
        ] {
            let (mut handle, mut player) = fed(&armed, 1, 128);
            part(&mut player, cx, 0, 0, 0, 128);
            assert!(kept(&mut handle).0.is_empty(), "{name}");
        }
        let (mut handle, mut player) = fed(&[route(0, false, Monitoring::Armed)], 1, 128);
        part(&mut player, (Mode::Play, true, false), 0, 0, 0, 128);
        assert!(
            kept(&mut handle).0.is_empty(),
            "a route that does not record"
        );
        let (mut handle, mut player) = fed(&[recording(5, false, Monitoring::Armed)], 1, 128);
        part(&mut player, (Mode::Play, true, false), 0, 0, 0, 128);
        assert!(kept(&mut handle).0.is_empty(), "a channel the stream lacks");
    }

    #[test]
    fn consecutive_parts_follow_each_other_and_two_routes_keep_two_chunks() {
        let routes = [
            recording(0, false, Monitoring::Never),
            InputRoute {
                track: 8,
                ..recording(1, false, Monitoring::Never)
            },
        ];
        let (mut handle, mut player) = fed(&routes, 2, 128);
        part(&mut player, (Mode::Play, true, false), 10, 0, 0, 100);
        part(&mut player, (Mode::Play, true, false), 110, 0, 100, 28);
        let (chunks, _) = kept(&mut handle);
        let seen: Vec<_> = chunks
            .iter()
            .map(|c| (c.track, c.device, c.at, c.frames))
            .collect();
        assert_eq!(
            seen,
            [
                (TRACK, 0, 10, 100),
                (8, 0, 10, 100),
                (TRACK, 100, 110, 28),
                (8, 100, 110, 28)
            ]
        );
    }

    #[test]
    fn monitoring_glides_instead_of_stepping() {
        let (_, mut player) = fed(&[route(0, false, Monitoring::Always)], 1, 128);
        let [l, _] = render(&mut player, Mode::Play, false, 0, 128);
        assert!(l[0] < 0.01 * 0.1, "starts from silence: {}", l[0]);
        assert!(l.windows(2).all(|w| w[1] >= w[0]), "rises");
        assert!(l[127] < 0.01, "and has not arrived yet");
        let [l, _] = render(&mut player, Mode::Play, false, 0, 128);
        assert!((l[127] - 0.01).abs() < 1e-6, "it arrives within 5 ms");
    }

    #[test]
    fn a_new_list_keeps_a_tracks_level_and_returns_the_old_list() {
        let (mut handle, mut player) = fed(&[route(0, false, Monitoring::Always)], 1, 128);
        let _ = settled(&mut player, Mode::Play, false);
        assert!(handle.set_routes(&[route(0, false, Monitoring::Always)]));
        player.input(&Inputs {
            device: 0,
            sample_rate: RATE,
            channels: 1,
            frames: 128,
            data: &[0.01; 128],
        });
        let [l, _] = render(&mut player, Mode::Play, false, 0, 128);
        assert!((l[0] - 0.01).abs() < 1e-6, "no fade-in again: {}", l[0]);
        assert_eq!(handle.collect_garbage(), 2, "both lists come back");
    }

    #[test]
    fn meters_read_peak_rms_and_clip_and_clear_on_taking() {
        let (handle, _player) = fed(&[], 2, 128);
        let level = handle.take_level(1);
        assert!((level.peak - 0.02).abs() < 1e-6 && level.rms > 0.0 && !level.clipped);
        assert_eq!(handle.take_level(1).peak, 0.0, "the peak is taken");
        assert_eq!(handle.take_level(9), InputLevel::default(), "not opened");
        let (handle, mut player) = input_player();
        let mut data = vec![0.5; 64];
        data[10] = -1.0;
        player.input(&Inputs {
            device: 0,
            sample_rate: RATE,
            channels: 1,
            frames: 64,
            data: &data,
        });
        let level = handle.take_level(0);
        assert!(level.clipped && level.peak == 1.0);
        assert!(!handle.take_level(0).clipped, "the light is taken");
    }

    #[test]
    fn a_block_too_large_to_hold_is_metered_and_counted_but_not_monitored() {
        let frames = MAX_BLOCK + 1;
        let (handle, mut player) = fed(&[route(0, false, Monitoring::Always)], 1, frames);
        assert_eq!(handle.oversized_blocks(), 1);
        assert!(handle.take_level(0).peak > 0.0);
        assert_eq!(settled(&mut player, Mode::Play, false)[0][127], 0.0);
    }
}
