//! The track buses of the plan in use, for a [`TrackSource`] to play into.
//!
//! [`TrackSource`]: super::super::TrackSource

use super::{BUS, CHUNK};

pub struct Buses<'a> {
    pub(super) by_id: &'a [(u64, u32)],
    pub(super) data: &'a mut [f32],
    pub(super) frames: usize,
}

impl<'a> Buses<'a> {
    /// Buses over `data`, one [`CHUNK`] of each of [`BUS_CHANNELS`] sides per
    /// track in `by_id` (sorted by id, with the track's place). `frames` is
    /// at most [`CHUNK`].
    pub(crate) fn over(by_id: &'a [(u64, u32)], data: &'a mut [f32], frames: usize) -> Self {
        Self {
            by_id,
            data,
            frames,
        }
    }
}

impl Buses<'_> {
    /// Frames in this part.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The bus of track `id`; `None` when the routing has no such track
    /// (its audio is not heard).
    pub fn track(&mut self, id: u64) -> Option<Bus<'_>> {
        let index = self
            .by_id
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|i| self.by_id[i].1 as usize)?;
        Some(Bus {
            data: &mut self.data[index * BUS..(index + 1) * BUS],
            frames: self.frames,
        })
    }
}

/// A track's stereo bus for this part. Sources add to it.
pub struct Bus<'a> {
    data: &'a mut [f32],
    frames: usize,
}

impl Bus<'_> {
    /// Side 0 (left) or 1 (right): `frames` samples, zeroed at the start
    /// of the part.
    pub fn channel(&mut self, side: usize) -> &mut [f32] {
        let start = side.min(1) * CHUNK;
        &mut self.data[start..start + self.frames]
    }
}
