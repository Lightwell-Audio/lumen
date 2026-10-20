//! The parameter changes of one block (M7 slice A3, ADR-0084): what a lane
//! of a plugin's parameter becomes when the engine hands the host a block.
//!
//! Each queue is one parameter's `(sample offset, normalized value)` points
//! in time order, the shape VST3 takes them in. The block is filled on the
//! audio thread, so its room is fixed when it is made and a point that does
//! not fit is refused and counted, never grown into.

/// The most parameters that change in one block: the room of the shared
/// region's default layout.
pub const MOST_QUEUES: usize = 64;

/// The most points in one block, over all its parameters.
pub const MOST_POINTS: usize = 1024;

/// A mark to go back to with [`ParamBlock::rewind`].
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    queues: usize,
    points: usize,
}

/// The parameter changes of one block. Allocates once, in [`ParamBlock::new`].
#[derive(Debug)]
pub struct ParamBlock {
    /// Each queue's parameter id and where its points end in `points`.
    queues: Vec<(u32, u32)>,
    points: Vec<(u32, f64)>,
    /// The block's frames: points are placed inside it.
    frames: u32,
    /// Queues of this block given up on or cut down for want of room.
    dropped: u32,
}

impl ParamBlock {
    /// A block with room for [`MOST_QUEUES`] parameters and [`MOST_POINTS`]
    /// points. Allocates: call off the audio thread.
    pub fn new() -> Self {
        Self {
            queues: Vec::with_capacity(MOST_QUEUES),
            points: Vec::with_capacity(MOST_POINTS),
            frames: 0,
            dropped: 0,
        }
    }

    /// A block with no room, for a call that has nothing to say. Allocates
    /// nothing.
    pub const fn none() -> Self {
        Self {
            queues: Vec::new(),
            points: Vec::new(),
            frames: 0,
            dropped: 0,
        }
    }

    /// Starts a block of `frames`: nothing changes in it yet.
    pub fn start(&mut self, frames: usize) {
        self.queues.clear();
        self.points.clear();
        self.frames = frames.min(u32::MAX as usize) as u32;
        self.dropped = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.queues.is_empty()
    }

    /// How many parameters change.
    pub fn len(&self) -> usize {
        self.queues.len()
    }

    /// The `i`th parameter's id and its points.
    pub fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        let (id, end) = self.queues[i];
        let start = i
            .checked_sub(1)
            .map_or(0, |before| self.queues[before].1 as usize);
        (id, &self.points[start..end as usize])
    }

    /// Where the block stands, to take back what is added after it.
    pub fn mark(&self) -> Mark {
        Mark {
            queues: self.queues.len(),
            points: self.points.len(),
        }
    }

    /// Takes back everything added since `mark`.
    pub fn rewind(&mut self, mark: Mark) {
        self.queues.truncate(mark.queues);
        self.points.truncate(mark.points);
    }

    /// Starts a queue for parameter `id`. False when the block holds as many
    /// as it may.
    pub fn open(&mut self, id: u32) -> bool {
        if self.queues.len() >= MOST_QUEUES {
            return false;
        }
        self.queues.push((id, self.points.len() as u32));
        true
    }

    /// Adds a point to the open queue, `offset` frames into the block, in
    /// time order: one at or before the last is replaced by it. A value
    /// outside 0 to 1, or not a number, is brought in; an offset past the
    /// block is put on its last frame. False when the block holds as many
    /// points as it may.
    pub fn point(&mut self, offset: u32, value: f64) -> bool {
        if self.queues.is_empty() {
            return false;
        }
        let offset = offset.min(self.frames.saturating_sub(1));
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let start = self.queues_start();
        if let Some(last) = self.points[start..].last_mut()
            && last.0 >= offset
        {
            *last = (last.0, value);
            return true;
        }
        if self.points.len() >= MOST_POINTS {
            return false;
        }
        self.points.push((offset, value));
        if let Some(open) = self.queues.last_mut() {
            open.1 = self.points.len() as u32;
        }
        true
    }

    /// Where the open queue's points start.
    fn queues_start(&self) -> usize {
        self.queues
            .len()
            .checked_sub(2)
            .map_or(0, |before| self.queues[before].1 as usize)
    }

    /// A queue was given up on or cut down to its two ends: it did not fit.
    pub fn note_cut(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    /// Queues given up on or cut down in this block.
    pub fn cut(&self) -> u32 {
        self.dropped
    }

    /// Becomes a copy of `other`, within its own room.
    pub fn copy_from(&mut self, other: &ParamBlock) {
        self.queues.clear();
        self.points.clear();
        self.frames = other.frames;
        self.dropped = other.dropped;
        let queues = other.queues.len().min(MOST_QUEUES);
        for i in 0..queues {
            let (id, list) = other.queue(i);
            if self.points.len() + list.len() > MOST_POINTS.min(self.points.capacity()) {
                self.dropped = self.dropped.saturating_add((queues - i) as u32);
                break;
            }
            self.queues.push((id, 0));
            self.points.extend_from_slice(list);
            let end = self.points.len() as u32;
            if let Some(last) = self.queues.last_mut() {
                last.1 = end;
            }
        }
    }
}

