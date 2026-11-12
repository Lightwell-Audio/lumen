//! Crossfades: a clip list change that moves clips fades over 5 ms, each
//! moved clip ramping between its old and new placement.

use super::set::ClipSet;

/// A clip list change that moves clips crossfades over 5 ms.
pub(super) const SWAP_PER_SECOND: u32 = 200;

/// A crossfade from one clip list to the next.
pub(super) struct Swap {
    pub(super) old: Box<ClipSet>,
    /// Frames played of `length` (set on the first block, from its rate).
    pub(super) done: i64,
    pub(super) length: i64,
}

/// How a clip fades across a clip list change, on top of its level.
#[derive(Clone, Copy)]
pub(super) enum Ramp {
    Steady,
    /// Its new placement fades in.
    In(Crossfade),
    /// Its old placement fades out, and is silent after.
    Out(Crossfade),
}

/// Where a crossfade stands in this block.
#[derive(Clone, Copy)]
pub(super) struct Crossfade {
    /// Timeline sample at which it ends. Counted from the block, so it
    /// holds while scrubbing too, where blocks do not follow each other.
    pub(super) end: i64,
    pub(super) length: i64,
}

impl Ramp {
    pub(super) fn active(self, t: i64) -> bool {
        match self {
            Ramp::Steady => false,
            Ramp::In(x) | Ramp::Out(x) => t < x.end,
        }
    }

    /// The gain at timeline sample `t`. In and out always add up to one.
    #[inline]
    pub(super) fn gain(self, t: i64) -> f32 {
        let progress =
            |x: Crossfade| (((t - (x.end - x.length)) as f32 + 0.5) / x.length as f32).min(1.0);
        match self {
            Ramp::Steady => 1.0,
            Ramp::In(x) => progress(x),
            Ramp::Out(x) => 1.0 - progress(x),
        }
    }
}
