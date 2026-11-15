const JUMPS: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct Playhead {
    clock: i64,
    base: (i64, i64),
    jumps: [(i64, i64); JUMPS],
    pending: usize,
    lookahead: i64,
    preroll: (i64, i64),
}

impl Default for Playhead {
    fn default() -> Self {
        Self {
            clock: 0,
            base: (0, 0),
            jumps: [(0, 0); JUMPS],
            pending: 0,
            lookahead: 0,
            preroll: (0, 0),
        }
    }
}

impl Playhead {
    pub fn start(&mut self, sample: i64, lookahead: i64) {
        self.lookahead = lookahead;
        self.base = (self.clock, sample - lookahead);
        self.pending = 0;
        self.preroll = (self.clock + lookahead, sample);
        self.jump(sample);
    }

    pub fn resync(&mut self, playhead: i64, lookahead: i64) {
        self.lookahead = lookahead;
        self.base = (self.clock, playhead);
        self.pending = 0;
        self.preroll = (self.clock, playhead);
    }

    pub fn jump(&mut self, sample: i64) {
        if self.pending == JUMPS {
            let (clock, sample) = self.jumps[0];
            self.base = (clock + self.lookahead, sample);
            self.jumps.copy_within(1.., 0);
            self.pending -= 1;
        }
        self.jumps[self.pending] = (self.clock, sample);
        self.pending += 1;
        self.hear();
    }

    pub fn advance(&mut self, frames: i64) {
        self.clock += frames;
        self.hear();
    }

    pub fn reposition(&mut self, playhead: i64, map: impl Fn(i64) -> i64) {
        self.base = (self.clock, playhead);
        for jump in &mut self.jumps[..self.pending] {
            jump.1 = map(jump.1);
        }
        self.preroll.1 = map(self.preroll.1);
    }

    pub fn position(&self) -> i64 {
        self.base.1 + (self.clock - self.base.0)
    }

    pub fn until_jump(&self) -> Option<i64> {
        (self.pending > 0).then(|| self.jumps[0].0 + self.lookahead - self.clock)
    }

    pub fn prerolling(&self) -> bool {
        self.clock < self.preroll.0
    }

    pub fn shown(&self) -> i64 {
        if self.prerolling() {
            self.preroll.1
        } else {
            self.position()
        }
    }

    fn hear(&mut self) {
        while self.pending > 0 && self.jumps[0].0 + self.lookahead <= self.clock {
            let (clock, sample) = self.jumps[0];
            self.base = (clock + self.lookahead, sample);
            self.jumps.copy_within(1.., 0);
            self.pending -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_lookahead_the_playhead_is_the_render_position() {
        let mut head = Playhead::default();
        head.start(1_000, 0);
        assert_eq!(head.position(), 1_000);
        assert!(!head.prerolling());
        head.advance(50);
        head.jump(200);
        assert_eq!(head.position(), 200);
        assert_eq!(head.until_jump(), None);
    }

    #[test]
    fn with_lookahead_jumps_are_heard_that_much_later() {
        let mut head = Playhead::default();
        head.start(1_000, 100);
        assert!(head.prerolling());
        assert_eq!(head.position(), 900);
        assert_eq!(head.shown(), 1_000);
        assert_eq!(head.until_jump(), Some(100));
        head.advance(100);
        assert!(!head.prerolling());
        assert_eq!(head.position(), 1_000);
        head.advance(400);
        head.jump(0);
        assert_eq!(head.position(), 1_400);
        assert_eq!(head.until_jump(), Some(100));
        head.advance(99);
        assert_eq!(head.position(), 1_499);
        head.advance(1);
        assert_eq!(head.position(), 0);
    }

    #[test]
    fn a_new_lookahead_keeps_the_playhead_where_it_is() {
        let mut head = Playhead::default();
        head.start(0, 10);
        head.advance(500);
        assert_eq!(head.position(), 490);
        head.resync(490, 40);
        head.jump(530);
        head.advance(20);
        assert_eq!(head.position(), 510);
        head.advance(20);
        assert_eq!(head.position(), 530);
    }

    #[test]
    fn too_many_jumps_are_heard_early_rather_than_lost() {
        let mut head = Playhead::default();
        head.start(0, 1_000);
        for _ in 0..40 {
            head.advance(10);
            head.jump(0);
        }
        assert!(head.until_jump().is_some());
        head.advance(10_000);
        assert_eq!(head.until_jump(), None);
        assert_eq!(head.position(), 10_400 - 1_400);
    }
}
