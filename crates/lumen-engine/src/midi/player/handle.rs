use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::super::capture::Captured;
use super::super::live::LiveInput;
use super::super::{MidiOut, MidiTrack};
use super::Feed;
use super::set::MidiSet;

pub struct MidiHandle {
    pub(super) incoming: rtrb::Producer<Box<MidiSet>>,
    pub(super) retired: rtrb::Consumer<Box<MidiSet>>,
    pub(super) output: Option<rtrb::Consumer<MidiOut>>,
    pub(super) feeds: rtrb::Producer<Feed>,
    pub(super) old_feeds: rtrb::Consumer<Feed>,
    pub(super) captured: Option<rtrb::Consumer<Captured>>,
    pub(super) sync: Arc<AtomicU64>,
    pub(super) hosted_pending: Arc<AtomicBool>,
}

impl MidiHandle {
    pub fn set_sync(&self, ports: u64) {
        self.sync.store(ports, Ordering::Relaxed);
    }

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

    pub fn set_live(&mut self, feed: Box<dyn LiveInput>) -> Result<(), Box<dyn LiveInput>> {
        self.feeds
            .push(feed)
            .map_err(|rtrb::PushError::Full(feed)| feed)
    }

    pub fn take_captured(&mut self) -> Option<rtrb::Consumer<Captured>> {
        self.captured.take()
    }

    pub fn take_output(&mut self) -> Option<rtrb::Consumer<MidiOut>> {
        self.output.take()
    }
}
