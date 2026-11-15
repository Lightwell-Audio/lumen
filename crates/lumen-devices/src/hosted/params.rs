pub const MOST_QUEUES: usize = 64;

pub const MOST_POINTS: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub struct Mark {
    queues: usize,
    points: usize,
}

#[derive(Debug)]
pub struct ParamBlock {
    queues: Vec<(u32, u32, bool)>,
    points: Vec<(u32, f64)>,

    frames: u32,

    dropped: u32,
}

impl ParamBlock {
    pub fn new() -> Self {
        Self {
            queues: Vec::with_capacity(MOST_QUEUES),
            points: Vec::with_capacity(MOST_POINTS),
            frames: 0,
            dropped: 0,
        }
    }

    pub fn has_room(&self) -> bool {
        self.queues.capacity() >= MOST_QUEUES && self.points.capacity() >= MOST_POINTS
    }

    pub const fn none() -> Self {
        Self {
            queues: Vec::new(),
            points: Vec::new(),
            frames: 0,
            dropped: 0,
        }
    }

    pub fn start(&mut self, frames: usize) {
        self.queues.clear();
        self.points.clear();
        self.frames = frames.min(u32::MAX as usize) as u32;
        self.dropped = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.queues.is_empty()
    }

    pub fn frames(&self) -> usize {
        self.frames as usize
    }

    pub fn len(&self) -> usize {
        self.queues.len()
    }

    pub fn queue(&self, i: usize) -> (u32, &[(u32, f64)]) {
        let (id, end, _) = self.queues[i];
        let start = i
            .checked_sub(1)
            .map_or(0, |before| self.queues[before].1 as usize);
        (id, &self.points[start..end as usize])
    }

    pub fn is_lane(&self, i: usize) -> bool {
        self.queues.get(i).is_some_and(|queue| queue.2)
    }

    pub fn mark(&self) -> Mark {
        Mark {
            queues: self.queues.len(),
            points: self.points.len(),
        }
    }

    pub fn rewind(&mut self, mark: Mark) {
        self.queues.truncate(mark.queues);
        self.points.truncate(mark.points);
    }

    pub fn open(&mut self, id: u32) -> bool {
        self.push(id, false)
    }

    pub fn open_lane(&mut self, id: u32) -> bool {
        self.push(id, true)
    }

    fn push(&mut self, id: u32, lane: bool) -> bool {
        if self.queues.len() >= MOST_QUEUES {
            return false;
        }
        self.queues.push((id, self.points.len() as u32, lane));
        true
    }

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

    fn queues_start(&self) -> usize {
        self.queues
            .len()
            .checked_sub(2)
            .map_or(0, |before| self.queues[before].1 as usize)
    }

    pub fn note_cut(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    pub fn cut(&self) -> u32 {
        self.dropped
    }

    pub fn copy_from(&mut self, other: &ParamBlock) {
        debug_assert!(
            self.has_room() || other.is_empty(),
            "an empty placeholder is never copied into: it has no room"
        );
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
            let lane = other.is_lane(i);
            self.queues.push((id, 0, lane));
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
        assert!(p.open_lane(9));
        assert!(p.point(10, 1.0));
        p
    }

    #[test]
    fn queues_hold_their_own_points_in_order() {
        let p = filled();
        assert_eq!(p.len(), 2);
        assert_eq!(p.queue(0), (7, &[(0, 0.25), (64, 0.5)][..]));
        assert_eq!(p.queue(1), (9, &[(10, 1.0)][..]));
        assert!(!p.is_lane(0), "the first queue is a value at rest");
        assert!(p.is_lane(1), "the second is a lane's");
        assert!(!p.is_lane(2));
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
        assert!(!p.has_room());
        assert!(ParamBlock::new().has_room());
    }

    #[test]
    fn a_placeholder_takes_only_an_empty_block() {
        let mut placeholder = ParamBlock::none();
        placeholder.copy_from(&ParamBlock::none());
        assert!(placeholder.is_empty());
        assert!(!placeholder.has_room());
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "an empty placeholder is never copied into")]
    fn copying_into_a_placeholder_is_caught_in_debug_builds() {
        let mut placeholder = ParamBlock::none();
        placeholder.copy_from(&filled());
    }
}
