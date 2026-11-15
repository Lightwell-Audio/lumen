use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Default)]
pub(super) struct Costs {
    waits: AtomicU32,
    wait_sum: AtomicU64,
    wait_peak: AtomicU32,
    answers: AtomicU32,
    work_sum: AtomicU64,
    work_peak: AtomicU32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub blocks: u32,
    pub answers: u32,
    pub work_mean: Duration,
    pub work_peak: Duration,
    pub wait_mean: Duration,
    pub wait_peak: Duration,
}

fn nanos(d: Duration) -> u32 {
    u32::try_from(d.as_nanos()).unwrap_or(u32::MAX)
}

fn mean(sum: u64, count: u32) -> Duration {
    Duration::from_nanos(sum.checked_div(u64::from(count)).unwrap_or(0))
}

impl Costs {
    pub(super) fn note(&self, work_ns: Option<u32>, waited: Duration) {
        let waited = nanos(waited);
        self.waits.fetch_add(1, Ordering::Relaxed);
        self.wait_sum
            .fetch_add(u64::from(waited), Ordering::Relaxed);
        self.wait_peak.fetch_max(waited, Ordering::Relaxed);
        if let Some(work) = work_ns {
            self.answers.fetch_add(1, Ordering::Relaxed);
            self.work_sum.fetch_add(u64::from(work), Ordering::Relaxed);
            self.work_peak.fetch_max(work, Ordering::Relaxed);
        }
    }

    pub(super) fn take(&self) -> Cost {
        let waits = self.waits.swap(0, Ordering::Relaxed);
        let wait_sum = self.wait_sum.swap(0, Ordering::Relaxed);
        let wait_peak = self.wait_peak.swap(0, Ordering::Relaxed);
        let answers = self.answers.swap(0, Ordering::Relaxed);
        let work_sum = self.work_sum.swap(0, Ordering::Relaxed);
        let work_peak = self.work_peak.swap(0, Ordering::Relaxed);
        Cost {
            blocks: waits,
            answers,
            work_mean: mean(work_sum, answers),
            work_peak: Duration::from_nanos(u64::from(work_peak)),
            wait_mean: mean(wait_sum, waits),
            wait_peak: Duration::from_nanos(u64::from(wait_peak)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_is_averaged_over_answers_and_waits_over_every_block() {
        let costs = Costs::default();
        let us = Duration::from_micros;
        costs.note(Some(100_000), us(150));
        costs.note(Some(300_000), us(350));
        costs.note(None, us(2_000));
        assert_eq!(
            costs.take(),
            Cost {
                blocks: 3,
                answers: 2,
                work_mean: us(200),
                work_peak: us(300),
                wait_mean: us(2_500) / 3,
                wait_peak: us(2_000),
            }
        );
        assert_eq!(costs.take(), Cost::default(), "taken");
    }
}
