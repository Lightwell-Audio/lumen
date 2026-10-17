//! The host's audio thread: waits for blocks on the shared-memory link,
//! processes them and publishes the answers. It shares the plugin instance
//! with the control thread through one lock, so a configure or a state call
//! waits for the block in progress and the other way round.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use crate::instance::{Instance, InstanceError};
use crate::shm::sys::audio_thread_priority;
use crate::shm::{Answer, BlockScratch, HostLink, Next, Outcome, Stopper};

/// What the two threads share: the plugin and the generation it was last
/// configured for.
#[derive(Default)]
pub(crate) struct Guarded {
    pub instance: Option<Instance>,
    pub generation: Option<u32>,
}

// SAFETY: an `Instance` holds COM pointers, which are not `Send` by type.
// VST3 does not tie a plugin to a thread (its own host calls the audio
// processor and the controller from different threads); it requires only that
// calls are not made concurrently where the interface forbids it. Every
// access goes through the one mutex in `Shared`, so calls are serialised and
// the instance is never touched by two threads at once.
unsafe impl Send for Guarded {}

pub(crate) type Shared = Arc<Mutex<Guarded>>;

pub(crate) fn lock(shared: &Shared) -> MutexGuard<'_, Guarded> {
    // A poisoned lock means a thread panicked while holding it; panics on
    // either thread end the process, so there is nobody left to observe it.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running block loop and the means to end it.
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
                // A panic here would end only this thread and leave a host
                // that answers its pipe and never its blocks. End the process
                // instead, so Lumen sees a crash and recovers.
                if catch_unwind(AssertUnwindSafe(|| run(link, &shared))).is_err() {
                    std::process::abort();
                }
            })?;
        Ok(Self {
            stopper,
            thread: Some(thread),
        })
    }

    /// Ends the loop and waits for it.
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
                            && n <= f.max_frames
                    });
                    if current != generation {
                        Outcome::Stale
                    } else if !fits {
                        Outcome::Invalid
                    } else {
                        let began = std::time::Instant::now();
                        let outcome = process(instance, n, &mut scratch, &mut answer);
                        let took = began.elapsed();
                        answer.work_ns = u32::try_from(took.as_nanos()).unwrap_or(u32::MAX);
                        timing.note(took, n);
                        outcome
                    }
                }
                _ => Outcome::NotReady,
            };
        }
        link.finish(&answer, frames, &scratch.buffers.output);
    }
}

/// What the plugin's blocks cost, said on stderr for the first few and for
/// the slow ones, so a plugin that is given up on has left a record of why.
#[derive(Default)]
struct Timing {
    blocks: u64,
    said: u32,
}

impl Timing {
    /// Longer than this is worth saying: well over what a small block allows.
    const SLOW: std::time::Duration = std::time::Duration::from_millis(3);
    const MOST_LINES: u32 = 12;

    fn note(&mut self, took: std::time::Duration, frames: u32) {
        let first = self.blocks < 2;
        self.blocks += 1;
        if (first || took >= Self::SLOW) && self.said < Self::MOST_LINES {
            self.said += 1;
            crate::process::log(&format!(
                "block {} ({frames} frames) took {:.1} ms in the plugin",
                self.blocks,
                took.as_secs_f64() * 1000.0
            ));
        }
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
