use super::set::ClipSet;

pub(super) const SWAP_PER_SECOND: u32 = 200;

pub(super) struct Swap {
    pub(super) old: Box<ClipSet>,
    pub(super) done: i64,
    pub(super) length: i64,
}

#[derive(Clone, Copy)]
pub(super) enum Ramp {
    Steady,
    In(Crossfade),
    Out(Crossfade),
}

#[derive(Clone, Copy)]
pub(super) struct Crossfade {
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
