mod carry;
mod handle;
mod out;
mod part;
mod play;
mod set;
mod source;

pub use handle::MidiHandle;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use super::capture::{self, Captured};
use super::clock::ClockOut;
use super::hold::MAX_EVENTS;
use super::live::LiveInput;
use super::{MidiTrack, Target};

pub(in crate::midi) use out::Out;
use set::MidiSet;

const QUEUE: usize = 4;

pub(super) type Feed = Box<dyn LiveInput>;

pub struct MidiPlayer {
    pub(super) incoming: rtrb::Consumer<Box<MidiSet>>,
    pub(super) retired: rtrb::Producer<Box<MidiSet>>,
    pub(super) stash: Option<Box<MidiSet>>,
    pub(super) current: Box<MidiSet>,
    pub(super) out: Out,
    pub(super) clock: u64,
    pub(super) feeds: rtrb::Consumer<Feed>,
    pub(super) old_feeds: rtrb::Producer<Feed>,
    pub(super) live: Option<Feed>,
    pub(super) old_feed: Option<Feed>,
    pub(super) capture: rtrb::Producer<Captured>,
    pub(super) lost: u32,
    pub(super) pass: u32,
    pub(super) wrapped_since_locate: bool,
    pub(super) recording: bool,
    pub(super) sync: Arc<AtomicU64>,
    pub(super) synced: u64,
    pub(super) clock_out: ClockOut,
    pub(super) hosted: bool,
    pub(super) hosted_pending: Arc<AtomicBool>,
    pub(super) prepared: bool,
}

pub fn midi_player() -> (MidiHandle, MidiPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (out_tx, out_rx) = rtrb::RingBuffer::new(out::OUT_QUEUE);
    let (feeds_tx, feeds_rx) = rtrb::RingBuffer::new(2);
    let (old_feeds_tx, old_feeds_rx) = rtrb::RingBuffer::new(3);
    let (capture_tx, capture_rx) = rtrb::RingBuffer::new(capture::QUEUE);
    let sync = Arc::new(AtomicU64::new(0));
    let hosted_pending = Arc::new(AtomicBool::new(false));
    (
        MidiHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            output: Some(out_rx),
            feeds: feeds_tx,
            old_feeds: old_feeds_rx,
            captured: Some(capture_rx),
            sync: Arc::clone(&sync),
            hosted_pending: Arc::clone(&hosted_pending),
        },
        MidiPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(MidiSet::new(Vec::new())),
            out: Out {
                tx: out_tx,
                dropped: 0,
            },
            clock: 0,
            feeds: feeds_rx,
            old_feeds: old_feeds_tx,
            live: None,
            old_feed: None,
            capture: capture_tx,
            lost: 0,
            pass: 0,
            wrapped_since_locate: false,
            recording: false,
            sync,
            synced: 0,
            clock_out: ClockOut::default(),
            hosted: false,
            hosted_pending,
            prepared: false,
        },
    )
}

impl MidiPlayer {
    pub fn dropped(&self) -> u32 {
        let tracks: u32 = self
            .current
            .tracks
            .iter()
            .map(|t| t.state.dropped.saturating_add(t.state.live.dropped))
            .sum();
        tracks
            .saturating_add(self.out.dropped)
            .saturating_add(self.lost)
    }

    pub fn held(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.held.len())
    }

    pub fn held_live(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.live.len())
    }
}

impl MidiTrack {
    pub(super) fn sounds(&self) -> bool {
        matches!(self.target, Target::Port { .. }) || self.instrument.is_some()
    }
}

const _: () = assert!(MAX_EVENTS >= 512, "a panic and a chase need room");
