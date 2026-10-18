//! What the engine asks of a plugin host (M6 slice 2b, ADR-0069).
//!
//! A host runs a plugin in another process, and the engine never calls plugin
//! code. A block is handed over with [`Hosted::submit`], which never blocks,
//! and taken back with [`Hosted::collect`], which waits at most until the
//! callback's one deadline. Keeping the two apart lets the engine submit to
//! every host before it waits for any, so the hosts work while the engine
//! does.
//!
//! [`Guard`] adds the rules every user of a host shares: a block that cannot
//! be answered is silence (or the unprocessed input, for an effect); a host
//! that stays late for [`FAILED_AFTER_FRAMES`] of audio in a row, or whose
//! process has ended, is failed for good; a changed latency is reported once. It tells the
//! interface what it learns through a [`Status`] of atomics, and it listens to
//! the host's output, so a plugin is given blocks until what it plays has gone
//! quiet ([`Guard::sounding`]) rather than for a tail the plugin may misreport.
//! [`HostedInstrument`] plays a host as an [`Instrument`]. Everything here is
//! real-time safe: no
//! allocation, no lock, and the only wait is the one `collect` makes, bounded
//! by the deadline.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use lumen_core::midi::{Event, Kind, Msg, cc};

use crate::Instrument;

mod timing;

pub use timing::Cost;
use timing::Costs;

/// Frames of audio a host may stay late for, in a row, before it is failed:
/// about a second at 48 kHz. Blocks are silence (or the unprocessed input)
/// meanwhile. A count of blocks would fail a plugin whose first blocks are slow
/// (it fills its state, or loads samples) in a few milliseconds at small
/// buffers; a span of audio does not depend on the buffer size (ADR-0069,
/// amended).
pub const FAILED_AFTER_FRAMES: u64 = 48_000;

/// The most frames a host is given in one block: the engine splits longer
/// blocks (its mixer's chunk). Plugins are configured with it as their
/// largest block, and some size their work by it on every call: a plugin
/// told 8192 frames spent six times as long on a 128-frame block as one
/// told 256 (ADR-0075).
pub const MOST_FRAMES: usize = 256;

/// A plugin's output below this, on every sample of a block, is quiet:
/// −90 dBFS, under what a 16-bit file can hold.
pub const QUIET_LEVEL: f32 = 3.162_278e-5;

/// How long a plugin's output stays quiet before the engine stops giving it
/// blocks, unless the plugin reports a longer tail. Long enough for the gap
/// before a slow delay's next echo.
pub const QUIET_SECONDS: u32 = 2;

/// The longest reported tail the engine waits through (a plugin may report an
/// endless one); after it, only the output decides.
const TAIL_MOST_SECONDS: u32 = 10;

/// Why a host did not take a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The host is still on an earlier block.
    Busy,
    /// The host's process has ended.
    Gone,
}

/// How a host answered a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// In time: the output is in the buffers. `latency` and `tail` are what
    /// the plugin reports now, in frames; `restart` says it asked to be
    /// restarted (a change of buses or latency the engine should follow);
    /// `work_ns` is how long the plugin worked on the block.
    Ready {
        latency: u32,
        tail: u32,
        restart: bool,
        work_ns: u32,
    },
    /// Not by the deadline: this block is silence, and the block is still in
    /// flight.
    Late,
    /// The host's process has ended.
    Gone,
}

/// A plugin running in a host process.
pub trait Hosted: Send {
    /// Hands the host a block of `frames`. `input` holds the channels to
    /// process (none for an instrument) and `events` the MIDI of the block,
    /// offsets within it. Never blocks.
    fn submit(&mut self, frames: usize, input: &[&[f32]], events: &[Event]) -> Result<(), Refused>;

    /// Waits until `deadline` for the answer to the last submitted block and,
    /// when it is [`Reply::Ready`], overwrites `left` and `right` with the
    /// output (finite samples only).
    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply;

    /// Whether the host's process has ended, without waiting.
    fn ended(&self) -> bool;

    /// The output of bus pair `pair` (0 is the main bus, then the auxiliary
    /// outputs that are on, in bus order) for the block [`Hosted::collect`]
    /// last answered, into `left` and `right`. False when there is no such
    /// pair; a mono bus is copied to both sides. Never blocks.
    fn output(&mut self, _pair: usize, _left: &mut [f32], _right: &mut [f32]) -> bool {
        false
    }

    /// Asks the plugin to forget what it holds (its tail, its filters)
    /// before the next block it is given. Never blocks. A host with nothing
    /// to forget ignores it.
    fn flush(&mut self) {}
}

/// What a block came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The buffers hold the host's output.
    Ready,
    /// They do not: use silence, or for an effect the unprocessed input.
    Silent,
}

