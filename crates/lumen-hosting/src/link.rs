//! The engine's end of a host: [`Link`] plays a [`BlockLink`] as a
//! [`Hosted`] (M6 slice 2c, ADR-0071). Everything here runs on the audio
//! thread: no allocation, no lock, no system call. The one wait is
//! `BlockLink::wait`, bounded by the deadline the engine gives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use lumen_core::midi::{Event, Kind};
use lumen_devices::{Hosted, Refused, Reply};
use lumen_plugin_host::protocol::{
    Event as HostEvent, EventKind, MAX_CHANNELS, RESTART_RECONFIGURE,
};
use lumen_plugin_host::shm::{BlockLink, Outcome, Poll, SubmitError};

/// Where a bus's channels sit in a block, and how many it has (1 or 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub first: usize,
    pub channels: usize,
}

/// What a link has dropped, readable from the interface thread.
#[derive(Debug, Default)]
pub struct Counts {
    /// Messages the host protocol does not carry (program change, key
    /// pressure, system messages), or events past the block's room.
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
    /// The output buses that are on, main first: where each sits in a block.
    pairs: Vec<Pair>,
    /// Channels a block carries in: the audio in and the sidechain.
    inputs: usize,
    /// Silence for the input channels the engine gave nothing for (a
    /// sidechain nobody feeds), as long as a block can be.
    silence: Vec<f32>,
    ended: Arc<AtomicBool>,
    counts: Arc<Counts>,
    /// The block's events in the host's terms, reused every block.
    events: Vec<HostEvent>,
    /// How many of them a block may hold: the region's room (a `Vec` may
    /// have been given more than it was asked for).
    room: usize,
}

impl Link {
    /// Wraps `link`, configured as `generation`. Allocates: call off the
    /// audio thread.
    pub(crate) fn new(
        link: BlockLink,
        generation: u32,
        tail: u32,
        pairs: Vec<Pair>,
        ended: Arc<AtomicBool>,
        counts: Arc<Counts>,
    ) -> Self {
        let room = link.layout().max_events as usize;
        let inputs = usize::from(link.layout().inputs);
        let silence = vec![0.0; link.layout().max_frames as usize];
        Self {
            link,
            generation,
            tail,
            pairs,
            inputs,
            silence,
            ended,
            counts,
            events: Vec::with_capacity(room),
            room,
        }
    }

    /// The longest block the plugin was configured for, in frames.
    pub fn most_frames(&self) -> u32 {
        self.link.layout().max_frames
    }

    pub fn counts(&self) -> Arc<Counts> {
        self.counts.clone()
    }
}

