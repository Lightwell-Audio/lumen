//! Live MIDI into the engine (M5 slice 2, ADR-0046): messages from the
//! MIDI inputs and the computer keyboard, each placed on the device frame
//! at which it is heard.
//!
//! The owner's J ("steady"): a message is heard one block after it was
//! played, so a phrase keeps its spacing whatever the block boundaries. The
//! time it was played comes from the driver's stamp (stamps keep the
//! spacing; each input's offset from the clock is the smallest
//! `arrived - stamp` seen lately, which drifts up slowly so a driver clock
//! that runs slow is followed). Frames come from the audio callback's
//! [`StreamClock`]: the earliest `time - frame` lately seen is frame 0's
//! time, as for MIDI out. A message is never placed earlier than the part
//! being rendered, nor more than two blocks after the block that takes it,
//! so a clock that is off can only make it sound sooner.
//!
//! [`LiveFeed`] runs on the audio thread, inside the engine's MIDI player:
//! nothing here allocates once it is made. The computer keyboard sends
//! through [`LiveKeys`] from the UI thread.

use std::sync::Arc;

use lumen_core::midi::{Kind, Msg};
use lumen_engine::{KEYBOARD, LiveIn, LiveInput};
use lumen_midi::Timed;

use crate::midi_out::{StreamClock, frame_ns};

/// How many recent blocks frame 0's time is taken over.
const WINDOW: usize = 256;
/// Messages waiting for their frame; more are played at once.
const PENDING: usize = 512;
/// A message stamped this much before it arrived is not believed: the
/// input's offset starts again from this message.
const DOUBT_NS: i64 = 20_000_000;
/// A note-on this late is dropped (the audio stood still); its note-off
/// still goes, so nothing hangs.
const STALE_NS: i64 = 100_000_000;
/// Messages queued from the computer keyboard.
const KEYS_QUEUE: usize = 256;

/// One input's offset from its driver stamps to the clock.
#[derive(Debug, Clone, Copy, Default)]
struct StampMap {
    offset: Option<i64>,
}

impl StampMap {
    /// The clock time at which a message stamped `stamp_ns` that arrived at
    /// `arrived_ns` was played.
    fn played(&mut self, stamp_ns: i64, arrived_ns: i64) -> i64 {
        let gap = arrived_ns - stamp_ns;
        let offset = match self.offset {
            Some(o) if gap >= o && gap - o <= DOUBT_NS => o + (gap - o) / 256,
            _ => gap,
        };
        self.offset = Some(offset);
        (stamp_ns + offset).min(arrived_ns)
    }
}

/// Frame 0's time: the earliest `time - frame` over recent blocks.
struct FrameMap {
    offsets: [i64; WINDOW],
    count: usize,
    next: usize,
    last: Option<(u64, u32)>,
    base: i64,
}

impl FrameMap {
    fn new() -> Self {
        Self {
            offsets: [0; WINDOW],
            count: 0,
            next: 0,
            last: None,
            base: 0,
        }
    }

    fn observe(&mut self, frame: u64, nanos: i64, rate: u32) {
        if self.last == Some((frame, rate)) {
            return;
        }
        if self.last.is_some_and(|(_, r)| r != rate) {
            self.count = 0;
            self.next = 0;
        }
        self.last = Some((frame, rate));
        self.offsets[self.next] = nanos - frame_ns(frame, rate);
        self.next = (self.next + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
        self.base = self.offsets[..self.count]
            .iter()
            .copied()
            .min()
            .unwrap_or(0);
    }

    /// The frame heard at clock time `nanos`, or `None` before any block.
    fn frame_at(&self, nanos: i64, rate: u32) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let since = (nanos - self.base).max(0) as u128;
        Some(((since * u128::from(rate) + 500_000_000) / 1_000_000_000) as u64)
    }
}

/// A message from the computer keyboard, at clock time `nanos`.
#[derive(Debug, Clone, Copy)]
struct Keyed {
    nanos: i64,
    msg: Msg,
}

/// The UI's side of the computer keyboard: sends notes now.
pub struct LiveKeys {
    tx: rtrb::Producer<Keyed>,
    clock: Arc<StreamClock>,
    /// Messages sent so far: the activity light compares it frame to frame.
    sent: u32,
}

impl LiveKeys {
    /// Sends `msg` as played now. False if the queue is full.
    pub fn send(&mut self, msg: Msg) -> bool {
        let nanos = self.clock.now();
        let sent = self.tx.push(Keyed { nanos, msg }).is_ok();
        if sent {
            self.sent = self.sent.wrapping_add(1);
        }
        sent
    }

    /// How many messages have been sent (for the activity light).
    pub fn activity(&self) -> u32 {
        self.sent
    }
}

