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

pub const FAILED_AFTER_FRAMES: u64 = 48_000;

pub const MOST_FRAMES: usize = 256;

pub const QUIET_LEVEL: f32 = 3.162_278e-5;

pub const QUIET_SECONDS: u32 = 2;

const TAIL_MOST_SECONDS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Busy,
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Ready {
        latency: u32,
        tail: u32,
        restart: bool,
        work_ns: u32,
    },
    Late,
    Gone,
}

pub trait Hosted: Send {
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> Result<(), Refused>;

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply;

    fn ended(&self) -> bool;

    fn output(&mut self, _pair: usize, _left: &mut [f32], _right: &mut [f32]) -> bool {
        false
    }

    fn flush(&mut self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ready,
    Silent,
}

#[derive(Debug)]
pub struct Status {
    failed: AtomicBool,
    late: AtomicU32,
    latency: AtomicU32,
    tail: AtomicU32,
    news: AtomicBool,
    restart: AtomicBool,
    params_cut: AtomicU32,
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

    pub fn take_cost(&self) -> Cost {
        self.costs.take()
    }

    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Release);
    }

    pub fn bypassed(&self) -> bool {
        self.bypass.load(Ordering::Acquire)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn late_blocks(&self) -> u32 {
        self.late.load(Ordering::Relaxed)
    }

    pub fn params_cut(&self) -> u32 {
        self.params_cut.load(Ordering::Relaxed)
    }

    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    pub fn tail(&self) -> u32 {
        self.tail.load(Ordering::Relaxed)
    }

    pub fn take_news(&self) -> Option<u32> {
        self.news
            .swap(false, Ordering::AcqRel)
            .then(|| self.latency())
    }

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