/// A MIDI message as the host's event, if the host can take it: notes,
/// controllers, pitch bend and channel pressure (a plugin hears these through
/// its own controller mapping) and program changes (through its program
/// parameter). Velocities and values are 0 to 1 in the host's terms; bend
/// has its centre at 0.5. Key pressure and system messages are not carried:
/// the caller counts them.
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
        Kind::Controller { number, value } => EventKind::Controller {
            channel,
            number: i16::from(number),
            value: f32::from(value) / 127.0,
        },
        Kind::Pressure { value } => EventKind::Pressure {
            channel,
            value: f32::from(value) / 127.0,
        },
        Kind::Program { program } => EventKind::Program {
            channel,
            program: i16::from(program),
        },
        Kind::PitchBend { value } => EventKind::PitchBend {
            channel,
            value: ((f32::from(value) + 8192.0) / 16383.0).clamp(0.0, 1.0),
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
        let room = self.room;
        for event in events {
            match host_event(event, frames) {
                Some(e) if self.events.len() < room => self.events.push(e),
                _ => {
                    self.counts.dropped_events.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // The channels the layout carries, the engine's where it gave them
        // and silence where it did not.
        let mut channels: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
        let silence = &self.silence[..frames.min(self.silence.len())];
        for (k, slot) in channels.iter_mut().enumerate().take(self.inputs) {
            *slot = input.get(k).copied().unwrap_or(silence);
        }
        match self.link.submit(
            self.generation,
            frames as u32,
            &channels[..self.inputs],
            &self.events,
            &[],
        ) {
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
                self.copy_pair(0, left, right);
                Reply::Ready {
                    latency: answer.latency,
                    tail: self.tail,
                    // Only a restart that needs the plugin set up again: a
                    // plugin says its parameter values changed whenever it
                    // loads a patch, and setting it up for that once cost a
                    // plugin its host while it was busy loading (MT-VST-4).
                    restart: answer.restart & RESTART_RECONFIGURE != 0,
                    work_ns: answer.work_ns,
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

    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if pair >= self.pairs.len() {
            return false;
        }
        self.copy_pair(pair, left, right);
        true
    }

    fn flush(&mut self) {
        self.link.flush_next();
    }
}

impl Link {
    /// Bus pair `pair` of the block last answered, into `left` and `right`.
    fn copy_pair(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) {
        let Some(&Pair { first, channels }) = self.pairs.get(pair) else {
            return;
        };
        self.link.copy_output(first, left);
        if channels > 1 {
            self.link.copy_output(first + 1, right);
        } else {
            let n = left.len().min(right.len());
            right[..n].copy_from_slice(&left[..n]);
        }
    }

    /// The output buses that are on, main first.
    pub fn pairs(&self) -> &[Pair] {
        &self.pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::midi::Msg;

    fn at(offset: u32, msg: Msg) -> Event {
        Event { offset, msg }
    }

    fn kind_of(event: &Event) -> EventKind {
        host_event(event, 128).expect("the host takes it").kind
    }

    #[test]
    fn notes_controllers_pressure_and_programs_reach_the_host_in_its_terms() {
        assert_eq!(
            kind_of(&at(0, Msg::note_on(3, 60, 127))),
            EventKind::NoteOn {
                channel: 3,
                pitch: 60,
                velocity: 1.0
            }
        );
        assert_eq!(
            kind_of(&at(0, Msg::note_off(15, 61))),
            EventKind::NoteOff {
                channel: 15,
                pitch: 61,
                velocity: 64.0 / 127.0
            }
        );
        assert_eq!(
            kind_of(&at(0, Msg::controller(0, 74, 127))),
            EventKind::Controller {
                channel: 0,
                number: 74,
                value: 1.0
            }
        );
        assert_eq!(
            kind_of(&at(0, Msg::pressure(1, 0))),
            EventKind::Pressure {
                channel: 1,
                value: 0.0
            }
        );
        assert_eq!(
            kind_of(&at(0, Msg::program(2, 9))),
            EventKind::Program {
                channel: 2,
                program: 9
            }
        );
    }

    #[test]
    fn pitch_bend_runs_from_zero_to_one_with_its_centre_in_the_middle() {
        let bend = |v| match kind_of(&at(0, Msg::pitch_bend(0, v))) {
            EventKind::PitchBend { value, .. } => value,
            other => panic!("{other:?}"),
        };
        assert_eq!(bend(-8192), 0.0);
        assert_eq!(bend(8191), 1.0);
        assert!((bend(0) - 0.5).abs() < 1e-3, "{}", bend(0));
        assert!(bend(-1) < bend(0) && bend(0) < bend(1));
    }

    #[test]
    fn messages_the_host_cannot_take_are_left_out() {
        // Key pressure (0xA0) and system messages.
        let key_pressure = Msg([0xA0, 60, 40]);
        assert!(host_event(&at(0, key_pressure), 128).is_none());
        assert!(host_event(&at(0, Msg::CLOCK), 128).is_none());
        assert!(host_event(&at(0, Msg::song_position(8)), 128).is_none());
    }

    #[test]
    fn an_event_at_or_past_the_end_of_the_block_goes_on_its_last_frame() {
        let note = Msg::note_on(0, 60, 100);
        for (offset, frames, want) in [
            (0, 128, 0),
            (127, 128, 127),
            (128, 128, 127),
            (5_000, 128, 127),
            (u32::MAX, 64, 63),
            (3, 1, 0),
            (3, 0, 0),
        ] {
            let e = host_event(&at(offset, note), frames).expect("a note");
            assert_eq!(e.offset, want, "offset {offset} in {frames} frames");
        }
    }
}
