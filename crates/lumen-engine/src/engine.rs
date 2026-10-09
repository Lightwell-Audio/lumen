//! The engine pair: [`EngineHandle`] on the UI side, [`Engine`] on the audio
//! thread, connected by lock-free queues (ADR-0003).
//!
//! The transport keeps its markers in ticks and its play position in
//! samples. When the tempo map or the device's sample rate changes, the
//! position is carried over through ticks, so the bar and beat stay put.
//!
//! Transport semantics (ADR-0017), as in Ableton Live. There are two
//! positions: the **play marker** (where playback starts) and the
//! **playhead** (where playback is).
//! - **Play** starts from the play marker.
//! - **Continue** starts from the playhead (Ableton's Shift+Space).
//! - **Stop** and **Pause** stop where playback is: the playhead stays there.
//!   Stop while already stopped returns both to the start of the song.
//! - **Locate** moves both; while playing, playback continues from there.
//! - **Loop:** while the loop is on and playback reaches its end, it wraps to
//!   its start on the exact sample. Playback that starts after the loop end
//!   plays on.
//!
//! - **Scrub** (while stopped): plays around a position in a
//!   [`ScrubStyle`] and moves both markers there; **EndScrub** fades it out.
//!   While playing, the app locates instead.
//!
//! Every jump is click-free (`declick`): a 2 ms crossfade on locates and
//! loop wraps, a fade-in on play and a fade-out on stop and pause.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_rt::DenormalGuard;

