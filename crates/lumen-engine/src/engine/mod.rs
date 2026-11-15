mod command;
mod maps;
mod transport;

pub use command::{Command, Region};

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_rt::DenormalGuard;

use crate::declick::Declick;
use crate::metronome::{CountIn, Metronome, TestTone};
use crate::playhead::Playhead;
use crate::scrub::Scrubber;
use crate::snapshot::{Shared, TransportSnapshot};
use crate::source::{Inputs, Jump, Mode, Output, RenderContext, Source};
use crate::timeline::Timeline;

pub(in crate::engine) const COMMAND_CAPACITY: usize = 256;
const TIMELINE_CAPACITY: usize = 4;

#[derive(Debug)]
pub struct EngineHandle {
    commands: rtrb::Producer<Command>,
    timelines: rtrb::Producer<Box<Timeline>>,
    retired: rtrb::Consumer<Box<Timeline>>,
    shared: Arc<Shared>,
}

impl EngineHandle {
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands
            .push(command)
            .map_err(|rtrb::PushError::Full(c)| c)
    }

    pub fn set_timeline(&mut self, timeline: Timeline) -> Result<(), Box<Timeline>> {
        self.timelines
            .push(Box::new(timeline))
            .map_err(|rtrb::PushError::Full(t)| t)
    }

    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }

    pub fn transport(&self) -> TransportSnapshot {
        self.shared.read()
    }

    pub fn transport_reader(&self) -> TransportReader {
        TransportReader(Arc::clone(&self.shared))
    }
}

#[derive(Clone)]
pub struct TransportReader(Arc<Shared>);

impl TransportReader {
    pub fn read(&self) -> TransportSnapshot {
        self.0.read()
    }
}

pub struct Engine<S> {
    pub(in crate::engine) commands: rtrb::Consumer<Command>,
    pub(in crate::engine) timelines: rtrb::Consumer<Box<Timeline>>,
    pub(in crate::engine) retired: rtrb::Producer<Box<Timeline>>,
    pub(in crate::engine) stash: Option<Box<Timeline>>,
    pub(in crate::engine) timeline: Box<Timeline>,
    pub(in crate::engine) source: S,
    pub(in crate::engine) shared: Arc<Shared>,
    pub(in crate::engine) sample_rate: u32,
    pub(in crate::engine) playing: bool,
    pub(in crate::engine) position: i64,
    pub(in crate::engine) ahead: i64,
    pub(in crate::engine) lookahead: i64,
    pub(in crate::engine) playhead: Playhead,
    pub(in crate::engine) play_marker: Ticks,
    pub(in crate::engine) loop_region: Region,
    pub(in crate::engine) punch: Region,
    pub(in crate::engine) loop_samples: Option<(i64, i64)>,
    pub(in crate::engine) punch_samples: Option<(i64, i64)>,
    pub(in crate::engine) declick: Declick,
    pub(in crate::engine) metronome: Metronome,
    pub(in crate::engine) test_tone: TestTone,
    pub(in crate::engine) scrub: Scrubber,
    pub(in crate::engine) stream: u64,
    pub(in crate::engine) recording: bool,
    pub(in crate::engine) counting: Option<CountIn>,
    pub(in crate::engine) deadline: Option<Instant>,
    pub(in crate::engine) offline: Option<Duration>,
}

pub const OFFLINE_WAIT: Duration = Duration::from_secs(10);

pub const HOST_SHARE: (u64, u64) = (3, 4);

