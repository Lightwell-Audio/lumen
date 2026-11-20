use std::sync::Arc;

use lumen_core::midi::{Kind, Msg};
use lumen_engine::{AUDITION, INPUT_SLOTS, KEYBOARD, LIVE_SOURCES, LiveIn, LiveInput};
use lumen_midi::Timed;

use crate::live_tap::{Heard, Played, Shared, TAP_QUEUE, Tap};
use crate::midi::out::{StreamClock, frame_ns};

const WINDOW: usize = 256;
const PENDING: usize = 512;
const DOUBT_NS: i64 = 20_000_000;
const STALE_NS: i64 = 100_000_000;
const KEYS_QUEUE: usize = 256;

#[derive(Debug, Clone, Copy, Default)]
struct StampMap {
    offset: Option<i64>,
}

impl StampMap {
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

    fn frame_at(&self, nanos: i64, rate: u32) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let since = (nanos - self.base).max(0) as u128;
        Some(((since * u128::from(rate) + 500_000_000) / 1_000_000_000) as u64)
    }
}

#[derive(Debug, Clone, Copy)]
struct Keyed {
    nanos: i64,
    track: Option<u64>,
    raw: bool,
    msg: Msg,
}

pub struct LiveKeys {
    tx: rtrb::Producer<Keyed>,
    clock: Arc<StreamClock>,
    shared: Arc<Shared>,
    sent: u32,
    held: u128,
}

impl LiveKeys {
    pub fn send(&mut self, msg: Msg) -> bool {
        self.push(None, msg)
    }

    pub fn send_to(&mut self, track: u64, msg: Msg) -> bool {
        self.push(Some(track), msg)
    }

    fn push(&mut self, track: Option<u64>, msg: Msg) -> bool {
        let nanos = self.clock.now();
        let raw = track.is_none() && self.shared.raw.load(std::sync::atomic::Ordering::Relaxed);
        let sent = self
            .tx
            .push(Keyed {
                nanos,
                track,
                raw,
                msg,
            })
            .is_ok();
        if sent {
            self.sent = self.sent.wrapping_add(1);
            match msg.kind() {
                Kind::NoteOn { key, .. } => self.held |= 1_u128 << (key & 127),
                Kind::NoteOff { key, .. } => self.held &= !(1_u128 << (key & 127)),
                _ => {}
            }
        }
        sent
    }

    pub fn activity(&self) -> u32 {
        self.sent
    }

    pub fn held(&self) -> bool {
        self.held != 0
    }
}

#[derive(Clone, Copy)]
struct Block {
    start: u64,
    frames: u64,
    rate: u32,
}

pub(crate) struct LiveFeed {
    input: Option<rtrb::Consumer<Timed>>,
    keys: rtrb::Consumer<Keyed>,
    clock: Arc<StreamClock>,
    frames: FrameMap,
    stamps: [StampMap; INPUT_SLOTS as usize],
    floor: [(u64, u64); LIVE_SOURCES as usize],
    pending: [LiveIn; PENDING],
    len: usize,
    tap: rtrb::Producer<Heard>,
    notes: rtrb::Producer<Played>,
    shared: Arc<Shared>,
}

pub(crate) fn live(
    input: Option<rtrb::Consumer<Timed>>,
    clock: Arc<StreamClock>,
) -> (LiveFeed, LiveKeys, Tap) {
    let (tx, keys) = rtrb::RingBuffer::new(KEYS_QUEUE);
    let (tap_tx, tap_rx) = rtrb::RingBuffer::new(TAP_QUEUE);
    let (notes_tx, notes_rx) = rtrb::RingBuffer::new(crate::live_tap::NOTE_QUEUE);
    let shared = Shared::new();
    let empty = LiveIn {
        frame: 0,
        played: 0,
        source: 0,
        track: None,
        raw: false,
        msg: Msg::default(),
    };
    (
        LiveFeed {
            input,
            keys,
            clock: Arc::clone(&clock),
            frames: FrameMap::new(),
            stamps: [StampMap::default(); INPUT_SLOTS as usize],
            floor: [(0, 0); LIVE_SOURCES as usize],
            pending: [empty; PENDING],
            len: 0,
            tap: tap_tx,
            notes: notes_tx,
            shared: Arc::clone(&shared),
        },
        LiveKeys {
            tx,
            clock,
            shared: Arc::clone(&shared),
            sent: 0,
            held: 0,
        },
        Tap {
            rx: tap_rx,
            notes: notes_rx,
            shared,
        },
    )
}

impl LiveFeed {
    fn place(&self, nanos: i64, block: Option<Block>, start: u64) -> (u64, u64) {
        let Some(block) = block else {
            return (start, start);
        };
        match self.frames.frame_at(nanos, block.rate) {
            Some(played) => (
                (played + block.frames)
                    .min(block.start + 2 * block.frames)
                    .max(start),
                played,
            ),
            None => (start, start),
        }
    }