/// What the block taking messages looks like.
#[derive(Clone, Copy)]
struct Block {
    /// The callback's first frame.
    start: u64,
    frames: u64,
    rate: u32,
}

/// The audio side: hands the engine each part's live messages. A
/// [`LiveInput`].
pub struct LiveFeed {
    input: Option<rtrb::Consumer<Timed>>,
    keys: rtrb::Consumer<Keyed>,
    clock: Arc<StreamClock>,
    frames: FrameMap,
    stamps: [StampMap; KEYBOARD as usize],
    /// Sorted by frame; `len` used.
    pending: [LiveIn; PENDING],
    len: usize,
}

/// The pair: the feed for the engine, and the keyboard's sender.
pub fn live(input: Option<rtrb::Consumer<Timed>>, clock: Arc<StreamClock>) -> (LiveFeed, LiveKeys) {
    let (tx, keys) = rtrb::RingBuffer::new(KEYS_QUEUE);
    let empty = LiveIn {
        frame: 0,
        source: 0,
        msg: Msg::default(),
    };
    (
        LiveFeed {
            input,
            keys,
            clock: Arc::clone(&clock),
            frames: FrameMap::new(),
            stamps: [StampMap::default(); KEYBOARD as usize],
            pending: [empty; PENDING],
            len: 0,
        },
        LiveKeys { tx, clock, sent: 0 },
    )
}

impl LiveFeed {
    /// The frame at which a message played at `nanos` is heard.
    fn place(&self, nanos: i64, block: Option<Block>, start: u64) -> u64 {
        let Some(block) = block else {
            return start;
        };
        match self.frames.frame_at(nanos, block.rate) {
            Some(frame) => (frame + block.frames)
                .min(block.start + 2 * block.frames)
                .max(start),
            None => start,
        }
    }

    /// Whether a message played at `nanos` is too late to start a note.
    fn stale(&self, nanos: i64, msg: Msg) -> bool {
        matches!(msg.kind(), Kind::NoteOn { .. }) && self.clock.now() - nanos > STALE_NS
    }

    /// Queues `message` in frame order, or gives it at once if the queue
    /// is full.
    fn queue(&mut self, message: LiveIn, take: &mut dyn FnMut(LiveIn)) {
        if self.len == PENDING {
            take(message);
            return;
        }
        let mut i = self.len;
        while i > 0 && self.pending[i - 1].frame > message.frame {
            self.pending[i] = self.pending[i - 1];
            i -= 1;
        }
        self.pending[i] = message;
        self.len += 1;
    }
}

impl LiveInput for LiveFeed {
    fn pull(&mut self, start: u64, end: u64, take: &mut dyn FnMut(LiveIn)) {
        let block = self.clock.read_block().map(|(frame, nanos, rate, frames)| {
            self.frames.observe(frame, nanos, rate);
            Block {
                start: frame,
                frames: u64::from(frames),
                rate,
            }
        });
        if let Some(mut input) = self.input.take() {
            while let Ok(timed) = input.pop() {
                let Some(source) = u8::try_from(timed.slot).ok().filter(|s| *s < KEYBOARD) else {
                    continue;
                };
                let msg = Msg(timed.message.bytes);
                let arrived = self.clock.nanos_of(timed.arrived);
                let stamp = i64::try_from(timed.stamp_us)
                    .unwrap_or(i64::MAX / 1_000)
                    .saturating_mul(1_000);
                let played = self.stamps[usize::from(source)].played(stamp, arrived);
                if self.stale(played, msg) {
                    continue;
                }
                let frame = self.place(played, block, start);
                self.queue(LiveIn { frame, source, msg }, take);
            }
            self.input = Some(input);
        }
        while let Ok(keyed) = self.keys.pop() {
            if self.stale(keyed.nanos, keyed.msg) {
                continue;
            }
            let frame = self.place(keyed.nanos, block, start);
            self.queue(
                LiveIn {
                    frame,
                    source: KEYBOARD,
                    msg: keyed.msg,
                },
                take,
            );
        }
        let ready = self.pending[..self.len]
            .iter()
            .take_while(|m| m.frame < end)
            .count();
        for message in &self.pending[..ready] {
            take(*message);
        }
        self.pending.copy_within(ready..self.len, 0);
        self.len -= ready;
    }

