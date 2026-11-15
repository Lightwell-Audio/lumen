use std::collections::HashMap;

use super::Clip;
use super::fades::Table;
use super::player::ClipPlayer;
use super::set::ClipSet;
use crate::stream::{LoadPages, NoLoader, PageLoad};

const QUEUE: usize = 4;

pub struct ClipsHandle {
    incoming: rtrb::Producer<Box<ClipSet>>,
    retired: rtrb::Consumer<Box<ClipSet>>,
    tables: HashMap<[u32; 4], Table>,
    bare: bool,
}

impl ClipsHandle {
    pub fn bare_ends(&mut self, on: bool) {
        self.bare = on;
    }

    pub fn set(&mut self, clips: Vec<Clip>) -> Result<(), Vec<Clip>> {
        self.incoming
            .push(Box::new(ClipSet::new(clips, &mut self.tables, self.bare)))
            .map_err(|rtrb::PushError::Full(set)| set.clips)
    }

    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

pub fn clip_player() -> (ClipsHandle, ClipPlayer) {
    clip_player_for::<NoLoader>()
}

pub fn clip_player_for_render() -> (ClipsHandle, ClipPlayer<LoadPages>) {
    clip_player_for::<LoadPages>()
}

fn clip_player_for<L: PageLoad>() -> (ClipsHandle, ClipPlayer<L>) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    (
        ClipsHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            tables: HashMap::new(),
            bare: false,
        },
        ClipPlayer::new(incoming_rx, retired_tx),
    )
}