    fn tell(&mut self, source: u8, channel: u8, number: u8, value: u8, played: u64) {
        let heard = Heard {
            source,
            channel,
            number,
            value,
            played,
        };
        if self.tap.push(heard).is_err() {
            self.shared
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn tell_note(&mut self, source: u8, channel: u8, key: u8, velocity: u8) {
        let played = Played {
            source,
            channel,
            key,
            velocity,
        };
        if self.notes.push(played).is_err() {
            self.shared
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn stale(&self, nanos: i64, msg: Msg) -> bool {
        matches!(msg.kind(), Kind::NoteOn { .. }) && self.clock.now() - nanos > STALE_NS
    }

    fn queue(&mut self, mut message: LiveIn, take: &mut dyn FnMut(LiveIn)) {
        if let Some(floor) = self.floor.get_mut(usize::from(message.source)) {
            message.frame = message.frame.max(floor.0);
            message.played = message.played.max(floor.1);
            *floor = (message.frame, message.played);
        }
        if self.len == PENDING {
            for waiting in &self.pending[..self.len] {
                take(*waiting);
            }
            self.len = 0;
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
                let Some(source) = u8::try_from(timed.slot).ok().filter(|s| *s < INPUT_SLOTS)
                else {
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
                let (frame, at) = self.place(played, block, start);
                if let Kind::Controller { number, value } = msg.kind() {
                    self.tell(source, msg.channel(), number, value, at);
                    if self.shared.taken.contains(msg.channel(), number) {
                        continue;
                    }
                }
                if let Kind::NoteOn { key, velocity } = msg.kind() {
                    self.tell_note(source, msg.channel(), key, velocity);
                }
                self.queue(
                    LiveIn {
                        frame,
                        played: at,
                        source,
                        track: None,
                        raw: self.shared.raw.load(std::sync::atomic::Ordering::Relaxed),
                        msg,
                    },
                    take,
                );
            }
            self.input = Some(input);
        }
        while let Ok(keyed) = self.keys.pop() {
            if self.stale(keyed.nanos, keyed.msg) {
                continue;
            }
            let (frame, at) = self.place(keyed.nanos, block, start);
            self.queue(
                LiveIn {
                    frame,
                    played: at,
                    source: if keyed.track.is_some() {
                        AUDITION
                    } else {
                        KEYBOARD
                    },
                    track: keyed.track,
                    raw: keyed.raw,
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

    fn block(feed: &mut LiveFeed, clock: &StreamClock, index: u64, nanos: i64) -> Vec<LiveIn> {
        clock.publish_at(index * 128, 128, 48_000, nanos);
        let mut got = Vec::new();
        feed.pull(index * 128, index * 128 + 128, &mut |m| got.push(m));
        got
    }

    fn at(frame: i64) -> i64 {
        frame * 1_000_000_000 / 48_000
    }

    #[test]
    fn played_notes_keep_their_spacing_one_block_later_whatever_the_blocks() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(64);
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
        let late = [
            0, 900_000, 300_000, 0, 700_000, 100_000, 0, 0, 500_000, 0, 0, 0,
        ];
        let played = [100_i64, 333, 610, 1_000];
        let mut heard = Vec::new();
        let callback = |i: usize| at(i as i64 * 128) + late[i];
        for i in 0..late.len() {
            for (n, &frame) in played.iter().enumerate() {
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
        let played: Vec<u64> = heard.iter().map(|m| m.played).collect();
        assert_eq!(played, vec![100, 333, 610, 1_000]);
        assert!(heard.iter().all(|m| m.source == 2));
    }

    #[test]
    fn a_message_is_never_placed_before_the_part_nor_far_after_its_block() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        clock.publish_at(128, 128, 48_000, 40 * MS);
        tx.push(timed(&clock, 1, 39_000, 39 * MS, Msg::note_on(0, 60, 100)))
            .unwrap();
        let mut got = Vec::new();
        feed.pull(128, 256, &mut |m| got.push(m));
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
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
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
        assert_eq!(map.played(0, 60 * MS), 60 * MS);
        let played = map.played(5 * MS, 66 * MS);
        assert!((played - 65 * MS).abs() < MS / 100, "{played}");
    }

    #[test]
    fn a_sources_messages_keep_their_order_when_the_frame_map_moves() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        clock.publish_at(128, 128, 48_000, 21 * MS);
        tx.push(timed(&clock, 1, 20_000, 20 * MS, Msg::note_off(0, 60)))
            .unwrap();
        let mut got = Vec::new();
        feed.pull(128, 256, &mut |m| got.push(m));
        assert!(got.is_empty());
        clock.publish_at(256, 128, 44_100, 100 * MS);
        tx.push(timed(&clock, 1, 39_000, 39 * MS, Msg::note_on(0, 60, 100)))
            .unwrap();
        feed.pull(256, 384, &mut |m| got.push(m));
        feed.pull(384, 512, &mut |m| got.push(m));
        feed.pull(512, 640, &mut |m| got.push(m));
        let kinds: Vec<Msg> = got.iter().map(|m| m.msg).collect();
        assert_eq!(kinds, vec![Msg::note_off(0, 60), Msg::note_on(0, 60, 100)]);
        assert!(got[0].frame <= got[1].frame);
    }

    #[test]
    fn a_full_queue_gives_what_waits_before_the_newest_message() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(1_024);
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        clock.publish_at(128, 128, 48_000, 11 * MS);
        let sent: Vec<Msg> = (0..=PENDING)
            .map(|i| {
                let key = 40 + (i / 2 % 40) as u8;
                if i % 2 == 0 {
                    Msg::note_on(0, key, 100)
                } else {
                    Msg::note_off(0, key)
                }
            })
            .collect();
        for (i, msg) in sent.iter().enumerate() {
            let at = 10_000 + i as u64;
            tx.push(timed(&clock, 1, at, at as i64 * 1_000, *msg))
                .unwrap();
        }
        let mut got = Vec::new();
        feed.pull(128, 256, &mut |m| got.push(m.msg));
        feed.pull(256, 384, &mut |m| got.push(m.msg));
        feed.pull(384, 512, &mut |m| got.push(m.msg));
        assert_eq!(got, sent, "arrival order, nothing lost or overtaken");
    }

    #[test]
    fn inputs_past_the_last_source_are_ignored() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(8);
        let (mut feed, _keys, _tap) = live(Some(rx), Arc::clone(&clock));
        let now = clock.now();
        tx.push(timed(&clock, 200, 0, now, Msg::note_on(0, 60, 100)))
            .unwrap();
        assert!(block(&mut feed, &clock, 0, now).is_empty());
    }

    fn heard_by_the_tap(tap: &mut Tap) -> Vec<Heard> {
        let mut got = Vec::new();
        tap.drain(|h| got.push(h));
        got
    }

    #[test]
    fn a_controller_is_told_to_the_interface_at_the_frame_it_was_played_and_still_plays() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let (mut feed, _keys, mut tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        let stamp = (at(200) + 5_000 * MS) / 1_000;
        let arrives = at(200);
        tx.push(timed(
            &clock,
            2,
            stamp as u64,
            arrives,
            Msg::controller(2, 7, 99),
        ))
        .unwrap();
        tx.push(timed(
            &clock,
            2,
            stamp as u64 + 1,
            arrives,
            Msg::note_on(0, 60, 9),
        ))
        .unwrap();
        let mut played = block(&mut feed, &clock, 1, at(256));
        played.extend(block(&mut feed, &clock, 2, at(384)));
        let heard = heard_by_the_tap(&mut tap);
        assert_eq!(
            heard,
            vec![Heard {
                source: 2,
                channel: 2,
                number: 7,
                value: 99,
                played: 200,
            }],
            "only the controller is told"
        );
        let kinds: Vec<_> = played.iter().map(|m| m.msg).collect();
        assert!(kinds.contains(&Msg::controller(2, 7, 99)), "{kinds:?}");
        assert!(kinds.contains(&Msg::note_on(0, 60, 9)), "{kinds:?}");
    }

    #[test]
    fn a_controller_a_mapping_takes_is_told_but_not_given_to_the_engine() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let (mut feed, _keys, mut tap) = live(Some(rx), Arc::clone(&clock));
        tap.take([(2, 7)]);
        block(&mut feed, &clock, 0, 0);
        let arrives = at(200);
        tx.push(timed(&clock, 2, 1, arrives, Msg::controller(2, 7, 10)))
            .unwrap();
        tx.push(timed(&clock, 2, 2, arrives, Msg::controller(3, 7, 11)))
            .unwrap();
        tx.push(timed(&clock, 2, 3, arrives, Msg::controller(2, 8, 12)))
            .unwrap();
        let mut played = block(&mut feed, &clock, 1, at(256));
        played.extend(block(&mut feed, &clock, 2, at(384)));
        assert_eq!(heard_by_the_tap(&mut tap).len(), 3);
        let kinds: Vec<_> = played.iter().map(|m| m.msg).collect();
        assert_eq!(
            kinds,
            vec![Msg::controller(3, 7, 11), Msg::controller(2, 8, 12)]
        );
        tap.take([]);
        tx.push(timed(&clock, 2, 4, at(300), Msg::controller(2, 7, 13)))
            .unwrap();
        let mut played = block(&mut feed, &clock, 3, at(512));
        played.extend(block(&mut feed, &clock, 4, at(640)));
        assert!(played.iter().any(|m| m.msg == Msg::controller(2, 7, 13)));
    }

    #[test]
    fn controllers_there_is_no_room_to_tell_are_counted_and_never_waited_for() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(TAP_QUEUE + 64);
        let (mut feed, _keys, mut tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        let sent = TAP_QUEUE + 40;
        for i in 0..sent {
            tx.push(timed(
                &clock,
                1,
                i as u64,
                at(10),
                Msg::controller(0, 1, (i % 128) as u8),
            ))
            .unwrap();
        }
        block(&mut feed, &clock, 1, at(256));
        assert_eq!(heard_by_the_tap(&mut tap).len(), TAP_QUEUE);
        assert_eq!(tap.dropped(), 40);
    }

    #[test]
    fn a_note_on_is_told_to_the_interface_for_the_map_editors_learn() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let (mut feed, _keys, mut tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        let arrives = at(200);
        tx.push(timed(&clock, 2, 1, arrives, Msg::note_on(2, 46, 90)))
            .unwrap();
        tx.push(timed(&clock, 2, 2, arrives, Msg::note_off(2, 46)))
            .unwrap();
        block(&mut feed, &clock, 1, at(256));
        block(&mut feed, &clock, 2, at(384));
        let mut notes = Vec::new();
        tap.drain_notes(|n| notes.push(n));
        assert_eq!(notes.len(), 1, "the note-on, not the release");
        assert_eq!(
            (notes[0].key, notes[0].velocity, notes[0].source),
            (46, 90, 2)
        );
    }

    #[test]
    fn notes_from_a_port_and_from_the_keyboard_are_raw_only_while_the_flag_is_up() {
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(16);
        let (mut feed, mut keys, tap) = live(Some(rx), Arc::clone(&clock));
        block(&mut feed, &clock, 0, 0);
        let arrives = at(200);
        tx.push(timed(&clock, 2, 1, arrives, Msg::note_on(0, 46, 90)))
            .unwrap();
        assert!(keys.send(Msg::note_on(0, 50, 90)));
        assert!(keys.send_to(7, Msg::note_on(0, 51, 90)));
        let mut plain = Vec::new();
        for i in 1..4 {
            plain.extend(block(&mut feed, &clock, i, at(128 * (i as i64 + 1))));
        }
        assert_eq!(plain.len(), 3);
        assert!(plain.iter().all(|m| !m.raw), "the map answers");

        tap.set_raw(true);
        tx.push(timed(&clock, 2, 2, at(300), Msg::note_on(0, 47, 90)))
            .unwrap();
        assert!(keys.send(Msg::note_on(0, 52, 90)));
        assert!(keys.send_to(7, Msg::note_on(0, 53, 90)));
        let mut raw = Vec::new();
        for i in 4..8 {
            raw.extend(block(&mut feed, &clock, i, at(128 * (i as i64 + 1))));
        }
        let by_key = |key: u8| {
            raw.iter()
                .find(|m| m.msg == Msg::note_on(0, key, 90))
                .copied()
                .expect("the note came through")
        };
        assert!(by_key(47).raw, "a port's note");
        assert!(by_key(52).raw, "the computer keyboard's note");
        assert!(!by_key(53).raw, "an audition is always mapped");

        assert!(keys.send(Msg::note_off(0, 52)));
        tap.set_raw(false);
        let mut off = Vec::new();
        for i in 8..11 {
            off.extend(block(&mut feed, &clock, i, at(128 * (i as i64 + 1))));
        }
        assert_eq!(off.len(), 1);
        assert!(
            off.iter().all(|m| m.raw),
            "a message sent while the flag was up stays as it was sent"
        );
    }

    #[test]
    fn telling_and_taking_controllers_allocates_nothing_on_the_audio_thread() {
        assert!(lumen_rt::guard_is_active());
        let clock = StreamClock::new();
        let (mut tx, rx) = rtrb::RingBuffer::new(TAP_QUEUE + 64);
        let (mut feed, _keys, tap) = live(Some(rx), Arc::clone(&clock));
        tap.take([(0, 1)]);
        block(&mut feed, &clock, 0, 0);
        for i in 0..TAP_QUEUE + 40 {
            let msg = if i % 50 == 49 {
                Msg::note_on(0, 60, 9)
            } else {
                Msg::controller(0, (i % 2) as u8, (i % 128) as u8)
            };
            tx.push(timed(&clock, 1, i as u64, at(10), msg)).unwrap();
        }
        clock.publish_at(128, 128, 48_000, at(256));
        let mut taken = 0_usize;
        let ((), seen) = lumen_rt::count_rt_violations(|| {
            feed.pull(128, 256, &mut |_| taken += 1);
        });
        assert_eq!(seen, 0, "the tap allocates or frees on the audio thread");
        assert!(tap.dropped() > 0, "the overflow was met");
    }
}