    fn waiting(&self) -> bool {
        self.len > 0 || !self.keys.is_empty() || self.input.as_ref().is_some_and(|i| !i.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_midi::Short;

    const MS: i64 = 1_000_000;

    fn timed(clock: &StreamClock, slot: u16, stamp_us: u64, arrived_ns: i64, msg: Msg) -> Timed {
        Timed {
            slot,
            stamp_us,
            arrived: clock.instant_at(arrived_ns),
            message: Short::from_bytes(msg.bytes()).unwrap(),
        }
    }

    /// Runs one 128-frame block at 48 kHz that started at clock time
    /// `nanos`, returning what it played.
    fn block(feed: &mut LiveFeed, clock: &StreamClock, index: u64, nanos: i64) -> Vec<LiveIn> {
        clock.publish_at(index * 128, 128, 48_000, nanos);
        let mut got = Vec::new();
        feed.pull(index * 128, index * 128 + 128, &mut |m| got.push(m));
        got
    }

    /// Nanoseconds of frame `frame` at 48 kHz.
    fn at(frame: i64) -> i64 {
        frame * 1_000_000_000 / 48_000
    }

    #[test]
    fn played_notes_keep_their_spacing_one_block_later_whatever_the_blocks() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(64);
        let (mut feed, _keys) = live(Some(rx), Arc::clone(&clock));
        // Blocks every 128 frames, each callback up to 0.9 ms late.
        let late = [
            0, 900_000, 300_000, 0, 700_000, 100_000, 0, 0, 500_000, 0, 0, 0,
        ];
        // Notes played at frames 100, 333, 610, 1000 (driver stamps from
        // an origin 5 s before the clock's).
        let played = [100_i64, 333, 610, 1_000];
        let mut heard = Vec::new();
        let callback = |i: usize| at(i as i64 * 128) + late[i];
        for i in 0..late.len() {
            for (n, &frame) in played.iter().enumerate() {
                // It arrives up to 0.6 ms after it was played, and is taken
                // by the first callback after that.
                let arrives = at(frame) + (n as i64 % 3) * 300_000;
                let after_previous = i == 0 || arrives > callback(i - 1);
                if after_previous && arrives <= callback(i) {
                    let stamp = (at(frame) + 5_000 * MS) / 1_000;
                    tx.push(timed(
                        &clock,
                        2,
                        stamp as u64,
                        arrives,
                        Msg::note_on(0, 60, 9),
                    ))
                    .unwrap();
                }
            }
            heard.extend(block(&mut feed, &clock, i as u64, callback(i)));
        }
        let frames: Vec<u64> = heard.iter().map(|m| m.frame).collect();
        assert_eq!(frames, vec![228, 461, 738, 1_128]);
        assert!(heard.iter().all(|m| m.source == 2));
    }

    #[test]
    fn a_message_is_never_placed_before_the_part_nor_far_after_its_block() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        // Frame 0 was at 0 ns; the next callback is 37 ms late, and takes a
        // note played at 39 ms (frame 1,872).
        clock.publish_at(128, 128, 48_000, 40 * MS);
        tx.push(timed(&clock, 1, 39_000, 39 * MS, Msg::note_on(0, 60, 100)))
            .unwrap();
        let mut got = Vec::new();
        feed.pull(128, 256, &mut |m| got.push(m));
        // Placed at most two blocks on from the block taking it: here it
        // waits, then plays in the next block at frame 384.
        assert!(got.is_empty());
        let next = block(&mut feed, &clock, 3, 40 * MS + 1);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].frame, 384);
        assert_eq!(next[0].source, 1);
    }

    #[test]
    fn after_the_audio_stood_still_note_ons_are_dropped_and_note_offs_kept() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys) = live(Some(rx), Arc::clone(&clock));
        let then = clock.now();
        tx.push(timed(&clock, 0, 0, then, Msg::note_on(0, 60, 100)))
            .unwrap();
        tx.push(timed(&clock, 0, 1, then, Msg::note_off(0, 60)))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let mut got = block(&mut feed, &clock, 0, clock.now());
        got.extend(block(&mut feed, &clock, 1, clock.now()));
        got.extend(block(&mut feed, &clock, 2, clock.now()));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].msg, Msg::note_off(0, 60));
    }

    #[test]
    fn driver_stamps_that_do_not_move_fall_back_to_arrival() {
        let mut map = StampMap::default();
        assert_eq!(map.played(0, 10 * MS), 10 * MS);
        // The same stamp 50 ms later: not believed, so it is when it came.
        assert_eq!(map.played(0, 60 * MS), 60 * MS);
        // A stamp that moves with the clock keeps its spacing.
        // (The offset creeps up by 1/256 of a later gap: a few µs here.)
        let played = map.played(5 * MS, 66 * MS);
        assert!((played - 65 * MS).abs() < MS / 100, "{played}");
    }

    #[test]
    fn inputs_past_the_last_source_are_ignored() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys) = live(Some(rx), Arc::clone(&clock));
        let now = clock.now();
        tx.push(timed(&clock, 200, 0, now, Msg::note_on(0, 60, 100)))
            .unwrap();
        assert!(block(&mut feed, &clock, 0, now).is_empty());
    }
}
