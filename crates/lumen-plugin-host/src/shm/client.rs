use std::io;
use std::time::Instant;

use super::region::{Answer, FLAG_FLUSH, Layout, Outcome, Region, Submission, SubmitError};
use super::sys::{Names, Signal};
use super::{Waiting, Wake};
use crate::protocol::{Event, ParamQueues};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    Idle,
    Pending,
    Done(Answer),
}

struct InFlight {
    seq: u64,
    generation: u32,
    frames: u32,
}

pub struct BlockLink {
    region: Region,
    go: Signal,
    done: Signal,
    wake: Wake,
    next_seq: u64,
    in_flight: Option<InFlight>,
    answered_frames: u32,
    stale: u32,
    invalid: u32,
    flush_next: bool,
}

impl BlockLink {
    pub fn create(base: &str, layout: Layout, wake: Wake) -> io::Result<(Self, Names)> {
        let names = Names::new(base);
        let region = Region::create(&names.region, layout)?;
        let go = Signal::create(&names.go)?;
        let done = Signal::create(&names.done)?;
        let link = Self {
            region,
            go,
            done,
            wake,
            next_seq: 1,
            in_flight: None,
            answered_frames: 0,
            stale: 0,
            invalid: 0,
            flush_next: false,
        };
        Ok((link, names))
    }

    pub fn layout(&self) -> &Layout {
        self.region.layout()
    }

    pub fn wake(&self) -> Wake {
        self.wake
    }

    pub fn host_ready(&self) -> bool {
        self.region.ready()
    }

    pub fn stale(&self) -> u32 {
        self.stale
    }

    pub fn invalid(&self) -> u32 {
        self.invalid
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn flush_next(&mut self) {
        self.flush_next = true;
    }

    pub fn submit<Q: ParamQueues + ?Sized>(
        &mut self,
        generation: u32,
        frames: u32,
        input: &[&[f32]],
        events: &[Event],
        params: &Q,
    ) -> Result<u64, SubmitError> {
        if let Some(flight) = &self.in_flight {
            if self.region.done() == flight.seq {
                self.stale += 1;
                self.in_flight = None;
            } else {
                return Err(SubmitError::Busy);
            }
        }
        let seq = self.next_seq;
        let flags = if self.flush_next { FLAG_FLUSH } else { 0 };
        self.region.write_flagged(Submission {
            seq,
            generation,
            frames,
            input,
            events,
            params,
            flags,
        })?;
        self.flush_next = false;
        self.next_seq += 1;
        self.in_flight = Some(InFlight {
            seq,
            generation,
            frames,
        });
        if self.wake.host.needs_signal() {
            self.go.set();
        }
        Ok(seq)
    }

    pub fn poll(&mut self) -> Poll {
        let Some(flight) = &self.in_flight else {
            return Poll::Idle;
        };
        if self.region.done() != flight.seq {
            return Poll::Pending;
        }
        let (seq, generation, frames) = (flight.seq, flight.generation, flight.frames);
        self.in_flight = None;
        let answer = match self.region.answer() {
            Some(a) if a.seq == seq && a.generation == generation => a,
            _ => {
                self.invalid += 1;
                Answer {
                    seq,
                    generation,
                    outcome: Outcome::Invalid,
                    status: 0,
                    latency: 0,
                    restart: 0,
                    repaired: 0,
                    work_ns: 0,
                }
            }
        };
        if answer.outcome == Outcome::Stale {
            self.stale += 1;
        }
        self.answered_frames = frames;
        Poll::Done(answer)
    }

    pub fn wait(&mut self, deadline: Instant) -> Poll {
        let spin_until = match self.wake.lumen {
            Waiting::Spin => deadline,
            Waiting::Event => Instant::now(),
            Waiting::Hybrid(spin) => (Instant::now() + spin).min(deadline),
        };
        loop {
            let poll = self.poll();
            if poll != Poll::Pending {
                return poll;
            }
            if Instant::now() >= spin_until {
                break;
            }
            std::hint::spin_loop();
        }
        if self.wake.lumen == Waiting::Spin {
            return Poll::Pending;
        }
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return self.poll();
            }
            self.done.wait(left);
            let poll = self.poll();
            if poll != Poll::Pending {
                return poll;
            }
        }
    }

    pub fn copy_output(&self, channel: usize, dst: &mut [f32]) -> u32 {
        self.region.copy_output(channel, self.answered_frames, dst)
    }
}
