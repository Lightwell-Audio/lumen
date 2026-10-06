//! The musical timeline the engine plays against: tempo and time signature.
//! Immutable once built; changes are made by building a new one off the
//! audio thread and swapping it in ([`crate::EngineHandle::set_timeline`]).

use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{TempoMap, Ticks};

#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub tempo: TempoMap,
    pub meter: MeterMap,
}

impl Timeline {
    pub fn new(tempo: TempoMap, meter: MeterMap) -> Self {
        Self { tempo, meter }
    }

    /// One tempo and one time signature for the whole timeline.
    ///
    /// # Panics
    /// If `bpm` is outside 20–999 or `meter` is not valid; check user input
    /// with [`TempoMap::constant`] and [`Meter::is_valid`] first.
    pub fn constant(bpm: f64, meter: Meter) -> Self {
        Self {
            tempo: TempoMap::constant(bpm).expect("tempo in range"),
            meter: MeterMap::constant(meter).expect("valid time signature"),
        }
    }

    /// The sample at `t` (the tempo map's rounding rule).
    pub fn sample_at(&self, t: Ticks, sample_rate: u32) -> i64 {
        self.tempo.sample_at(t, sample_rate)
    }

    /// The tick nearest to the start of `sample`.
    pub fn ticks_at(&self, sample: i64, sample_rate: u32) -> Ticks {
        self.tempo.ticks_at_sample(sample, sample_rate)
    }
}

impl Default for Timeline {
    /// 120 BPM in 4/4, the default for a new project.
    fn default() -> Self {
        Self::constant(120.0, Meter::FOUR_FOUR)
    }
}
