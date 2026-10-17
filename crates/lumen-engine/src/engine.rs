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
//!
//! **Read ahead** (M4 slice 6, ADR-0041): when the source is latent (delay
//! compensation), the engine renders the timeline that far ahead of the
//! playhead, so what is heard lines up with the playhead and the
//! metronome. The playhead follows the render position (`ahead`) that far
//! behind, jumps included (`Playhead`); the source crossfades its own
//! jumps then, since a crossfade at the output would come before the jump
//! is heard. A change of latency while playing keeps the playhead going
//! and moves the render position, which skips or repeats the difference,
//! crossfaded.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_rt::DenormalGuard;

use crate::declick::{Declick, MAX_FADE, fade_frames};
use crate::metronome::{self, CountIn, Metronome, MetronomeSettings, TestTone};
use crate::playhead::Playhead;
use crate::scrub::{ScrubStyle, Scrubber};
use crate::snapshot::{Shared, TransportSnapshot};
use crate::source::{Jump, Mode, Output, RenderContext, Source};
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
    /// Play from the play marker and record (M5 slice 6): MIDI tracks that
    /// record keep what arrives, with its timeline position. First the song
    /// waits at the marker for `count_in` metronome beats (0 for none),
    /// clicking whether or not the metronome is on. While already playing,
    /// recording starts at once. Stop, Pause, Locate while counting and
    /// Panic end it.
    Record {
        count_in: u32,
    },
    /// Stop recording and carry on playing.
    EndRecord,
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
    /// test tone, scrubbing and any fade in progress. MIDI notes are cut too (M5 slice 1).
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
    /// The playhead: the timeline sample heard next.
    position: i64,
    /// The timeline sample the source renders next: the playhead plus
    /// `lookahead`, wrapped into the loop.
    ahead: i64,
    /// Frames the source's output trails what it renders.
    lookahead: i64,
    /// Where the playhead is while the source renders ahead.
    playhead: Playhead,
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
    /// Frames processed since the engine was made: the device clock MIDI
    /// out is timed against (`RenderContext::stream`).
    stream: u64,
    /// Recording, or counting in to it.
    recording: bool,
    /// The song waits at the play marker while these beats click.
    counting: Option<CountIn>,
    /// When plugin hosts must have answered for this callback: set at its
    /// start while a host is attached (ADR-0069), else `None`.
    deadline: Option<Instant>,
}

