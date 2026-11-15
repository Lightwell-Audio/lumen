use lumen_core::time::{MAX_BPM, MIN_BPM, Ticks};
use lumen_core::warp::{self, Marker};
use serde::{Deserialize, Serialize};

pub const MAX_TRANSPOSE_CENTS: i32 = 4_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpMode {
    #[default]
    Repitch,
    Rhythmic,
    Melodic,
    FullMix,
}

impl WarpMode {
    pub const ALL: [WarpMode; 4] = [Self::Repitch, Self::Rhythmic, Self::Melodic, Self::FullMix];

    pub fn transposes(self) -> bool {
        self != Self::Repitch
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    pub mode: WarpMode,
    pub original_bpm: f64,
    pub markers: Vec<Marker>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transpose_cents: i32,
}

fn is_zero(cents: &i32) -> bool {
    *cents == 0
}

impl Warp {
    pub fn new(original_bpm: f64, source: f64) -> Self {
        Warp {
            mode: WarpMode::default(),
            original_bpm,
            markers: vec![Marker::new(source, Ticks::ZERO)],
            transpose_cents: 0,
        }
    }

    pub fn source_at(&self, at: Ticks) -> f64 {
        warp::source_at(&self.markers, self.original_bpm, at)
    }

    pub fn tick_at_source(&self, source: f64) -> Ticks {
        warp::tick_at_source(&self.markers, self.original_bpm, source)
    }

    pub fn first_tick(&self) -> Ticks {
        let mut at = self.tick_at_source(0.0);
        for _ in 0..16 {
            if self.source_at(at) < 0.0 {
                at = at + Ticks(1);
            } else if self.source_at(at - Ticks(1)) >= 0.0 {
                at = at - Ticks(1);
            } else {
                break;
            }
        }
        at
    }

    pub fn last_tick(&self, total: f64) -> Ticks {
        let mut at = self.tick_at_source(total);
        for _ in 0..16 {
            if self.source_at(at) > total {
                at = at - Ticks(1);
            } else if self.source_at(at + Ticks(1)) <= total {
                at = at + Ticks(1);
            } else {
                break;
            }
        }
        at
    }

    pub fn shift(&mut self, by: Ticks) {
        warp::shift(&mut self.markers, by);
    }

    pub fn shifted(&self, by: Ticks) -> Warp {
        let mut out = self.clone();
        out.shift(by);
        out
    }

    pub fn continues(&self, next: &Warp, by: Ticks) -> bool {
        self.shifted(Ticks::ZERO - by) == *next
    }

    pub fn check(&self) -> Result<(), &'static str> {
        if !(MIN_BPM..=MAX_BPM).contains(&self.original_bpm) {
            return Err("its original tempo is out of range");
        }
        if self.markers.is_empty() {
            return Err("it has no markers");
        }
        if !warp::is_sound(&self.markers) {
            return Err("its markers are not a map that moves forward");
        }
        if !(-MAX_TRANSPOSE_CENTS..=MAX_TRANSPOSE_CENTS).contains(&self.transpose_cents) {
            return Err("its transpose is out of range");
        }
        if !self
            .markers
            .iter()
            .all(|m| m.source.abs() <= LONGEST_FILE_SECONDS && crate::validate::within(m.at))
        {
            return Err("a marker lies outside the song or the file");
        }
        Ok(())
    }
}

const LONGEST_FILE_SECONDS: f64 = 1.0e7;