impl Default for ParamBlock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> ParamBlock {
        let mut p = ParamBlock::new();
        p.start(128);
        assert!(p.open(7));
        assert!(p.point(0, 0.25) && p.point(64, 0.5));
        assert!(p.open(9));
        assert!(p.point(10, 1.0));
        p
    }

    #[test]
    fn queues_hold_their_own_points_in_order() {
        let p = filled();
        assert_eq!(p.len(), 2);
        assert_eq!(p.queue(0), (7, &[(0, 0.25), (64, 0.5)][..]));
        assert_eq!(p.queue(1), (9, &[(10, 1.0)][..]));
    }

    #[test]
    fn a_point_is_kept_inside_the_block_and_the_range() {
        let mut p = ParamBlock::new();
        p.start(100);
        p.open(1);
        p.point(5_000, 7.0);
        assert_eq!(p.queue(0).1, &[(99, 1.0)][..]);
        let mut p = ParamBlock::new();
        p.start(100);
        p.open(1);
        p.point(3, f64::NAN);
        p.point(4, -2.0);
        assert_eq!(p.queue(0).1, &[(3, 0.0), (4, 0.0)][..]);
    }

    #[test]
    fn a_point_at_or_before_the_last_replaces_it() {
        let mut p = ParamBlock::new();
        p.start(100);
        p.open(1);
        p.point(10, 0.1);
        p.point(10, 0.9);
        p.point(4, 0.5);
        assert_eq!(p.queue(0).1, &[(10, 0.5)][..]);
    }

    #[test]
    fn rewinding_takes_back_a_queue_and_its_points() {
        let mut p = filled();
        let mark = p.mark();
        p.open(11);
        p.point(1, 0.3);
        p.rewind(mark);
        assert_eq!(p.len(), 2);
        assert_eq!(p.queue(1), (9, &[(10, 1.0)][..]));
    }

    #[test]
    fn the_block_refuses_what_it_has_no_room_for() {
        let mut p = ParamBlock::new();
        p.start(4096);
        for id in 0..MOST_QUEUES as u32 {
            assert!(p.open(id));
        }
        assert!(!p.open(999), "past the queues");
        let mut p = ParamBlock::new();
        p.start(4096);
        p.open(1);
        for k in 0..MOST_POINTS as u32 {
            assert!(p.point(k, 0.5));
        }
        assert!(!p.point(MOST_POINTS as u32, 0.5), "past the points");
    }

    #[test]
    fn a_copy_is_the_same_block() {
        let p = filled();
        let mut q = ParamBlock::new();
        q.copy_from(&p);
        assert_eq!(q.len(), 2);
        assert_eq!(q.queue(0), p.queue(0));
        assert_eq!(q.queue(1), p.queue(1));
    }

    #[test]
    fn no_block_has_no_room_and_costs_nothing() {
        let p = ParamBlock::none();
        assert!(p.is_empty());
        assert_eq!(p.points.capacity(), 0);
    }
}