/// What the engine has learned about a host, readable from the interface
/// thread: atomics, written by the audio thread.
#[derive(Debug)]
pub struct Status {
    failed: AtomicBool,
    late: AtomicU32,
    latency: AtomicU32,
    tail: AtomicU32,
    news: AtomicBool,
    restart: AtomicBool,
    /// The one thing the interface writes: the device is bypassed.
    bypass: AtomicBool,
    costs: Costs,
}

impl Status {
    fn new(latency: u32) -> Arc<Self> {
        Arc::new(Self {
            failed: AtomicBool::new(false),
            late: AtomicU32::new(0),
            latency: AtomicU32::new(latency),
            tail: AtomicU32::new(0),
            news: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            bypass: AtomicBool::new(false),
            costs: Costs::default(),
        })
    }

    /// What the plugin's blocks cost since the last call: its work in the
    /// host and the audio thread's wait for it.
    pub fn take_cost(&self) -> Cost {
        self.costs.take()
    }

    /// Bypasses the device (an effect then passes its input through, delayed
    /// by its latency) or turns it on again. Safe from any thread.
    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Release);
    }

    pub fn bypassed(&self) -> bool {
        self.bypass.load(Ordering::Acquire)
    }

    /// The host is given up on: it missed too many blocks or ended. A failed
    /// host is never submitted to again; it is replaced.
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Blocks that went without the host's answer, since it was attached.
    pub fn late_blocks(&self) -> u32 {
        self.late.load(Ordering::Relaxed)
    }

    /// The plugin's latency in frames, as last reported.
    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    /// The plugin's tail in frames, as last reported.
    pub fn tail(&self) -> u32 {
        self.tail.load(Ordering::Relaxed)
    }

    /// The new latency, once, after the plugin reported a different one.
    pub fn take_news(&self) -> Option<u32> {
        self.news
            .swap(false, Ordering::AcqRel)
            .then(|| self.latency())
    }

    /// Whether the plugin asked to be restarted since last asked.
    pub fn take_restart(&self) -> bool {
        self.restart.swap(false, Ordering::AcqRel)
    }
}

/// A host and the rules for using it.
pub struct Guard {
    host: Box<dyn Hosted>,
    /// A block was submitted and not yet collected.
    sent: bool,
    /// The frames of the block in flight.
    frames: usize,
    /// Frames of audio late in a row.
    missed: u64,
    /// Frames since the host's output was last above [`QUIET_LEVEL`]; it
    /// starts quiet.
    quiet: u64,
    /// A block was asked for since the last [`Guard::age`].
    asked: bool,
    /// The last block collected was the host's (see [`Guard::output`]).
    ready: bool,
    status: Arc<Status>,
}

impl Guard {
    /// A host whose plugin last reported `latency` frames. Allocates (the
    /// [`Status`]): call it on the interface side.
    pub fn new(host: Box<dyn Hosted>, latency: u32) -> Self {
        Self {
            host,
            sent: false,
            frames: 0,
            missed: 0,
            quiet: u64::MAX,
            asked: false,
            ready: false,
            status: Status::new(latency),
        }
    }

    /// Whether the host's output may still be heard at `rate`: it was above
    /// [`QUIET_LEVEL`] within the last [`QUIET_SECONDS`], or within its
    /// reported tail when that is longer (at most ten seconds). A failed host
    /// is not.
    pub fn sounding(&self, rate: u32) -> bool {
        let rate = u64::from(rate);
        let tail = u64::from(self.tail()).min(rate * u64::from(TAIL_MOST_SECONDS));
        let window = (rate * u64::from(QUIET_SECONDS)).max(tail);
        !self.failed() && self.quiet < window
    }

    /// Something was said that may make the plugin sound (a note, a pedal):
    /// it counts as sounding from now, before its output says so.
    pub fn wake(&mut self) {
        self.quiet = 0;
    }

    /// The plugin was told to stop every sound: it counts as quiet at once.
    pub fn hush(&mut self) {
        self.quiet = u64::MAX;
    }

    /// The plugin is asked to forget what it holds before its next block: a
    /// reverb's tail must not carry to a new place in the song. It keeps
    /// being asked for blocks and is heard quiet as soon as its output is.
    pub fn flush(&mut self) {
        if !self.failed() {
            self.host.flush();
        }
    }

    /// `frames` went by: a host that was not asked for a block since the
    /// last call has been quiet for them (its node did not run).
    pub fn age(&mut self, frames: usize) {
        if !std::mem::take(&mut self.asked) {
            self.quiet = self.quiet.saturating_add(frames as u64);
        }
    }

