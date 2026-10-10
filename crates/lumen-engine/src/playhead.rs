//! The playhead while the source renders ahead of it (delay compensation,
//! M4 slice 6, ADR-0041).
//!
//! A latent routing plays what the tracks rendered `lookahead` frames ago,
//! so the engine renders the timeline that far ahead (read ahead, the
//! owner's choice) and the playhead, what is heard, follows the render
//! position that far behind, jumps included: a loop wrap is rendered
//! `lookahead` frames before it is heard. With no lookahead, the playhead
//! is the render position.

/// Jumps that can be in flight at once. More (a loop far shorter than the
/// compensation) are heard early.
const JUMPS: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct Playhead {
    /// Frames played since playback started.
    clock: i64,
    /// The playhead was at `sample` at clock `clock` and runs on from
    /// there.
    base: (i64, i64),
    /// Jumps of the render position not heard yet, oldest first: (clock,
    /// sample it jumped to).
    jumps: [(i64, i64); JUMPS],
    pending: usize,
    lookahead: i64,
    /// Until this clock, playback is pre-rolling: the render position
    /// started at `.1`, which is not heard yet.
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
    /// Playback starts rendering at `sample`, `lookahead` frames before it
    /// is heard.
    pub fn start(&mut self, sample: i64, lookahead: i64) {
        self.lookahead = lookahead;
        self.base = (self.clock, sample - lookahead);
        self.pending = 0;
        self.preroll = (self.clock + lookahead, sample);
        self.jump(sample);
    }

    /// The lookahead changes while playing: the playhead carries on from
    /// `playhead`, and pending jumps are dropped (the caller moves the
    /// render position).
    pub fn resync(&mut self, playhead: i64, lookahead: i64) {
        self.lookahead = lookahead;
        self.base = (self.clock, playhead);
        self.pending = 0;
        self.preroll = (self.clock, playhead);
    }

    /// The render position jumped to `sample` now.
    pub fn jump(&mut self, sample: i64) {
        if self.pending == JUMPS {
            // Heard early rather than lost.
            let (clock, sample) = self.jumps[0];
            self.base = (clock + self.lookahead, sample);
            self.jumps.copy_within(1.., 0);
            self.pending -= 1;
        }
        self.jumps[self.pending] = (self.clock, sample);
        self.pending += 1;
        self.hear();
    }

    /// `frames` were rendered.
    pub fn advance(&mut self, frames: i64) {
        self.clock += frames;
        self.hear();
    }

    /// Moves every position through `map` (a tempo or rate change), keeping
    /// the playhead at `playhead`.
    pub fn reposition(&mut self, playhead: i64, map: impl Fn(i64) -> i64) {
        self.base = (self.clock, playhead);
        for jump in &mut self.jumps[..self.pending] {
            jump.1 = map(jump.1);
        }
        self.preroll.1 = map(self.preroll.1);
    }

    /// What is heard now.
    pub fn position(&self) -> i64 {
        self.base.1 + (self.clock - self.base.0)
    }

    /// Frames until the next jump is heard, if one is pending.
    pub fn until_jump(&self) -> Option<i64> {
        (self.pending > 0).then(|| self.jumps[0].0 + self.lookahead - self.clock)
    }

    /// Pre-rolling: what was rendered first is not heard yet.
    pub fn prerolling(&self) -> bool {
        self.clock < self.preroll.0
    }

    /// Where the playhead shows: during the pre-roll, where playback
    /// started rather than before it.
    pub fn shown(&self) -> i64 {
        if self.prerolling() {
            self.preroll.1
        } else {
            self.position()
        }
    }

    /// Makes the jumps that are now heard the base.
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
        // Pre-rolling: the playhead shows the start.
        assert!(head.prerolling());
        assert_eq!(head.position(), 900);
        assert_eq!(head.shown(), 1_000);
        assert_eq!(head.until_jump(), Some(100));
        head.advance(100);
        assert!(!head.prerolling());
        assert_eq!(head.position(), 1_000);
        // A loop wrap rendered now (render position 1_500 → 0) is heard
        // 100 frames later, when the playhead reaches 1_500.
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
        // The last wrap, rendered at clock 400, was heard at 1_400.
        assert_eq!(head.position(), 10_400 - 1_400);
    }
}
