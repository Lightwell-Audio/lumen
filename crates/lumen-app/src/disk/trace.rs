//! Test-only progress counters for a cue, sampled without blocking the worker.
//! A snapshot is observational: fields can span concurrent worker updates.
//! Timing starts when the worker consumes an accepted cue; held-start logs
//! separately retain the earlier request/hold interval, including queue delay.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct Trace {
    epoch: Instant,
    consumed_at_nanos: AtomicU64,
    stage: AtomicU64,
    plans: AtomicU64,
    plan_nanos: AtomicU64,
    prepare_nanos: AtomicU64,
    read_nanos: AtomicU64,
    max_read_nanos: AtomicU64,
    reads: AtomicU64,
    idle_waits: AtomicU64,
    retiring_waits: AtomicU64,
    full_waits: AtomicU64,
    last_progress_since_cue_nanos: AtomicU64,
    adopted_after_nanos: AtomicU64,
}

#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub stage: &'static str,
    pub since_cue_consumed_nanos: u64,
    pub plans: u64,
    pub plan_nanos: u64,
    pub prepare_nanos: u64,
    pub read_nanos: u64,
    pub max_read_nanos: u64,
    pub reads: u64,
    pub idle_waits: u64,
    pub retiring_waits: u64,
    pub full_waits: u64,
    pub last_progress_since_cue_nanos: u64,
    pub adopted_after_nanos: u64,
}

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Cue,
    Planning,
    Preparing,
    Reading,
    Idle,
    Retiring,
    Full,
}

fn nanos(elapsed: Duration) -> u64 {
    elapsed.as_nanos().min(u128::from(u64::MAX)) as u64
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            consumed_at_nanos: AtomicU64::default(),
            stage: AtomicU64::default(),
            plans: AtomicU64::default(),
            plan_nanos: AtomicU64::default(),
            prepare_nanos: AtomicU64::default(),
            read_nanos: AtomicU64::default(),
            max_read_nanos: AtomicU64::default(),
            reads: AtomicU64::default(),
            idle_waits: AtomicU64::default(),
            retiring_waits: AtomicU64::default(),
            full_waits: AtomicU64::default(),
            last_progress_since_cue_nanos: AtomicU64::default(),
            adopted_after_nanos: AtomicU64::default(),
        }
    }
}

impl Trace {
    pub(super) fn reset(&self) {
        self.consumed_at_nanos
            .store(nanos(self.epoch.elapsed()), Ordering::Relaxed);
        for value in [
            &self.plans,
            &self.plan_nanos,
            &self.prepare_nanos,
            &self.read_nanos,
            &self.max_read_nanos,
            &self.reads,
            &self.idle_waits,
            &self.retiring_waits,
            &self.full_waits,
            &self.last_progress_since_cue_nanos,
            &self.adopted_after_nanos,
        ] {
            value.store(0, Ordering::Relaxed);
        }
        self.stage(Stage::Cue);
    }

    pub(super) fn stage(&self, stage: Stage) {
        self.stage.store(stage as u64, Ordering::Relaxed);
        let counter = match stage {
            Stage::Idle => Some(&self.idle_waits),
            Stage::Retiring => Some(&self.retiring_waits),
            Stage::Full => Some(&self.full_waits),
            _ => None,
        };
        if let Some(counter) = counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn plan(&self, elapsed: Duration) {
        self.plans.fetch_add(1, Ordering::Relaxed);
        self.plan_nanos.fetch_add(nanos(elapsed), Ordering::Relaxed);
    }

    pub(super) fn prepare(&self, elapsed: Duration) {
        self.prepare_nanos
            .fetch_add(nanos(elapsed), Ordering::Relaxed);
    }

    pub(super) fn read(&self, elapsed: Duration, cue: Option<Instant>) {
        let elapsed = nanos(elapsed);
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.read_nanos.fetch_add(elapsed, Ordering::Relaxed);
        self.max_read_nanos.fetch_max(elapsed, Ordering::Relaxed);
        if let Some(cue) = cue {
            self.last_progress_since_cue_nanos
                .store(nanos(cue.elapsed()), Ordering::Relaxed);
        }
    }

    pub(super) fn adopted(&self, cue: Option<Instant>) {
        if let Some(cue) = cue {
            let _ = self.adopted_after_nanos.compare_exchange(
                0,
                nanos(cue.elapsed()).max(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            since_cue_consumed_nanos: nanos(self.epoch.elapsed())
                .saturating_sub(self.consumed_at_nanos.load(Ordering::Relaxed)),
            stage: match self.stage.load(Ordering::Relaxed) {
                0 => "cue",
                1 => "planning",
                2 => "preparing",
                3 => "reading",
                4 => "idle",
                5 => "retiring",
                6 => "full",
                _ => "unknown",
            },
            plans: self.plans.load(Ordering::Relaxed),
            plan_nanos: self.plan_nanos.load(Ordering::Relaxed),
            prepare_nanos: self.prepare_nanos.load(Ordering::Relaxed),
            read_nanos: self.read_nanos.load(Ordering::Relaxed),
            max_read_nanos: self.max_read_nanos.load(Ordering::Relaxed),
            reads: self.reads.load(Ordering::Relaxed),
            idle_waits: self.idle_waits.load(Ordering::Relaxed),
            retiring_waits: self.retiring_waits.load(Ordering::Relaxed),
            full_waits: self.full_waits.load(Ordering::Relaxed),
            last_progress_since_cue_nanos: self
                .last_progress_since_cue_nanos
                .load(Ordering::Relaxed),
            adopted_after_nanos: self.adopted_after_nanos.load(Ordering::Relaxed),
        }
    }
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CueProgress")
            .field("stage", &self.stage)
            .field("since_cue_consumed_nanos", &self.since_cue_consumed_nanos)
            .field("plans", &self.plans)
            .field("plan_nanos", &self.plan_nanos)
            .field("prepare_nanos", &self.prepare_nanos)
            .field("read_nanos", &self.read_nanos)
            .field("max_read_nanos", &self.max_read_nanos)
            .field("reads", &self.reads)
            .field("idle_waits", &self.idle_waits)
            .field("retiring_waits", &self.retiring_waits)
            .field("full_waits", &self.full_waits)
            .field(
                "last_progress_since_cue_consumed_nanos",
                &self.last_progress_since_cue_nanos,
            )
            .field("consume_to_adopt_nanos", &self.adopted_after_nanos)
            .finish()
    }
}