use crate::declick::{Declick, MAX_FADE, fade_frames};
use crate::metronome::{self, Metronome, MetronomeSettings, TestTone};
use crate::scrub::{ScrubStyle, Scrubber};
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Play from the play marker.
    Play,
    /// Play from the playhead, where playback last stopped.
    Continue,
    /// Stop where playback is.
    Pause,
    /// Stop where playback is; when already stopped, return to the start.
    Stop,
    Locate(Ticks),
    SetLoop(Region),
    SetPunch(Region),
    SetMetronome(MetronomeSettings),
    /// 1 kHz at −20 dBFS on every output, playing or not.
    TestTone(bool),
    /// While stopped: play around `at` in `style`, and move the play
    /// marker and the playhead there. Ignored while playing.
    Scrub {
        at: Ticks,
        style: ScrubStyle,
    },
    /// Fade the scrub out.
    EndScrub,
    /// Stop at once and silence everything: playback, the metronome, the
    /// test tone, scrubbing and any fade in progress. (MIDI all-notes-off joins it in
    /// M5.)
    Panic,
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
    declick: Declick,
    metronome: Metronome,
    test_tone: TestTone,
    scrub: Scrubber,
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
        declick: Declick::default(),
        metronome: Metronome::default(),
        test_tone: TestTone::default(),
        scrub: Scrubber::default(),
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
        self.swap_timeline(channels);
        if sample_rate != self.sample_rate && sample_rate > 0 {
            let at = self.ticks_now();
            self.sample_rate = sample_rate;
            self.reposition(at);
        }
        self.apply_commands(channels);
        let rate = self.sample_rate;
        if self.playing {
            self.render(data, channels, frames);
        } else {
            if self.scrub.active() {
                self.scrub.render(
                    &mut self.source,
                    &self.timeline,
                    rate,
                    data,
                    channels,
                    frames,
                );
            } else {
                // Meters fall to rest while nothing plays.
                self.source.idle(rate, frames);
            }
            if self.declick.active() {
                // A stop or pause just happened: its tail fades out over silence.
                self.declick.apply(data, frames, channels, 0, frames);
            }
            self.metronome
                .ring_out(data, frames, channels, frames, rate);
        }
        self.test_tone.render(data, frames, channels, frames, rate);
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
                tail: false,
            };
            let mut out = Output::new(data, frames, offset, n, channels);
            self.source.render(&cx, &mut out);
            self.declick.apply(data, frames, channels, offset, n);
            // After the crossfade, so a click on a loop start keeps its attack.
            self.metronome.render(
                &self.timeline,
                self.sample_rate,
                self.position,
                data,
                metronome::Window {
                    stride: frames,
                    channels,
                    offset,
                    count: n,
                },
            );
            self.position += n as i64;
            offset += n;
            if let Some((start, end)) = self.loop_samples
                && self.position == end
            {
                // Crossfade what would follow the loop end into the loop start.
                self.capture_tail(channels);
                self.position = start;
                self.locate_source(Jump::LoopWrap);
                self.declick.fade_in(fade_frames(self.sample_rate));
            }
        }
    }

    /// Renders what would follow the current position into the declick
    /// tail, faded out, ahead of a jump. Only while playing.
    fn capture_tail(&mut self, channels: usize) {
        if !self.playing {
            return;
        }
        let frames = fade_frames(self.sample_rate);
        let Some(buffer) = self.declick.tail_buffer(channels) else {
            return;
        };
        let cx = RenderContext {
            timeline: &self.timeline,
            sample_rate: self.sample_rate,
            start: self.position,
            tail: true,
        };
        let mut out = Output::new(buffer, MAX_FADE, 0, frames, channels);
        self.source.render(&cx, &mut out);
        self.declick.tail_rendered(channels, frames);
    }

    fn apply_commands(&mut self, channels: usize) {
        // Bounded, so a flood of commands cannot exhaust the block's budget.
        for _ in 0..COMMAND_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            match command {
                Command::Play | Command::Continue => {
                    if !self.playing {
                        self.scrub.end();
                        if command == Command::Play {
                            self.position = self.sample_at(self.play_marker);
                        }
                        self.playing = true;
                        self.locate_source(Jump::Play);
                        self.declick.fade_in(fade_frames(self.sample_rate));
                    }
                }
                Command::Pause => {
                    self.capture_tail(channels);
                    self.playing = false;
                }
                Command::Stop => {
                    if self.playing {
                        self.capture_tail(channels);
                        self.playing = false;
                    } else {
                        self.play_marker = Ticks::ZERO;
                        self.position = 0;
                    }
                }
                Command::Locate(at) => {
                    self.capture_tail(channels);
                    self.play_marker = at;
                    self.position = self.sample_at(at);
                    if self.playing {
                        self.locate_source(Jump::Locate);
                        self.declick.fade_in(fade_frames(self.sample_rate));
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
                Command::Scrub { at, style } => {
                    if !self.playing {
                        let at = at.max(Ticks::ZERO);
                        self.play_marker = at;
                        self.position = self.sample_at(at);
                        let end = match style {
                            ScrubStyle::Loop { length } => self.sample_at(at + length),
                            ScrubStyle::Tape => self.position,
                        };
                        self.scrub
                            .scrub(self.position, end, style, self.sample_rate);
                    }
                }
                Command::EndScrub => self.scrub.end(),
                Command::SetMetronome(settings) => self.metronome.settings = settings,
                Command::TestTone(on) => self.test_tone.on = on,
                Command::Panic => {
                    self.playing = false;
                    self.declick.clear();
                    self.metronome.silence();
                    self.test_tone.silence();
                    self.scrub.silence();
                }
            }
        }
    }

    fn swap_timeline(&mut self, channels: usize) {
        // One swap per block, and only when the old timeline has a home.
        if self.stash.is_some() {
            return;
        }
        let Ok(incoming) = self.timelines.pop() else {
            return;
        };
        let at = self.ticks_now();
        // If the playhead moves (a tempo change before it), crossfade from
        // what the old timeline would have played, as every jump does.
        if incoming.sample_at(at, self.sample_rate) != self.position {
            self.capture_tail(channels);
        }
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
    /// When that is the same sample (an unchanged timeline, or a tempo
    /// change after the playhead), the audio carries on untouched.
    fn reposition(&mut self, at: Ticks) {
        let position = self.sample_at(at);
        let moved = position != self.position;
        self.position = position;
        self.refresh_regions();
        if self.playing && moved {
            self.locate_source(Jump::Reposition);
            self.declick.fade_in(fade_frames(self.sample_rate));
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
            marker: self.play_marker,
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