    /// Follows how loud the block's output was.
    fn heard(&mut self, left: &[f32], right: &[f32]) {
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(left.len() as u64);
        }
    }

    fn quiet_for(&mut self, frames: usize) {
        self.quiet = self.quiet.saturating_add(frames as u64);
    }

    /// What the interface may read about the host.
    pub fn status(&self) -> Arc<Status> {
        self.status.clone()
    }

    pub fn failed(&self) -> bool {
        self.status.failed()
    }

    /// The interface bypassed the device.
    pub fn bypassed(&self) -> bool {
        self.status.bypassed()
    }

    pub fn latency(&self) -> u32 {
        self.status.latency()
    }

    pub fn tail(&self) -> u32 {
        self.status.tail()
    }

    pub fn late_blocks(&self) -> u32 {
        self.status.late_blocks()
    }

    pub fn take_news(&mut self) -> Option<u32> {
        self.status.take_news()
    }

    pub fn take_restart(&mut self) -> bool {
        self.status.take_restart()
    }

    /// Hands the host a block. Returns whether it took it; when it did not,
    /// [`Guard::finish`] answers [`Outcome::Silent`] at once.
    pub fn begin(&mut self, frames: usize, input: &[&[f32]], events: &[Event]) -> bool {
        self.sent = false;
        self.frames = frames;
        self.asked = true;
        if self.failed() {
            return false;
        }
        if self.host.ended() {
            self.fail();
            return false;
        }
        match self.host.submit(frames, input, events) {
            Ok(()) => {
                self.sent = true;
                true
            }
            Err(Refused::Busy) => {
                self.miss();
                false
            }
            Err(Refused::Gone) => {
                self.fail();
                false
            }
        }
    }

    /// Bus pair `pair` (0 is the main bus, then the auxiliary outputs that
    /// are on) of the block [`Guard::finish`] last answered, into `left` and
    /// `right`: false when that block was not the host's or there is no such
    /// pair. Finite samples only. Never blocks.
    pub fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if !(self.ready && self.host.output(pair, left, right)) {
            return false;
        }
        // An instrument that plays only on its other outputs is still
        // sounding: they count as much as the main one.
        let loud = |side: &[f32]| side.iter().any(|s| s.abs() > QUIET_LEVEL);
        if loud(left) || loud(right) {
            self.quiet = 0;
        }
        true
    }

    /// Takes the block's answer, waiting until `deadline` at most.
    pub fn finish(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Outcome {
        self.ready = false;
        if !std::mem::take(&mut self.sent) {
            self.quiet_for(left.len());
            return Outcome::Silent;
        }
        let outcome = self.collect(deadline, left, right);
        self.ready = outcome == Outcome::Ready;
        match outcome {
            Outcome::Ready => self.heard(left, right),
            Outcome::Silent => self.quiet_for(left.len()),
        }
        outcome
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Outcome {
        let asked = Instant::now();
        let reply = self.host.collect(deadline, left, right);
        let waited = asked.elapsed();
        match reply {
            Reply::Ready {
                latency,
                tail,
                restart,
                work_ns,
            } => {
                self.status.costs.note(Some(work_ns), waited);
                self.missed = 0;
                if self.status.latency.swap(latency, Ordering::Relaxed) != latency {
                    self.status.news.store(true, Ordering::Release);
                }
                self.status.tail.store(tail, Ordering::Relaxed);
                if restart {
                    self.status.restart.store(true, Ordering::Release);
                }
                Outcome::Ready
            }
            Reply::Late => {
                self.status.costs.note(None, waited);
                self.miss();
                if self.host.ended() {
                    self.fail();
                }
                Outcome::Silent
            }
            Reply::Gone => {
                self.fail();
                Outcome::Silent
            }
        }
    }

    fn fail(&mut self) {
        self.status.failed.store(true, Ordering::Release);
    }

    fn miss(&mut self) {
        self.missed += self.frames as u64;
        self.status.late.fetch_add(1, Ordering::Relaxed);
        if self.missed >= FAILED_AFTER_FRAMES {
            self.fail();
        }
    }
}

/// The most events one hosted instrument takes to its host in a block; a
/// storm past it is dropped and counted.
const EVENTS: usize = 2048;

/// Whether a message may make the plugin sound: everything but the one that
/// stops every sound at once.
fn wakes(msg: Msg) -> bool {
    !matches!(
        msg.kind(),
        Kind::Controller {
            number: cc::ALL_SOUND_OFF,
            ..
        }
    )
}

/// The notes held down and the channels with the sustain pedal down, which
/// say whether an instrument still sounds without a word being said. A held
/// note keeps the host given blocks even while its output is quiet (a slow
/// attack, a swell).
struct Holding {
    /// A bit for each channel and key.
    notes: [u64; 32],
    /// A bit for each channel with the pedal down.
    pedals: u16,
    /// How many notes are down: asked every block, so not counted from the
    /// bits each time.
    down: u32,
}

impl Holding {
    const NONE: Self = Self {
        notes: [0; 32],
        pedals: 0,
        down: 0,
    };

    fn any(&self) -> bool {
        self.down != 0 || self.pedals != 0
    }

    fn clear(&mut self) {
        *self = Self::NONE;
    }

    fn set(&mut self, channel: u8, key: u8, down: bool) {
        let bit = usize::from(channel & 15) * 128 + usize::from(key & 127);
        let (word, mask) = (bit / 64, 1_u64 << (bit % 64));
        let was = self.notes[word] & mask != 0;
        if down && !was {
            self.notes[word] |= mask;
            self.down += 1;
        } else if !down && was {
            self.notes[word] &= !mask;
            self.down -= 1;
        }
    }

    fn note(&mut self, msg: Msg) {
        let channel = msg.channel();
        match msg.kind() {
            Kind::NoteOn { key, .. } => self.set(channel, key, true),
            Kind::NoteOff { key, .. } => self.set(channel, key, false),
            Kind::Controller { number, value } => match number {
                cc::SUSTAIN if value >= 64 => self.pedals |= 1 << channel,
                cc::SUSTAIN => self.pedals &= !(1 << channel),
                // The plugin lets go of what it holds, pedal or not.
                cc::ALL_SOUND_OFF | cc::ALL_NOTES_OFF => self.clear(),
                _ => {}
            },
            _ => {}
        }
    }
}

/// A plugin instrument in a host, played as an [`Instrument`] (M6 slice 2b).
/// The app builds one when it attaches a host, and a [`placeholder`] in every
/// later track list: the engine carries a track's instrument across lists by
/// its kind, so the real one stays and the placeholder is dropped.
///
/// [`placeholder`]: HostedInstrument::placeholder
pub struct HostedInstrument {
    guard: Option<Guard>,
    /// Messages that arrived with no block to ride on, for the next block.
    pending: Vec<Event>,
    /// The block's events, pending ones first.
    block: Vec<Event>,
    begun: bool,
    rate: u32,
    holding: Holding,
    dropped: u32,
}

impl HostedInstrument {
    /// Allocates; call off the audio thread.
    pub fn new(guard: Guard) -> Self {
        Self {
            guard: Some(guard),
            pending: Vec::with_capacity(EVENTS),
            block: Vec::with_capacity(EVENTS),
            begun: false,
            rate: 0,
            holding: Holding::NONE,
            dropped: 0,
        }
    }

    /// A stand-in with no host, for a track list whose instrument is already
    /// attached. Allocates nothing.
    pub fn placeholder() -> Self {
        Self {
            guard: None,
            pending: Vec::new(),
            block: Vec::new(),
            begun: false,
            rate: 0,
            holding: Holding::NONE,
            dropped: 0,
        }
    }

    /// Events dropped because a block had more than the host takes.
    pub fn dropped_events(&self) -> u32 {
        self.dropped
    }
}

impl Instrument for HostedInstrument {
    fn hosted(&self) -> bool {
        self.guard.is_some()
    }

    fn begin(&mut self, rate: u32, frames: usize, events: &[Event]) -> bool {
        self.begun = false;
        if rate > 0 {
            self.rate = rate;
        }
        let rate = self.rate;
        let Some(guard) = self.guard.as_mut() else {
            return false;
        };
        let held = self.holding.any();
        if rate == 0
            || (events.is_empty() && self.pending.is_empty() && !held && !guard.sounding(rate))
        {
            // Nothing sounds and nothing is said: the host is left alone.
            return false;
        }
        self.block.clear();
        self.block.extend_from_slice(&self.pending);
        self.pending.clear();
        let room = EVENTS - self.block.len();
        self.block.extend(events.iter().take(room));
        self.dropped += events.len().saturating_sub(room) as u32;
        let sent = guard.begin(frames, &[], &self.block);
        if !sent && !guard.failed() {
            // The host is still on an earlier block: what was said goes with
            // the next one, at its start, rather than being lost (a lost
            // note-off would leave a note hanging).
            let said = self.block.iter().map(|e| Event {
                offset: 0,
                msg: e.msg,
            });
            self.pending.extend(said);
        }
        let mut woken = false;
        for event in &self.block {
            self.holding.note(event.msg);
            woken |= wakes(event.msg);
        }
        // What was said, and held notes and a held pedal, keep it sounding
        // block after block; after that, its output decides.
        if woken || self.holding.any() {
            guard.wake();
        }
        self.begun = sent;
        sent
    }

    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        self.guard
            .as_mut()
            .is_some_and(|guard| guard.output(pair, left, right))
    }

    fn finish(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) {
        if !std::mem::take(&mut self.begun) {
            return;
        }
        if let Some(guard) = self.guard.as_mut() {
            guard.finish(deadline, left, right);
        }
    }

    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
        let frames = left.len();
        if self.guard.is_none() {
            return;
        }
        if frames == 0 {
            // Messages only (a track let go of its notes): they go with the
            // next block.
            for event in events {
                if self.pending.len() < EVENTS {
                    self.pending.push(Event {
                        offset: 0,
                        msg: event.msg,
                    });
                } else {
                    self.dropped += 1;
                }
            }
            return;
        }
        if self.begin(rate, frames, events) {
            let rate = u64::from(self.rate.max(1));
            let deadline = Instant::now() + Duration::from_micros(frames as u64 * 750_000 / rate);
            self.finish(deadline, left, right);
        }
    }

    fn active(&self) -> bool {
        // Messages waiting for a block count: a note-off must reach the host
        // even when the project has stopped.
        self.guard.as_ref().is_some_and(|g| {
            !g.failed() && (!self.pending.is_empty() || self.holding.any() || g.sounding(self.rate))
        })
    }

    fn silence(&mut self) {
        self.holding.clear();
        let Some(guard) = self.guard.as_mut() else {
            return;
        };
        guard.hush();
        if self.pending.len() < EVENTS {
            self.pending.push(Event {
                offset: 0,
                msg: Msg::controller(0, cc::ALL_SOUND_OFF, 0),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// What a fake host was asked, and the level it answers with (the test
    /// may change it while the host is in use).
    #[derive(Default)]
    struct Seen {
        submits: u32,
        /// Submits still to refuse as busy, as a host on an earlier block.
        busy: u32,
        events: Vec<Vec<Event>>,
        value: f32,
    }

    /// A host that answers from a script and records what it is asked.
    struct Fake {
        script: Vec<Reply>,
        next: usize,
        refuse: Option<Refused>,
        ended: bool,
        seen: Arc<Mutex<Seen>>,
    }

    impl Fake {
        fn new(script: Vec<Reply>) -> (Box<Self>, Arc<Mutex<Seen>>) {
            let seen = Arc::new(Mutex::new(Seen {
                value: 0.5,
                ..Seen::default()
            }));
            let fake = Box::new(Self {
                script,
                next: 0,
                refuse: None,
                ended: false,
                seen: seen.clone(),
            });
            (fake, seen)
        }
    }

    impl Hosted for Fake {
        fn submit(&mut self, _: usize, _: &[&[f32]], events: &[Event]) -> Result<(), Refused> {
            if let Some(why) = self.refuse {
                return Err(why);
            }
            let mut seen = self.seen.lock().unwrap();
            if seen.busy > 0 {
                seen.busy -= 1;
                return Err(Refused::Busy);
            }
            seen.submits += 1;
            seen.events.push(events.to_vec());
            Ok(())
        }

        fn collect(&mut self, _: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
            let reply = self.script[self.next.min(self.script.len() - 1)];
            self.next += 1;
            if matches!(reply, Reply::Ready { .. }) {
                let value = self.seen.lock().unwrap().value;
                left.fill(value);
                right.fill(-value);
            }
            reply
        }

        fn ended(&self) -> bool {
            self.ended
        }
    }

    const READY: Reply = Reply::Ready {
        latency: 0,
        tail: 0,
        restart: false,
        work_ns: 40_000,
    };

    fn submits(seen: &Arc<Mutex<Seen>>) -> u32 {
        seen.lock().unwrap().submits
    }

    fn run(guard: &mut Guard) -> (Outcome, [f32; 2]) {
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        guard.begin(4, &[], &[]);
        let outcome = guard.finish(Instant::now() + Duration::from_millis(1), &mut l, &mut r);
        (outcome, [l[0], r[0]])
    }

    #[test]
    fn a_ready_block_is_the_hosts_output() {
        let (host, _) = Fake::new(vec![READY]);
        let mut guard = Guard::new(host, 0);
        assert_eq!(run(&mut guard), (Outcome::Ready, [0.5, -0.5]));
        assert!(!guard.failed());
    }

    #[test]
    fn the_plugins_work_and_the_wait_for_it_are_counted_late_blocks_too() {
        let (host, _) = Fake::new(vec![READY, Reply::Late]);
        let mut guard = Guard::new(host, 0);
        let status = guard.status();
        run(&mut guard);
        run(&mut guard);
        let cost = status.take_cost();
        assert_eq!(cost.blocks, 2, "both blocks were waited for");
        assert_eq!(
            cost.work_mean,
            Duration::from_micros(40),
            "work of the answer"
        );
        assert_eq!(status.take_cost().blocks, 0, "taken");
    }

    #[test]
    fn a_changed_latency_is_news_once_and_a_restart_is_asked_once() {
        let change = Reply::Ready {
            latency: 128,
            tail: 9,
            restart: true,
            work_ns: 0,
        };
        let (host, _) = Fake::new(vec![READY, change, change]);
        let mut guard = Guard::new(host, 0);
        let status = guard.status();
        run(&mut guard);
        assert_eq!(status.take_news(), None, "unchanged is not news");
        run(&mut guard);
        assert_eq!(status.take_news(), Some(128));
        assert_eq!(status.take_news(), None, "told once");
        assert!(status.take_restart());
        assert!(!status.take_restart());
        assert_eq!((status.latency(), status.tail()), (128, 9));
        run(&mut guard);
        assert_eq!(
            status.take_news(),
            None,
            "the same latency again is not news"
        );
    }

    #[test]
    fn a_second_of_late_audio_in_a_row_fails_the_host_and_an_answer_in_between_resets_it() {
        // Blocks of 4 frames: the span is counted in audio, not blocks.
        let per_second = (FAILED_AFTER_FRAMES / 4) as usize;
        let mut script = vec![Reply::Late; per_second - 1];
        script.push(READY);
        script.extend(vec![Reply::Late; per_second]);
        let (host, seen) = Fake::new(script);
        let mut guard = Guard::new(host, 0);
        for _ in 0..per_second - 1 {
            assert_eq!(run(&mut guard).0, Outcome::Silent);
        }
        assert!(!guard.failed(), "a few milliseconds late is a glitch");
        assert_eq!(run(&mut guard).0, Outcome::Ready);
        for _ in 0..per_second - 1 {
            assert_eq!(run(&mut guard).0, Outcome::Silent);
        }
        assert!(
            !guard.failed(),
            "an answer in between started the count again"
        );
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert!(guard.failed(), "a second late in a row");
        assert_eq!(guard.late_blocks(), (2 * per_second - 1) as u32);
        let asked = submits(&seen);
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert_eq!(
            submits(&seen),
            asked,
            "a failed host is never submitted to again"
        );
    }

    #[test]
    fn a_host_that_is_busy_gives_silence_and_counts() {
        let (mut host, _) = Fake::new(vec![READY]);
        host.refuse = Some(Refused::Busy);
        let mut guard = Guard::new(host, 0);
        for _ in 0..FAILED_AFTER_FRAMES / 4 - 1 {
            assert_eq!(run(&mut guard).0, Outcome::Silent);
        }
        assert!(!guard.failed());
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert!(guard.failed());
    }

    #[test]
    fn an_ended_or_gone_host_fails_at_once() {
        let (mut host, _) = Fake::new(vec![READY]);
        host.ended = true;
        let mut guard = Guard::new(host, 0);
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert!(guard.failed());

        let (mut host, _) = Fake::new(vec![READY]);
        host.refuse = Some(Refused::Gone);
        let mut guard = Guard::new(host, 0);
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert!(guard.failed());

        let (host, _) = Fake::new(vec![Reply::Gone]);
        let mut guard = Guard::new(host, 0);
        assert_eq!(run(&mut guard).0, Outcome::Silent);
        assert!(guard.failed());
    }

    #[test]
    fn finishing_without_a_block_in_flight_is_silence() {
        let (host, seen) = Fake::new(vec![READY]);
        let mut guard = Guard::new(host, 0);
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        let outcome = guard.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(outcome, Outcome::Silent);
        assert_eq!(submits(&seen), 0);
    }

    fn on(offset: u32, key: u8) -> Event {
        Event {
            offset,
            msg: Msg::note_on(0, key, 100),
        }
    }

    fn instrument(script: Vec<Reply>) -> (HostedInstrument, Arc<Mutex<Seen>>) {
        let (host, seen) = Fake::new(script);
        (HostedInstrument::new(Guard::new(host, 0)), seen)
    }

    #[test]
    fn begin_hands_over_the_events_at_their_frames_and_finish_takes_the_output() {
        let (mut synth, seen) = instrument(vec![READY]);
        let events = [on(0, 60), on(37, 64)];
        assert!(synth.begin(48_000, 64, &events));
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        synth.finish(Instant::now() + Duration::from_millis(5), &mut l, &mut r);
        assert_eq!(seen.lock().unwrap().events, vec![events.to_vec()]);
        assert_eq!((l[63], r[63]), (0.5, -0.5));
    }

    #[test]
    fn render_without_an_early_begin_does_the_same_in_one_call() {
        let (mut synth, seen) = instrument(vec![READY]);
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        synth.render(48_000, &[on(5, 60)], &mut l, &mut r);
        assert_eq!(seen.lock().unwrap().events, vec![vec![on(5, 60)]]);
        assert_eq!((l[0], r[0]), (0.5, -0.5));
    }

    #[test]
    fn a_message_with_no_block_rides_on_the_next_one() {
        let (mut synth, seen) = instrument(vec![READY]);
        let off = Event {
            offset: 33,
            msg: Msg::note_off(0, 60),
        };
        synth.render(0, &[off], &mut [], &mut []);
        assert_eq!(submits(&seen), 0, "nothing is submitted for no frames");
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        synth.render(48_000, &[on(7, 62)], &mut l, &mut r);
        let sent = seen.lock().unwrap().events.clone();
        assert_eq!(
            sent,
            vec![vec![
                Event {
                    offset: 0,
                    msg: off.msg
                },
                on(7, 62)
            ]],
            "the carried message goes first, at the block's start"
        );
    }

    /// Blocks the instrument is given, at 1 kHz in blocks of 100 frames, until
    /// it is left alone (at most `most`).
    fn rung(synth: &mut HostedInstrument, most: usize) -> usize {
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        let mut blocks = 0;
        while synth.begin(1000, 100, &[]) {
            synth.finish(Instant::now(), &mut l, &mut r);
            blocks += 1;
            assert!(blocks <= most, "it must stop being rung");
        }
        blocks
    }

    /// At 1 kHz, blocks of 100 frames: the quiet that ends a plugin's turn.
    const QUIET_BLOCKS: usize = (QUIET_SECONDS * 10) as usize;

    fn quiet(seen: &Arc<Mutex<Seen>>) {
        seen.lock().unwrap().value = 0.0;
    }

    #[test]
    fn a_host_is_left_alone_while_nothing_sounds_and_rung_out_after_a_note_ends() {
        // A plugin that reports no tail and plays nothing after the note: it is
        // given blocks until two seconds of quiet have gone by.
        let (mut synth, seen) = instrument(vec![READY]);
        quiet(&seen);
        assert!(!synth.begin(1000, 100, &[]), "idle: no host work");
        assert!(!synth.active());
        let off = Event {
            offset: 50,
            msg: Msg::note_off(0, 60),
        };
        assert!(synth.begin(1000, 100, &[on(0, 60), off]));
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        synth.finish(Instant::now(), &mut l, &mut r);
        assert!(synth.active());
        assert_eq!(
            rung(&mut synth, 100),
            QUIET_BLOCKS - 1,
            "two seconds of quiet, counted from the block with the event"
        );
        assert!(!synth.active());
        assert_eq!(submits(&seen), QUIET_BLOCKS as u32);
    }

    #[test]
    fn a_host_whose_output_still_sounds_is_given_blocks_until_it_goes_quiet() {
        // The owner's report: a synth's release, and a delay in it, were cut a
        // second after the last note. Here the plugin keeps sounding for ten
        // seconds after the note ends, far past any guessed tail.
        let (mut synth, seen) = instrument(vec![READY]);
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        let off = Event {
            offset: 50,
            msg: Msg::note_off(0, 60),
        };
        assert!(synth.begin(1000, 100, &[on(0, 60), off]));
        synth.finish(Instant::now(), &mut l, &mut r);
        for block in 0..100 {
            assert!(synth.begin(1000, 100, &[]), "block {block}: still sounding");
            synth.finish(Instant::now(), &mut l, &mut r);
            assert_eq!(l[99], 0.5, "block {block}: the host's output is heard");
        }
        quiet(&seen);
        assert_eq!(
            rung(&mut synth, 100),
            QUIET_BLOCKS,
            "two seconds after the last sound, then left alone"
        );
        assert!(!synth.active());
    }

    #[test]
    fn a_reported_tail_longer_than_the_quiet_is_waited_through_up_to_ten_seconds() {
        for (tail, blocks) in [(5_000, 50), (u32::MAX, 100)] {
            let (mut synth, seen) = instrument(vec![Reply::Ready {
                latency: 0,
                tail,
                restart: false,
                work_ns: 0,
            }]);
            quiet(&seen);
            let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
            assert!(synth.begin(1000, 100, &[on(0, 60)]));
            synth.finish(Instant::now(), &mut l, &mut r);
            let off = Event {
                offset: 0,
                msg: Msg::note_off(0, 60),
            };
            assert!(synth.begin(1000, 100, &[off]));
            synth.finish(Instant::now(), &mut l, &mut r);
            assert_eq!(rung(&mut synth, 200), blocks - 1, "a tail of {tail} frames");
        }
    }

    #[test]
    fn a_guard_follows_quiet_while_its_node_does_not_run() {
        let (host, seen) = Fake::new(vec![READY]);
        let mut guard = Guard::new(host, 0);
        assert!(!guard.sounding(1000), "a new host has played nothing");
        run(&mut guard);
        assert!(guard.sounding(1000));
        // The block it was asked for is counted by its output, not again.
        guard.age(4);
        guard.age(1_999);
        assert!(guard.sounding(1000));
        guard.age(1);
        assert!(!guard.sounding(1000), "two seconds unasked");
        guard.wake();
        assert!(guard.sounding(1000));
        guard.hush();
        assert!(!guard.sounding(1000));
        quiet(&seen);
        run(&mut guard);
        assert!(!guard.sounding(1000), "a quiet block does not wake it");
    }

    #[test]
    fn a_held_note_keeps_the_host_asked_for_blocks_and_it_rings_out_after_the_release() {
        // Hold the note for thirty blocks, longer than the quiet, with no event
        // in any of them after the first and nothing heard from the plugin.
        let (mut synth, seen) = instrument(vec![READY]);
        quiet(&seen);
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        assert!(synth.begin(1000, 100, &[on(0, 60)]));
        synth.finish(Instant::now(), &mut l, &mut r);
        for block in 0..30 {
            assert!(
                synth.begin(1000, 100, &[]),
                "block {block}: the note is held"
            );
            synth.finish(Instant::now(), &mut l, &mut r);
            assert!(synth.active(), "block {block}");
        }
        let off = Event {
            offset: 0,
            msg: Msg::note_off(0, 60),
        };
        assert!(synth.begin(1000, 100, &[off]));
        synth.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(
            rung(&mut synth, 100),
            QUIET_BLOCKS - 1,
            "the quiet is counted from the release"
        );
    }

    #[test]
    fn a_held_pedal_and_all_notes_off_are_followed_too() {
        let (mut synth, seen) = instrument(vec![READY]);
        quiet(&seen);
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        let msg = |msg| Event { offset: 0, msg };
        let pedal = msg(Msg::controller(0, cc::SUSTAIN, 127));
        assert!(synth.begin(1000, 100, &[on(0, 60), pedal]));
        synth.finish(Instant::now(), &mut l, &mut r);
        let off = msg(Msg::note_off(0, 60));
        assert!(synth.begin(1000, 100, &[off]));
        synth.finish(Instant::now(), &mut l, &mut r);
        for block in 0..30 {
            assert!(
                synth.begin(1000, 100, &[]),
                "block {block}: the pedal is down"
            );
            synth.finish(Instant::now(), &mut l, &mut r);
        }
        // All notes off lets go of everything the plugin was holding.
        assert!(synth.begin(1000, 100, &[on(0, 62)]));
        synth.finish(Instant::now(), &mut l, &mut r);
        let all_off = msg(Msg::controller(0, cc::ALL_NOTES_OFF, 0));
        assert!(synth.begin(1000, 100, &[all_off]));
        synth.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(
            rung(&mut synth, 100),
            QUIET_BLOCKS - 1,
            "nothing is held after all notes off: the release rings out"
        );
    }

    #[test]
    fn a_failed_host_is_silent_and_not_active_and_a_placeholder_does_nothing() {
        let (mut synth, _) = instrument(vec![Reply::Gone]);
        assert!(synth.begin(48_000, 64, &[on(0, 60)]));
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        synth.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(l, [0.0; 64], "silence");
        assert!(!synth.active(), "a failed host rings no more");
        assert!(
            !synth.begin(48_000, 64, &[on(0, 60)]),
            "and is not asked again"
        );

        let mut placeholder = HostedInstrument::placeholder();
        assert!(!placeholder.hosted());
        assert!(!placeholder.begin(48_000, 64, &[on(0, 60)]));
        placeholder.render(48_000, &[on(0, 60)], &mut l, &mut r);
        assert!(!placeholder.active());
    }

    #[test]
    fn silence_sends_all_sound_off_with_the_next_block() {
        let (mut synth, seen) = instrument(vec![READY]);
        synth.silence();
        assert!(synth.active(), "the message must still reach the host");
        assert!(synth.begin(48_000, 64, &[]));
        assert!(!synth.active(), "and once it has, nothing is left to do");
        let sent = seen.lock().unwrap().events.clone();
        assert_eq!(
            sent,
            vec![vec![Event {
                offset: 0,
                msg: Msg::controller(0, cc::ALL_SOUND_OFF, 0)
            }]]
        );
    }

    #[test]
    fn what_a_busy_host_was_not_given_goes_with_the_next_block() {
        let (mut synth, seen) = instrument(vec![READY]);
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        assert!(synth.begin(48_000, 64, &[on(0, 60)]));
        synth.finish(Instant::now(), &mut l, &mut r);
        // The host is still on a block when the note-off is due.
        seen.lock().unwrap().busy = 1;
        let off = Event {
            offset: 37,
            msg: Msg::note_off(0, 60),
        };
        assert!(!synth.begin(48_000, 64, &[off]), "refused");
        synth.finish(Instant::now(), &mut l, &mut r);
        assert!(synth.active(), "the note-off still has to go");
        assert!(synth.begin(48_000, 64, &[on(5, 64)]));
        let sent = seen.lock().unwrap().events.clone();
        assert_eq!(
            sent.last().unwrap(),
            &vec![
                Event {
                    offset: 0,
                    msg: Msg::note_off(0, 60)
                },
                on(5, 64)
            ],
            "late, at the start of the block, before what is new"
        );
    }

    #[test]
    fn a_storm_past_the_limit_is_dropped_and_counted() {
        let (mut synth, seen) = instrument(vec![READY]);
        let storm: Vec<Event> = (0..EVENTS + 5).map(|i| on(i as u32 % 64, 60)).collect();
        assert!(synth.begin(48_000, 64, &storm));
        assert_eq!(synth.dropped_events(), 5);
        assert_eq!(seen.lock().unwrap().events[0].len(), EVENTS);
    }
}
