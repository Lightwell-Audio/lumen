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

    pub fn constant(bpm: f64, meter: Meter) -> Self {
        Self {
            tempo: TempoMap::constant(bpm).expect("tempo in range"),
            meter: MeterMap::constant(meter).expect("valid time signature"),
        }
    }

    pub fn sample_at(&self, t: Ticks, sample_rate: u32) -> i64 {
        self.tempo.sample_at(t, sample_rate)
    }

    pub fn ticks_at(&self, sample: i64, sample_rate: u32) -> Ticks {
        self.tempo.ticks_at_sample(sample, sample_rate)
    }

    pub fn bpm_at(&self, sample: i64, sample_rate: u32) -> f32 {
        self.tempo.bpm_at(self.ticks_at(sample, sample_rate)) as f32
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::constant(120.0, Meter::FOUR_FOUR)
    }
}
