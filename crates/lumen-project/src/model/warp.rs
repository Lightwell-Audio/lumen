//! Warp (format v33, M9.5 slice S4a): an audio clip that follows the song's
//! tempo instead of playing its file at the speed it was made.
//!
//! A clip with a [`Warp`] is *warped*; one without plays the file as before,
//! bit for bit (D2). The map ([`lumen_core::warp`]) says which second of the
//! file plays at which tick of the clip, measured from the clip's content
//! origin `start − loop_phase`, the clock `loop_phase` counts from. Edits
//! keep it true (`edit::warp`); nothing plays it until S4b.
//!
//! While a clip is warped, `offset` and `audio_phase_seconds` hold where the
//! map puts the origin, so everything that does not know warp yet reads a clip
//! that starts in the right place. Turning warp off drops the map and leaves
//! them as they stand, so the clip plays from there at the file's own speed.

use lumen_core::time::{MAX_BPM, MIN_BPM, Ticks};
use lumen_core::warp::{self, Marker};
use serde::{Deserialize, Serialize};

/// The widest transpose a clip takes, in cents either way: four octaves.
pub const MAX_TRANSPOSE_CENTS: i32 = 4_800;

/// How a warped clip is stretched (M9.5 §2 M).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpMode {
    /// Speed follows the tempo and the pitch moves with it, like tape.
    #[default]
    Repitch,
    /// The audio is cut at transients and each slice plays at its own speed.
    Rhythmic,
    /// Monophonic material: the library's settings for one voice.
    Melodic,
    /// Polyphonic material: the library's best mode.
    FullMix,
}

impl WarpMode {
    pub const ALL: [WarpMode; 4] = [Self::Repitch, Self::Rhythmic, Self::Melodic, Self::FullMix];

    /// Whether the clip can be transposed in this mode. Repitch cannot: a
    /// tape-style transpose would change the speed and the clip would stop
    /// fitting its bars.
    pub fn transposes(self) -> bool {
        self != Self::Repitch
    }
}

/// What makes an audio clip follow the tempo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    pub mode: WarpMode,
    /// The tempo the file was made at: where there are no markers, a beat of
    /// the song plays `60 / original_bpm` seconds of it.
    pub original_bpm: f64,
    /// The map, in time order. Never empty while warped: at least one marker
    /// says where the file sits against the clip.
    pub markers: Vec<Marker>,
    /// A transpose in cents (S6 sets it), 0 when none. Written only when set.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transpose_cents: i32,
}

fn is_zero(cents: &i32) -> bool {
    *cents == 0
}

impl Warp {
    /// A warp that plays a file made at `original_bpm`, with its second
    /// `source` at the clip's origin.
    pub fn new(original_bpm: f64, source: f64) -> Self {
        Warp {
            mode: WarpMode::default(),
            original_bpm,
            markers: vec![Marker::new(source, Ticks::ZERO)],
            transpose_cents: 0,
        }
    }

    /// The second of the file that plays `at` ticks from the clip's origin.
    pub fn source_at(&self, at: Ticks) -> f64 {
        warp::source_at(&self.markers, self.original_bpm, at)
    }

    /// The tick, from the origin, at which second `source` of the file plays.
    pub fn tick_at_source(&self, source: f64) -> Ticks {
        warp::tick_at_source(&self.markers, self.original_bpm, source)
    }

    /// The first tick, from the origin, at which the map reads the file
    /// (second 0 or later). Unlike [`Warp::tick_at_source`], which rounds to
    /// the nearest tick, this never lands before the file.
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

    /// The last tick, from the origin, at which the map has not read past a
    /// file of `total` seconds: where the file runs out, rounded down.
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

    /// Moves the whole map `by` ticks later.
    pub fn shift(&mut self, by: Ticks) {
        warp::shift(&mut self.markers, by);
    }

    /// This warp with its map moved `by` ticks later.
    pub fn shifted(&self, by: Ticks) -> Warp {
        let mut out = self.clone();
        out.shift(by);
        out
    }

    /// Whether a clip that starts `by` ticks after this one, with warp `next`,
    /// carries on this clip's audio without a break: the same map, seen from
    /// a clock `by` ticks later (what a split leaves).
    pub fn continues(&self, next: &Warp, by: Ticks) -> bool {
        self.shifted(Ticks::ZERO - by) == *next
    }

    /// Whether the numbers describe a warp a clip may hold.
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

/// No file is longer than this, and no marker lies further from the file's
/// start: a bound for the format.
const LONGEST_FILE_SECONDS: f64 = 1.0e7;