/// The share of a callback's period a plugin host has to answer in
/// (ADR-0069: three quarters, the rest is the engine's to finish in).
pub const HOST_SHARE: (u64, u64) = (3, 4);

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
        // The clock is read only while a host is attached (B3), so a project
        // without plugins costs what it did.
        self.deadline = self.source.hosted().then(|| {
            let period = frames as u64 * 1_000_000_000 / u64::from(sample_rate.max(1));
            Instant::now() + Duration::from_nanos(period * HOST_SHARE.0 / HOST_SHARE.1)
        });
        data[..channels * frames].fill(0.0);
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
                // An instrument still rings, or a note-off waits to go out.
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
        self.stream += frames as u64;
        self.publish();
    }

    /// The count-in's part of this block: the song waits at the play marker
    /// while the metronome clicks, and a live instrument still sounds. When
    /// the beats are done, playback starts mid-block and recording with it.
    /// Returns the frames used.
    fn count_in(&mut self, data: &mut [f32], channels: usize, frames: usize) -> usize {
        let Some(mut count) = self.counting else {
            return 0;
        };
        let rate = self.sample_rate;
        let n = (frames as i64).min(count.left()).max(0) as usize;
        if self.source.ringing() {
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
            let mut out = Output::new(data, frames, 0, n, channels);
            self.source.render(&cx, &mut out);
        } else {
            self.source.idle(rate, n);
        }
        if self.declick.active() {
            self.declick.apply(data, frames, channels, 0, n);
        }
        let window = metronome::Window {
            stride: frames,
            channels,
            offset: 0,
            count: n,
        };
        self.metronome.count(&mut count, rate, data, window);
        if count.left() > 0 {
            self.counting = Some(count);
        } else {
            self.counting = None;
            self.start();
        }
        n
    }

    /// Renders the block from frame `from` (earlier frames were a count-in).
    fn render(&mut self, data: &mut [f32], channels: usize, frames: usize, from: usize) {
        // Without latency the playhead is the render position, and the
        // `Playhead` is left alone (the transport gate counts every step).
        let latent = self.lookahead != 0;
        let mut offset = from;
        while offset < frames {
            let mut n = frames - offset;
            // The render position wraps at the loop end if it is before it.
            let wraps = self.loop_samples.filter(|&(_, end)| self.ahead < end);
            if let Some((_, end)) = wraps {
                n = n.min((end - self.ahead) as usize);
            }
            // The playhead moves on smoothly up to the next jump heard.
            if latent && let Some(until) = self.playhead.until_jump() {
                n = n.min(until.max(1) as usize);
            }
            let cx = RenderContext {
                timeline: &self.timeline,
                sample_rate: self.sample_rate,
                start: self.ahead,
                tail: false,
                stopping: false,
                mode: Mode::Play,
                stream: self.stream + offset as u64 + self.lookahead.max(0) as u64,
                device: self.stream + offset as u64,
                recording: self.recording,
                looping: self.loop_samples,
                deadline: self.deadline,
            };
            let mut out = Output::new(data, frames, offset, n, channels);
            self.source.render(&cx, &mut out);
            self.declick.apply(data, frames, channels, offset, n);
            // After the crossfade, so a click on a loop start keeps its
            // attack; none before the start is heard.
            if !(latent && self.playhead.prerolling()) {
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
            }
            self.ahead += n as i64;
            if latent {
                self.playhead.advance(n as i64);
            }
            offset += n;
            if let Some((start, end)) = wraps
                && self.ahead == end
            {
                // Crossfade what would follow the loop end into the loop start.
                self.declicked(channels, |engine| {
                    engine.ahead = start;
                    engine.locate_source(Jump::LoopWrap);
                });
            }
            self.position = self.heard();
        }
    }

    /// The playhead while playing: the render position, or behind it.
    fn heard(&self) -> i64 {
        if self.lookahead == 0 {
            self.ahead
        } else {
            self.playhead.position()
        }
    }

    /// Moves the render position with `jump`, crossfaded: here while the
    /// source is not latent, by the source otherwise. Only while playing.
    fn declicked(&mut self, channels: usize, jump: impl FnOnce(&mut Self)) {
        let here = self.lookahead == 0;
        if here {
            self.capture_tail(channels);
        }
        jump(self);
        if here {
            self.declick.fade_in(fade_frames(self.sample_rate));
        } else {
            self.playhead.jump(self.ahead);
        }
    }

    /// Follows a change in the source's latency. While playing, the
    /// playhead carries on and the render position moves to stay ahead of
    /// it by the new amount (wrapped into the loop), crossfaded.
    fn follow_latency(&mut self, channels: usize) {
        let lookahead = i64::from(self.source.latency());
        if lookahead == self.lookahead {
            return;
        }
        if !self.playing {
            self.lookahead = lookahead;
            return;
        }
        let playhead = self.position;
        let mut ahead = playhead + lookahead;
        if let Some((start, end)) = self.loop_samples
            && playhead < end
            && ahead >= end
        {
            ahead = start + (ahead - end) % (end - start);
        }
        // Crossfaded by whichever side is not latent after the change.
        if lookahead == 0 {
            self.capture_tail(channels);
        }
        self.lookahead = lookahead;
        self.playhead.resync(playhead, lookahead);
        self.ahead = ahead;
        self.locate_source(Jump::Locate);
        if lookahead == 0 {
            self.declick.fade_in(fade_frames(self.sample_rate));
        }
        self.playhead.jump(ahead);
        self.position = self.playhead.position();
    }

    /// Playback starts rendering at `self.ahead`.
    fn start(&mut self) {
        self.playing = true;
        self.locate_source(Jump::Play);
        // Read after the source's locate, which takes up a waiting
        // routing.
        self.lookahead = i64::from(self.source.latency());
        self.playhead.start(self.ahead, self.lookahead);
        self.position = self.playhead.position();
        self.declick.fade_in(fade_frames(self.sample_rate));
    }

    /// Stops where the playhead is: playback continues from what was
    /// heard.
    fn halt(&mut self, channels: usize) {
        self.capture(channels, true);
        self.playing = false;
        self.recording = false;
        self.ahead = self.position;
        self.source.stopped();
    }

    /// Places the stopped transport at `sample`.
    fn place(&mut self, sample: i64) {
        self.position = sample;
        self.ahead = sample;
    }

    /// Renders what would follow the current position into the declick
    /// tail, faded out, ahead of a jump. Only while playing.
    fn capture_tail(&mut self, channels: usize) {
        self.capture(channels, false);
    }

    /// As [`Self::capture_tail`]; `stopping` when playback is about to
    /// stop rather than jump (instruments carry on ringing after a stop,
    /// so they leave the tail out).
    fn capture(&mut self, channels: usize, stopping: bool) {
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
            start: self.ahead,
            tail: true,
            stopping,
            mode: Mode::Play,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: self.deadline,
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
                    // A count-in already leads to playing.
                    if !self.playing && self.counting.is_none() {
                        self.scrub.end();
                        self.ahead = if command == Command::Play {
                            self.sample_at(self.play_marker)
                        } else {
                            self.position
                        };
                        self.start();
                    }
                }
                Command::Record { count_in } => self.record(count_in),
                Command::EndRecord => self.recording = false,
                Command::Pause => {
                    if self.cancel_count_in() {
                    } else if self.playing {
                        self.halt(channels);
                    }
                }
                Command::Stop => {
                    if self.cancel_count_in() {
                    } else if self.playing {
                        self.halt(channels);
                    } else {
                        self.play_marker = Ticks::ZERO;
                        self.place(0);
                    }
                }
                Command::Locate(at) => {
                    self.play_marker = at;
                    let sample = self.sample_at(at);
                    self.cancel_count_in();
                    if self.playing {
                        self.declicked(channels, |engine| {
                            engine.ahead = sample;
                            engine.locate_source(Jump::Locate);
                        });
                        self.position = self.heard();
                    } else {
                        self.place(sample);
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
                        self.place(self.sample_at(at));
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
                    self.recording = false;
                    self.counting = None;
                    self.ahead = self.position;
                    self.declick.clear();
                    self.metronome.silence();
                    self.test_tone.silence();
                    self.scrub.silence();
                    self.source.panic();
                }
            }
        }
    }

    /// Starts recording (Command::Record).
    fn record(&mut self, count_in: u32) {
        if self.playing {
            self.recording = true;
        } else if self.counting.is_none() {
            self.scrub.end();
            let marker = self.sample_at(self.play_marker);
            self.recording = true;
            if count_in == 0 {
                self.ahead = marker;
                self.start();
                return;
            }
            self.place(marker);
            let meter = self.timeline.meter.meter_at(self.play_marker);
            let beat = self.sample_at(self.play_marker + meter.beat()) - marker;
            self.counting = Some(CountIn {
                beat: beat.max(1),
                beats: count_in,
                per_bar: meter.numerator.max(1),
                done: 0,
            });
        }
    }

    /// Ends a count-in and the recording that waits behind it. Whether
    /// there was one.
    fn cancel_count_in(&mut self) -> bool {
        let was = self.counting.take().is_some();
        if was {
            self.recording = false;
        }
        was
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
        let ahead = self.timeline.ticks_at(self.ahead, self.sample_rate);
        // If the render position moves (a tempo change before it),
        // crossfade from what the old timeline would have played, as every
        // jump does.
        let rate = self.sample_rate;
        if self.lookahead == 0 && incoming.sample_at(ahead, rate) != self.ahead {
            self.capture_tail(channels);
        }
        let timeline = &self.timeline;
        let convert = |s| incoming.sample_at(timeline.ticks_at(s, rate), rate);
        self.playhead
            .reposition(incoming.sample_at(at, rate), convert);
        let old = std::mem::replace(&mut self.timeline, incoming);
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
        self.reposition(at, ahead);
    }

    fn retry_stash(&mut self) {
        if let Some(old) = self.stash.take()
            && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
        {
            self.stash = Some(old);
        }
    }

    /// After a tempo or rate change: the playhead at `at` and the render
    /// position at `ahead`, the same musical positions in new samples.
    /// When the render position is the same sample (an unchanged timeline,
    /// or a tempo change after it), the audio carries on untouched. Call
    /// after `Playhead::reposition`.
    fn reposition(&mut self, at: Ticks, ahead: Ticks) {
        self.position = self.sample_at(at);
        let ahead = self.sample_at(ahead);
        let moved = ahead != self.ahead;
        self.ahead = ahead;
        self.refresh_regions();
        if self.playing && moved {
            self.locate_source(Jump::Reposition);
            if self.lookahead == 0 {
                self.declick.fade_in(fade_frames(self.sample_rate));
            }
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
            .locate(&self.timeline, self.sample_rate, self.ahead, jump);
    }

    fn sample_at(&self, t: Ticks) -> i64 {
        self.timeline.sample_at(t, self.sample_rate)
    }

    fn ticks_now(&self) -> Ticks {
        self.timeline.ticks_at(self.position, self.sample_rate)
    }

    fn snapshot(&self) -> TransportSnapshot {
        // While pre-rolling, the playhead shows where playback started.
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
}
