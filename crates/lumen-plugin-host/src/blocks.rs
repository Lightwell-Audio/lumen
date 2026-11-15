use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use crate::instance::{Instance, InstanceError};
use crate::shm::sys::audio_thread_priority;
use crate::shm::{Answer, BlockScratch, HostLink, Next, Outcome, Stopper};

#[derive(Default)]
pub(crate) struct Guarded {
    pub instance: Option<Instance>,
    pub generation: Option<u32>,
}

unsafe impl Send for Guarded {}

pub(crate) type Shared = Arc<Mutex<Guarded>>;

pub(crate) fn lock(shared: &Shared) -> MutexGuard<'_, Guarded> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct Blocks {
    stopper: Stopper,
    thread: Option<JoinHandle<()>>,
}

impl Blocks {
    pub fn start(link: HostLink, stopper: Stopper, shared: Shared) -> std::io::Result<Self> {
        let thread = std::thread::Builder::new()
            .name("host-audio".into())
            .spawn(move || {
                #[cfg(windows)]
                crate::process::init_com(false);
                if catch_unwind(AssertUnwindSafe(|| run(link, &shared))).is_err() {
                    std::process::abort();
                }
            })?;
        Ok(Self {
            stopper,
            thread: Some(thread),
        })
    }

    pub fn stop(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        self.stopper.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Blocks {
    fn drop(&mut self) {
        self.finish();
    }
}

fn run(mut link: HostLink, shared: &Shared) {
    let layout = *link.region().layout();
    let mut scratch = BlockScratch::new(&layout);
    audio_thread_priority();
    crate::process::flush_denormals();
    link.mark_ready();
    let mut timing = Timing::default();
    while let Next::Block(seq) = link.wait_for_block() {
        let mut said = None;
        let mut answer = Answer {
            seq,
            generation: 0,
            outcome: Outcome::Invalid,
            status: 0,
            latency: 0,
            restart: 0,
            repaired: 0,
            work_ns: 0,
        };
        let mut frames = 0;
        if let Ok((generation, n)) = link.region().read_request(&mut scratch) {
            answer.generation = generation;
            frames = n;
            let mut guard = lock(shared);
            let current = guard.generation;
            answer.outcome = match (&mut guard.instance, current) {
                (Some(instance), Some(current)) => {
                    let fits = instance.format().is_some_and(|f| {
                        f.inputs == layout.inputs
                            && f.outputs == layout.outputs
                            && n <= layout.max_frames
                    });
                    if current != generation {
                        Outcome::Stale
                    } else if !fits {
                        Outcome::Invalid
                    } else {
                        let began = std::time::Instant::now();
                        let outcome = if scratch.flush && instance.flush().is_err() {
                            Outcome::Failed
                        } else {
                            process(instance, n, &mut scratch, &mut answer)
                        };
                        let took = began.elapsed();
                        answer.work_ns = u32::try_from(took.as_nanos()).unwrap_or(u32::MAX);
                        said = timing.note(took, n);
                        outcome
                    }
                }
                _ => Outcome::NotReady,
            };
        }
        link.finish(&answer, frames, &scratch.buffers.output);
        if let Some(said) = said {
            said.say();
        }
    }
}

#[derive(Default)]
struct Timing {
    blocks: u64,
    said: u32,
}

impl Timing {
    const SLOW: std::time::Duration = std::time::Duration::from_millis(3);
    const MOST_LINES: u32 = 12;

    fn note(&mut self, took: std::time::Duration, frames: u32) -> Option<Said> {
        let first = self.blocks < 2;
        self.blocks += 1;
        if (first || took >= Self::SLOW) && self.said < Self::MOST_LINES {
            self.said += 1;
            return Some(Said {
                block: self.blocks,
                frames,
                took,
            });
        }
        None
    }
}

#[derive(Debug, PartialEq)]
struct Said {
    block: u64,
    frames: u32,
    took: std::time::Duration,
}

impl Said {
    fn say(self) {
        crate::process::log(&format!(
            "block {} ({} frames) took {:.1} ms in the plugin",
            self.block,
            self.frames,
            self.took.as_secs_f64() * 1000.0
        ));
    }
}

fn process(
    instance: &mut Instance,
    frames: u32,
    scratch: &mut BlockScratch,
    answer: &mut Answer,
) -> Outcome {
    let (buffers, events, params) = scratch.parts();
    match instance.process_into(frames, buffers, events, params) {
        Ok(done) => {
            answer.status = done.status;
            answer.latency = done.latency;
            answer.restart = done.restart;
            answer.repaired = done.repaired;
            Outcome::Done
        }
        Err(InstanceError::Sequence(_)) => Outcome::NotReady,
        Err(_) => Outcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_first_two_blocks_and_the_slow_ones_are_worth_a_line_and_no_more_than_twelve() {
        let mut timing = Timing::default();
        let fast = Duration::from_micros(100);
        assert!(timing.note(fast, 64).is_some(), "the first");
        assert!(timing.note(fast, 64).is_some(), "the second");
        assert_eq!(timing.note(fast, 64), None, "a fast block after them");
        let slow = timing
            .note(Duration::from_millis(4), 64)
            .expect("a slow one");
        assert_eq!(slow.block, 4);
        let mut said = 3;
        for _ in 0..100 {
            said += u32::from(timing.note(Duration::from_millis(4), 64).is_some());
        }
        assert_eq!(said, Timing::MOST_LINES, "the lines are capped");
    }
}