pub fn engine<S: Source>(
    timeline: Timeline,
    source: S,
    sample_rate: u32,
) -> (EngineHandle, Engine<S>) {
    let (command_tx, command_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
    let (timeline_tx, timeline_rx) = rtrb::RingBuffer::new(TIMELINE_CAPACITY);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(TIMELINE_CAPACITY + 1);
    let shared = Arc::new(Shared::default());
    let engine = Engine {
        commands: command_rx,
        timelines: timeline_rx,
        retired: retired_tx,
        stash: None,
        timeline: Box::new(timeline),
        source,
        shared: Arc::clone(&shared),
        sample_rate: sample_rate.max(1),
        playing: false,
        position: 0,
        ahead: 0,
        lookahead: 0,
        playhead: Playhead::default(),
        play_marker: Ticks::ZERO,
        loop_region: Region::default(),
        punch: Region::default(),
        loop_samples: None,
        punch_samples: None,
        declick: Declick::default(),
        metronome: Metronome::default(),
        test_tone: TestTone::default(),
        scrub: Scrubber::default(),
        stream: 0,
        recording: false,
        counting: None,
        deadline: None,
        offline: None,
    };
    engine.publish();
    let handle = EngineHandle {
        commands: command_tx,
        timelines: timeline_tx,
        retired: retired_rx,
        shared,
    };
    (handle, engine)
}

impl<S: Source> Engine<S> {
    pub fn set_offline(&mut self, wait: Option<Duration>) {
        self.offline = wait;
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    pub fn transport(&self) -> TransportSnapshot {
        self.snapshot()
    }

    pub fn process(&mut self, sample_rate: u32, data: &mut [f32], channels: usize, frames: usize) {
        self.process_with_input(sample_rate, &[], 0, data, channels, frames);
    }

    pub fn process_with_input(
        &mut self,
        sample_rate: u32,
        inputs: &[f32],
        input_channels: usize,
        data: &mut [f32],
        channels: usize,
        frames: usize,
    ) {
        let _denormals = DenormalGuard::new();
        self.deadline = self.source.hosted().then(|| {
            if let Some(wait) = self.offline {
                return Instant::now() + wait;
            }
            let period = frames as u64 * 1_000_000_000 / u64::from(sample_rate.max(1));
            Instant::now() + Duration::from_nanos(period * HOST_SHARE.0 / HOST_SHARE.1)
        });
        data[..channels * frames].fill(0.0);
        if input_channels > 0 {
            self.source.input(&Inputs {
                device: self.stream,
                sample_rate,
                channels: input_channels,
                frames,
                data: inputs,
            });
        }
        self.retry_stash();
        self.swap_timeline(channels);
        if sample_rate != self.sample_rate && sample_rate > 0 {
            let at = self.ticks_now();
            let ahead = self.timeline.ticks_at(self.ahead, self.sample_rate);
            let old_rate = self.sample_rate;
            let timeline = &self.timeline;
            let convert = |s| timeline.sample_at(timeline.ticks_at(s, old_rate), sample_rate);
            self.playhead
                .reposition(timeline.sample_at(at, sample_rate), convert);
            self.sample_rate = sample_rate;
            self.reposition(at, ahead);
        }
        self.follow_latency(channels);
        self.apply_commands(channels);
        let rate = self.sample_rate;
        let counted = self.count_in(data, channels, frames);
        if self.playing {
            self.render(data, channels, frames, counted);
        } else if counted == 0 {
            if self.scrub.active() {
                self.scrub.render(
                    &mut self.source,
                    &self.timeline,
                    rate,
                    data,
                    channels,
                    frames,
                );
            } else if self.source.ringing() {
                let cx = RenderContext {
                    timeline: &self.timeline,
                    sample_rate: rate,
                    start: self.position,
                    tail: false,
                    stopping: false,
                    mode: Mode::Ring,
                    stream: self.stream,
                    device: self.stream,
                    recording: false,
                    looping: None,
                    deadline: self.deadline,
                };
                let mut out = Output::new(data, frames, 0, frames, channels);
                self.source.render(&cx, &mut out);
            } else {
                self.source.idle(rate, frames);
            }
            if self.declick.active() {
                self.declick.apply(data, frames, channels, 0, frames);
            }
            self.metronome
                .ring_out(data, frames, channels, frames, rate);
        }
        self.test_tone.render(data, frames, channels, frames, rate);
        self.stream += frames as u64;
        self.publish();
    }

    fn snapshot(&self) -> TransportSnapshot {
        let shown = if self.playing && self.lookahead != 0 {
            self.playhead.shown()
        } else {
            self.position
        };
        TransportSnapshot {
            playing: self.playing,
            sample: shown,
            ticks: self.timeline.ticks_at(shown, self.sample_rate),
            marker: self.play_marker,
            sample_rate: self.sample_rate,
            punching: self.playing
                && self
                    .punch_samples
                    .is_some_and(|(start, end)| (start..end).contains(&self.position)),
            recording: self.recording,
            count_in_left: self.counting.map_or(0, |c| c.left()),
            count_in_total: self.counting.map_or(0, |c| c.total()),
        }
    }

    fn publish(&self) {
        self.shared.publish(&self.snapshot());
    }

    pub(in crate::engine) fn locate_source(&mut self, jump: Jump) {
        self.source
            .locate(&self.timeline, self.sample_rate, self.ahead, jump);
    }

    pub(in crate::engine) fn sample_at(&self, t: Ticks) -> i64 {
        self.timeline.sample_at(t, self.sample_rate)
    }

    pub(in crate::engine) fn ticks_now(&self) -> Ticks {
        self.timeline.ticks_at(self.position, self.sample_rate)
    }
}
