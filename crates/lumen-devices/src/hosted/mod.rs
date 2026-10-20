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

mod params;
mod timing;

pub use params::{MOST_POINTS, MOST_QUEUES, Mark, ParamBlock};
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
    /// process (none for an instrument), `events` the MIDI of the block,
    /// offsets within it, and `params` the parameters that change in it
    /// (automation lanes), at their sample. Never blocks.
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> Result<(), Refused>;

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
    /// Parameters whose lanes were cut down or left out of a block for want
    /// of room (M7 slice A3).
    params_cut: AtomicU32,
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
            params_cut: AtomicU32::new(0),
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

    /// How many parameters' lanes were cut down to their ends or left out
    /// of a block, since the host was attached: a plugin with more lanes than
    /// a block can carry.
    pub fn params_cut(&self) -> u32 {
        self.params_cut.load(Ordering::Relaxed)
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

mod guard;
mod instrument;

#[cfg(test)]
mod tests;

pub use guard::Guard;
pub use instrument::HostedInstrument;
