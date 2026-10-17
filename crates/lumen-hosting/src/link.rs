//! The engine's end of a host: [`Link`] plays a [`BlockLink`] as a
//! [`Hosted`] (M6 slice 2c, ADR-0071). Everything here runs on the audio
//! thread: no allocation, no lock, no system call. The one wait is
//! `BlockLink::wait`, bounded by the deadline the engine gives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use lumen_core::midi::{Event, Kind};
use lumen_devices::{Hosted, Refused, Reply};
use lumen_plugin_host::protocol::{Event as HostEvent, EventKind};
use lumen_plugin_host::shm::{BlockLink, Outcome, Poll, SubmitError};

/// What a link has dropped, readable from the interface thread.
#[derive(Debug, Default)]
pub struct Counts {
    /// Messages the host protocol cannot carry yet (controllers, bend,
    /// pressure, program), or notes past the block's room. Owner's C1.
    dropped_events: AtomicU32,
    /// Blocks the link refused because they did not fit its region.
    unfit_blocks: AtomicU32,
}

impl Counts {
    pub fn dropped_events(&self) -> u32 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    pub fn unfit_blocks(&self) -> u32 {
        self.unfit_blocks.load(Ordering::Relaxed)
    }
}

/// A plugin in a host process, as the engine plays it.
pub struct Link {
    link: BlockLink,
    generation: u32,
    /// The plugin's tail in frames, as reported when it was configured. The
    /// host's answers carry latency but not tail.
    tail: u32,
    /// Output channels the plugin writes (1 or 2).
    outputs: usize,
    ended: Arc<AtomicBool>,
    counts: Arc<Counts>,
    /// The block's events in the host's terms, reused every block.
    events: Vec<HostEvent>,
}

impl Link {
    /// Wraps `link`, configured as `generation`. Allocates: call off the
    /// audio thread.
    pub(crate) fn new(
        link: BlockLink,
        generation: u32,
        tail: u32,
        ended: Arc<AtomicBool>,
        counts: Arc<Counts>,
    ) -> Self {
        let room = link.layout().max_events as usize;
        let outputs = usize::from(link.layout().outputs).clamp(1, 2);
        Self {
            link,
            generation,
            tail,
            outputs,
            ended,
            counts,
            events: Vec::with_capacity(room),
        }
    }

    pub fn counts(&self) -> Arc<Counts> {
        self.counts.clone()
    }
}

/// A MIDI message as the host's note event, if it is a note. Velocity is
/// 0 to 1 in the host's terms.
fn host_event(event: &Event, frames: usize) -> Option<HostEvent> {
    let channel = i16::from(event.msg.channel());
    let kind = match event.msg.kind() {
        Kind::NoteOn { key, velocity } => EventKind::NoteOn {
            channel,
            pitch: i16::from(key),
            velocity: f32::from(velocity) / 127.0,
        },
        Kind::NoteOff { key, velocity } => EventKind::NoteOff {
            channel,
            pitch: i16::from(key),
            velocity: f32::from(velocity) / 127.0,
        },
        _ => return None,
    };
    Some(HostEvent {
        // A message at or past the end of the block would make the whole
        // block invalid: it goes on the last frame.
        offset: event.offset.min(frames.saturating_sub(1) as u32),
        kind,
    })
}

impl Hosted for Link {
    fn submit(&mut self, frames: usize, input: &[&[f32]], events: &[Event]) -> Result<(), Refused> {
        if self.ended.load(Ordering::Relaxed) {
            return Err(Refused::Gone);
        }
        self.events.clear();
        let room = self.events.capacity();
        for event in events {
            match host_event(event, frames) {
                Some(e) if self.events.len() < room => self.events.push(e),
                _ => {
                    self.counts.dropped_events.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        match self
            .link
            .submit(self.generation, frames as u32, input, &self.events, &[])
        {
            Ok(_) => Ok(()),
            Err(SubmitError::Busy) => Err(Refused::Busy),
            Err(SubmitError::Invalid(_)) => {
                self.counts.unfit_blocks.fetch_add(1, Ordering::Relaxed);
                Err(Refused::Busy)
            }
        }
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        match self.link.wait(deadline) {
            Poll::Done(answer) if answer.outcome == Outcome::Done => {
                self.link.copy_output(0, left);
                if self.outputs > 1 {
                    self.link.copy_output(1, right);
                } else {
                    let n = left.len().min(right.len());
                    right[..n].copy_from_slice(&left[..n]);
                }
                Reply::Ready {
                    latency: answer.latency,
                    tail: self.tail,
                    restart: answer.restart != 0,
                }
            }
            // A block the host answered without processing (stale,
            // invalid, failed, not ready) is a miss like a late one.
            _ => {
                if self.ended.load(Ordering::Relaxed) {
                    Reply::Gone
                } else {
                    Reply::Late
                }
            }
        }
    }

    fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }
}
