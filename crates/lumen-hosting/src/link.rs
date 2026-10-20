//! The engine's end of a host: [`Link`] plays a [`BlockLink`] as a
//! [`Hosted`] (M6 slice 2c, ADR-0071). Everything here runs on the audio
//! thread: no allocation, no lock, no system call. The one wait is
//! `BlockLink::wait`, bounded by the deadline the engine gives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use lumen_core::midi::{Event, Kind};
use lumen_devices::{Hosted, ParamBlock, Refused, Reply};
use lumen_plugin_host::protocol::{
    Event as HostEvent, EventKind, MAX_CHANNELS, ParamQueues, RESTART_RECONFIGURE,
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
    /// Parameters whose automation for a block was left out because the
    /// region had no room for it (M7 slice A3).
    dropped_params: AtomicU32,
}

impl Counts {
    pub fn dropped_events(&self) -> u32 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    pub fn unfit_blocks(&self) -> u32 {
        self.unfit_blocks.load(Ordering::Relaxed)
    }

    pub fn dropped_params(&self) -> u32 {
        self.dropped_params.load(Ordering::Relaxed)
    }
}

/// The first `queues` parameters of a block, as the region takes them.
struct Fit<'a> {
    block: &'a ParamBlock,
    queues: usize,
}

impl<'a> Fit<'a> {
    /// The whole queues of `block` that the region has room for: a parameter
    /// is sent whole or not at all, and how many were left out.
    fn of(block: &'a ParamBlock, max_queues: usize, max_points: usize) -> (Self, usize) {
        let mut points = 0;
        let mut queues = 0;
        while queues < block.len().min(max_queues) {
            let here = block.queue(queues).1.len();
            if points + here > max_points {
                break;
            }
            points += here;
            queues += 1;
        }
        (Self { block, queues }, block.len() - queues)
    }
}

impl ParamQueues for Fit<'_> {
    fn queue_count(&self) -> usize {
        self.queues
    }

    fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        self.block.queue(i)
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
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> Result<(), Refused> {
        if self.ended.load(Ordering::Relaxed) {
            return Err(Refused::Gone);
        }
        let layout = self.link.layout();
        let (fit, left_out) = Fit::of(
            params,
            layout.max_queues as usize,
            layout.max_points as usize,
        );
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
            &fit,
        ) {
            Ok(_) => {
                if left_out > 0 {
                    self.counts
                        .dropped_params
                        .fetch_add(left_out as u32, Ordering::Relaxed);
                }
                Ok(())
            }
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

    fn block(queues: &[usize]) -> ParamBlock {
        let mut block = ParamBlock::new();
        block.start(64);
        for (id, &points) in queues.iter().enumerate() {
            assert!(block.open(id as u32));
            for at in 0..points as u32 {
                assert!(block.point(at, 0.5));
            }
        }
        block
    }

    #[test]
    fn a_block_goes_whole_when_the_region_has_room_for_it() {
        let b = block(&[3, 5, 2]);
        let (fit, left_out) = Fit::of(&b, 64, 1024);
        assert_eq!((fit.queue_count(), left_out), (3, 0));
        assert_eq!(fit.queue(1).1.len(), 5);
    }

    #[test]
    fn a_parameter_is_left_out_whole_or_not_at_all() {
        let b = block(&[3, 5, 2, 4]);
        // Room for two queues: the first two go.
        let (fit, left_out) = Fit::of(&b, 2, 1024);
        assert_eq!((fit.queue_count(), left_out), (2, 2));
        // Room for nine points: 3 + 5 fit; the next would make ten, so the
        // rest wait, and a later small queue is not let past it.
        let (fit, left_out) = Fit::of(&b, 64, 9);
        assert_eq!((fit.queue_count(), left_out), (2, 2));
    }

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
