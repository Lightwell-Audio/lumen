//! The UI side's clip handle: replaces the clip list and frees old ones.

use std::collections::HashMap;

use super::Clip;
use super::fades::Table;
use super::player::ClipPlayer;
use super::set::ClipSet;
use crate::stream::{LoadPages, NoLoader, PageLoad};

const QUEUE: usize = 4;

/// UI side: replaces the clip list and frees old ones. May allocate.
pub struct ClipsHandle {
    incoming: rtrb::Producer<Box<ClipSet>>,
    retired: rtrb::Consumer<Box<ClipSet>>,
    /// Fade tables by curve, shared between clips and lists.
    tables: HashMap<[u32; 4], Table>,
    /// The lists sent leave the outermost edge of each track unfaded
    /// ([`ClipsHandle::bare_ends`]).
    bare: bool,
}

impl ClipsHandle {
    /// For lists sent from now on, the start of the first clip and the end of
    /// the last clip of each track get no 2 ms edge fade, as inside a seamless
    /// join. Rendering a stretch of clips into a file uses it: the file's
    /// outer edges are faded when the new clip plays, as any clip is, and the
    /// edges inside it keep the fades they had (M7b slice E3).
    pub fn bare_ends(&mut self, on: bool) {
        self.bare = on;
    }

    /// Sends a new clip list. Returns it if the queue is full (call again
    /// next frame).
    pub fn set(&mut self, clips: Vec<Clip>) -> Result<(), Vec<Clip>> {
        self.incoming
            .push(Box::new(ClipSet::new(clips, &mut self.tables, self.bare)))
            .map_err(|rtrb::PushError::Full(set)| set.clips)
    }

    /// Frees clip lists the audio side has replaced. Call regularly.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.retired.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

/// Creates the clip pair, starting with no clips. Allocates, so call it off
/// the audio thread. What it makes cannot load a streamed page: that is the
/// audio thread's player (sweep M16b).
pub fn clip_player() -> (ClipsHandle, ClipPlayer) {
    clip_player_for::<NoLoader>()
}

/// As [`clip_player`], for an offline render: a page a streamed clip misses
/// is loaded on the spot, on the rendering thread, so the render never
/// starves. Never give its player to the audio thread.
pub fn clip_player_for_render() -> (ClipsHandle, ClipPlayer<LoadPages>) {
    clip_player_for::<LoadPages>()
}

/// As [`clip_player`], with `L` deciding whether a missing page is loaded.
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
