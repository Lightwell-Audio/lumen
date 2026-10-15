//! The audio path between Lumen and a plugin-host process: one block of
//! shared memory per host, a wake-up in each direction, and nothing else.
//!
//! The control path (load, configure, state, parameters) stays on the pipes
//! of [`crate::client::HostProcess`]; only blocks cross here. Lumen's side,
//! [`BlockLink`], never allocates and never takes a lock; whether it may also
//! wait on the operating system, and how the host waits for work, are
//! settings ([`Wake`]) because the measurements in `docs/milestones/M6.md`
//! decide them.
//!
//! One block is in flight per host. While the host is late with a block,
//! Lumen submits nothing further to it, so a late host can never write into
//! a block that has moved on; its answer is recognised by sequence number,
//! dropped and counted.

mod client;
mod host;
mod region;
pub(crate) mod sys;

use std::time::Duration;

pub use client::{BlockLink, Poll};
pub use host::{HostLink, Next, Stopper};
pub use region::{Answer, BadBlock, BlockScratch, Layout, Outcome, Region, SubmitError};
pub use sys::audio_thread_priority;

/// How one side waits for the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    /// Poll memory and never ask the operating system to wait. Lowest
    /// latency, and a core busy while it waits.
    Spin,
    /// Wait on the operating system's signal. No CPU while waiting, at the
    /// price of the scheduler's wake-up time.
    Event,
    /// Poll for this long, then wait on the signal.
    Hybrid(Duration),
}

impl Waiting {
    /// Whether the other side must set a signal for this one.
    pub fn needs_signal(self) -> bool {
        !matches!(self, Waiting::Spin)
    }

    pub(crate) fn to_parts(self) -> (u8, u32) {
        match self {
            Waiting::Spin => (0, 0),
            Waiting::Event => (1, 0),
            Waiting::Hybrid(d) => (2, d.as_micros().min(u128::from(u32::MAX)) as u32),
        }
    }

    pub(crate) fn from_parts(tag: u8, micros: u32) -> Option<Self> {
        Some(match tag {
            0 => Waiting::Spin,
            1 => Waiting::Event,
            2 => Waiting::Hybrid(Duration::from_micros(u64::from(micros))),
            _ => return None,
        })
    }
}

/// The wake-up mechanism of a link: how the host waits for a block, and how
/// Lumen waits for the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    pub host: Waiting,
    pub lumen: Waiting,
}

impl Wake {
    /// Both sides wait on signals.
    pub const EVENT: Wake = Wake {
        host: Waiting::Event,
        lumen: Waiting::Event,
    };
    /// Both sides poll.
    pub const SPIN: Wake = Wake {
        host: Waiting::Spin,
        lumen: Waiting::Spin,
    };
}

#[cfg(test)]
mod tests;
