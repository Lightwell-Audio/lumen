//! The UI side's MIDI handle: replaces the track list, hands over the live
//! input, and frees what the audio side gives back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::super::capture::Captured;
use super::super::live::LiveInput;
use super::super::{MidiOut, MidiTrack};
use super::Feed;
use super::set::MidiSet;

/// UI side: replaces the tracks and frees old lists. May allocate.
pub struct MidiHandle {
    pub(super) incoming: rtrb::Producer<Box<MidiSet>>,
    pub(super) retired: rtrb::Consumer<Box<MidiSet>>,
    pub(super) output: Option<rtrb::Consumer<MidiOut>>,
    pub(super) feeds: rtrb::Producer<Feed>,
    pub(super) old_feeds: rtrb::Consumer<Feed>,
    pub(super) captured: Option<rtrb::Consumer<Captured>>,
    pub(super) sync: Arc<AtomicU64>,
    /// A list with a hosted instrument waits for the audio side.
    pub(super) hosted_pending: Arc<AtomicBool>,
}

impl MidiHandle {
    /// The ports that get MIDI clock (M5 slice 7): bit `n` for port `n`,
    /// ports 0 to 63.
    pub fn set_sync(&self, ports: u64) {
        self.sync.store(ports, Ordering::Relaxed);
    }

    /// Sends a new track list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, tracks: Vec<MidiTrack>) -> Result<(), Vec<MidiTrack>> {
        let set = MidiSet::new(tracks);
        let hosted = set.hosted();
        self.incoming
            .push(Box::new(set))
            .map(|()| {
                if hosted {
                    self.hosted_pending.store(true, Ordering::Release);
                }
            })
            .map_err(|rtrb::PushError::Full(set)| set.tracks)
    }

    /// Frees lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        while self.old_feeds.pop().is_ok() {
            freed += 1;
        }
        freed
    }

    /// Gives the audio side the live input to play (replacing any before
    /// it). Returns it if the queue is full (call again later).
    pub fn set_live(&mut self, feed: Box<dyn LiveInput>) -> Result<(), Box<dyn LiveInput>> {
        self.feeds
            .push(feed)
            .map_err(|rtrb::PushError::Full(feed)| feed)
    }

    /// The messages recording tracks kept, once: the app's recorder takes
    /// them (M5 slice 6).
    pub fn take_captured(&mut self) -> Option<rtrb::Consumer<Captured>> {
        self.captured.take()
    }

    /// The messages for hardware, once: the app's sender takes them.
    pub fn take_output(&mut self) -> Option<rtrb::Consumer<MidiOut>> {
        self.output.take()
    }
}
