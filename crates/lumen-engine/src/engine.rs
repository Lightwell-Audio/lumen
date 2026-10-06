//! The engine pair: [`EngineHandle`] on the UI side, [`Engine`] on the audio
//! thread, connected by lock-free queues (ADR-0003).
//!
//! The transport keeps its markers in ticks and its play position in
//! samples. When the tempo map or the device's sample rate changes, the
//! position is carried over through ticks, so the bar and beat stay put.
//!
//! Transport semantics (ADR-0017), as in Ableton Live:
//! - **Play** starts from the current position, which becomes the play marker.
//! - **Stop** while playing returns to the play marker; Stop while stopped
//!   returns to the start of the song.
//! - **Pause** stops where playback is.
//! - **Locate** moves the position and the play marker; while playing,
//!   playback continues from there.
//! - **Loop:** while the loop is on and playback reaches its end, it wraps to
//!   its start on the exact sample. Playback that starts after the loop end
//!   plays on.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_rt::DenormalGuard;

use crate::snapshot::{Shared, TransportSnapshot};
use crate::source::{Jump, Output, RenderContext, Source};
use crate::timeline::Timeline;

const COMMAND_CAPACITY: usize = 256;
const TIMELINE_CAPACITY: usize = 4;

/// A loop or punch range. It takes effect only when enabled and not empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    pub start: Ticks,
    pub end: Ticks,
    pub enabled: bool,
}

impl Region {
    pub fn new(start: Ticks, end: Ticks, enabled: bool) -> Self {
        Self {
            start,
            end,
            enabled,
        }
    }

    fn active(self) -> bool {
        self.enabled && self.end > self.start
    }
}

/// Live transport commands. Each takes effect at the start of the next
/// block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    Stop,
    Locate(Ticks),
    SetLoop(Region),
    SetPunch(Region),
}

/// UI side: sends commands and timelines, reads the transport, frees
/// retired timelines. Not real-time; may allocate.
#[derive(Debug)]
pub struct EngineHandle {
    commands: rtrb::Producer<Command>,
    timelines: rtrb::Producer<Box<Timeline>>,
    retired: rtrb::Consumer<Box<Timeline>>,
    shared: Arc<Shared>,
}

impl EngineHandle {
    /// Queues a command. Returns it if the queue is full.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands
            .push(command)
            .map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Queues a new tempo and time-signature map. Returns it if the queue
    /// is full.
    pub fn set_timeline(&mut self, timeline: Timeline) -> Result<(), Box<Timeline>> {
        self.timelines
            .push(Box::new(timeline))
            .map_err(|rtrb::PushError::Full(t)| t)
    }

    /// Frees timelines the audio side has replaced. Call regularly.
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
}

/// Audio side. Every method is real-time safe.
pub struct Engine<S> {
    commands: rtrb::Consumer<Command>,
    timelines: rtrb::Consumer<Box<Timeline>>,
    retired: rtrb::Producer<Box<Timeline>>,
    /// A replaced timeline that did not fit in the retired queue.
    stash: Option<Box<Timeline>>,
    timeline: Box<Timeline>,
    source: S,
    shared: Arc<Shared>,
    sample_rate: u32,
    playing: bool,
    /// Timeline sample of the next frame to play.
    position: i64,
    /// Where Play started (or the last locate): Stop returns here.
    play_marker: Ticks,
    loop_region: Region,
    punch: Region,
    /// Active loop as `[start, end)` samples at the current rate and tempo.
    loop_samples: Option<(i64, i64)>,
    punch_samples: Option<(i64, i64)>,
}

