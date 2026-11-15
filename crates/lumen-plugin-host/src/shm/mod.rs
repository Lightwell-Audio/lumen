mod client;
mod host;
mod region;
pub(crate) mod sys;

use std::time::Duration;

pub use client::{BlockLink, Poll};
pub use host::{HostLink, Next, Stopper};
pub use region::{Answer, BadBlock, BlockScratch, Layout, Outcome, Region, SubmitError};
pub use sys::audio_thread_priority;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    Spin,
    Event,
    Hybrid(Duration),
}

impl Waiting {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    pub host: Waiting,
    pub lumen: Waiting,
}

impl Wake {
    pub const EVENT: Wake = Wake {
        host: Waiting::Event,
        lumen: Waiting::Event,
    };
    pub const SPIN: Wake = Wake {
        host: Waiting::Spin,
        lumen: Waiting::Spin,
    };
}

#[cfg(test)]
mod tests;