/// Creates the engine pair. Allocates, so call it off the audio thread.
pub fn engine<S: Source>(
    timeline: Timeline,
    source: S,
    sample_rate: u32,
) -> (EngineHandle, Engine<S>) {
    let (command_tx, command_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
    let (timeline_tx, timeline_rx) = rtrb::RingBuffer::new(TIMELINE_CAPACITY);
    // One more slot than timelines in flight, so retiring rarely waits.
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
        play_marker: Ticks::ZERO,
        loop_region: Region::default(),
        punch: Region::default(),
        loop_samples: None,
        punch_samples: None,
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
    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    pub fn transport(&self) -> TransportSnapshot {
        self.snapshot()
    }

    /// Renders one block into channel-major `data` (`channels × frames`
    /// samples, overwritten). `sample_rate` is the device's current rate;
    /// when it changes, the position is kept musically.
    pub fn process(&mut self, sample_rate: u32, data: &mut [f32], channels: usize, frames: usize) {
        let _denormals = DenormalGuard::new();
        data[..channels * frames].fill(0.0);
        self.retry_stash();
        self.swap_timeline();
        if sample_rate != self.sample_rate && sample_rate > 0 {
            let at = self.ticks_now();
            self.sample_rate = sample_rate;
            self.reposition(at);
        }
        self.apply_commands();
        if self.playing {
            self.render(data, channels, frames);
        }
        self.publish();
    }

    fn render(&mut self, data: &mut [f32], channels: usize, frames: usize) {
        let mut offset = 0;
        while offset < frames {
            let mut n = frames - offset;
            if let Some((_, end)) = self.loop_samples
                && self.position < end
            {
                n = n.min((end - self.position) as usize);
            }
            let cx = RenderContext {
                timeline: &self.timeline,
                sample_rate: self.sample_rate,
                start: self.position,
            };
            let mut out = Output::new(data, frames, offset, n, channels);
            self.source.render(&cx, &mut out);
            self.position += n as i64;
            offset += n;
            if let Some((start, end)) = self.loop_samples
                && self.position == end
            {
                self.position = start;
                self.locate_source(Jump::LoopWrap);
            }
        }
    }

    fn apply_commands(&mut self) {
        // Bounded, so a flood of commands cannot exhaust the block's budget.
        for _ in 0..COMMAND_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            match command {
                Command::Play => {
                    if !self.playing {
                        self.playing = true;
                        self.play_marker = self.ticks_now();
                        self.locate_source(Jump::Play);
                    }
                }
                Command::Pause => self.playing = false,
                Command::Stop => {
                    if self.playing {
                        self.playing = false;
                    } else {
                        self.play_marker = Ticks::ZERO;
                    }
                    self.position = self.sample_at(self.play_marker);
                }
                Command::Locate(at) => {
                    self.play_marker = at;
                    self.position = self.sample_at(at);
                    if self.playing {
                        self.locate_source(Jump::Locate);
                    }
                }
                Command::SetLoop(region) => {
                    self.loop_region = region;
                    self.refresh_regions();
                }
                Command::SetPunch(region) => {
                    self.punch = region;
                    self.refresh_regions();
                }
            }
        }
    }

    fn swap_timeline(&mut self) {
        // One swap per block, and only when the old timeline has a home.
        if self.stash.is_some() {
            return;
        }
        let Ok(incoming) = self.timelines.pop() else {
            return;
        };
        let at = self.ticks_now();
        let old = std::mem::replace(&mut self.timeline, incoming);
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
        self.reposition(at);
    }

    fn retry_stash(&mut self) {
        if let Some(old) = self.stash.take()
            && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
        {
            self.stash = Some(old);
        }
    }

    /// After a tempo or rate change: same musical position, new samples.
    fn reposition(&mut self, at: Ticks) {
        self.position = self.sample_at(at);
        self.refresh_regions();
        if self.playing {
            self.locate_source(Jump::Reposition);
        }
    }

    fn refresh_regions(&mut self) {
        self.loop_samples = self.region_samples(self.loop_region);
        self.punch_samples = self.region_samples(self.punch);
    }

    fn region_samples(&self, region: Region) -> Option<(i64, i64)> {
        region
            .active()
            .then(|| (self.sample_at(region.start), self.sample_at(region.end)))
            .filter(|(start, end)| end > start)
    }

    fn locate_source(&mut self, jump: Jump) {
        self.source
            .locate(&self.timeline, self.sample_rate, self.position, jump);
    }

    fn sample_at(&self, t: Ticks) -> i64 {
        self.timeline.sample_at(t, self.sample_rate)
    }

    fn ticks_now(&self) -> Ticks {
        self.timeline.ticks_at(self.position, self.sample_rate)
    }

    fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot {
            playing: self.playing,
            sample: self.position,
            ticks: self.ticks_now(),
            sample_rate: self.sample_rate,
            punching: self.playing
                && self
                    .punch_samples
                    .is_some_and(|(start, end)| (start..end).contains(&self.position)),
        }
    }

    fn publish(&self) {
        self.shared.publish(&self.snapshot());
    }
}
