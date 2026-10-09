//! Meters on the UI side (M4 slice 5, ADR-0040): what each track's meter
//! shows, moved on from the levels the engine measured.
//!
//! The engine hands over each node's peaks since the last look, its RMS
//! and whether it clipped (`lumen_engine::Level`). Here they become what is
//! drawn, with the owner's B2 ballistics: a peak jumps up at once and falls
//! 20 dB a second; a hold line stays 1.5 s on the highest peak, then falls;
//! a clip stays lit until it is clicked. The master's loudness windows and
//! true peak are kept here too, reset when playback starts from stop (B5).
//!
//! Meters ask for frames only while something they show is moving, so a
//! stopped, silent project stays idle (risk R5).

use std::collections::HashMap;

use lumen_core::loudness::Loudness;
use lumen_engine::{Level, MixerHandle};

/// The bottom of every meter's scale: quieter reads as silence.
pub const FLOOR_DB: f32 = -60.0;
/// How fast a shown peak falls (the owner's B2).
pub const FALL_DB_PER_SECOND: f32 = 20.0;
/// How long the hold line stays on the highest peak before it falls.
pub const HOLD_SECONDS: f32 = 1.5;
/// How fast the hold line falls once its time is up.
const HOLD_FALL_DB_PER_SECOND: f32 = 30.0;
/// The longest step ballistics take at once (after an idle gap).
const MAX_STEP: f32 = 1.0;

/// What a meter shows, per side, in dB (`f32::NEG_INFINITY` for silence).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub peak: [f32; 2],
    pub rms: [f32; 2],
    /// The hold line.
    pub hold: [f32; 2],
    /// Went over full scale since the clip was last cleared.
    pub clipped: bool,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            peak: [f32::NEG_INFINITY; 2],
            rms: [f32::NEG_INFINITY; 2],
            hold: [f32::NEG_INFINITY; 2],
            clipped: false,
        }
    }
}

impl Reading {
    /// The louder side's hold, for a one-number peak readout.
    pub fn hold_db(&self) -> f32 {
        self.hold[0].max(self.hold[1])
    }

    /// Nothing above the floor: a meter at rest.
    pub fn at_rest(&self) -> bool {
        self.peak
            .iter()
            .chain(&self.rms)
            .chain(&self.hold)
            .all(|&db| db <= FLOOR_DB)
    }
}

pub fn db(linear: f32) -> f32 {
    if linear > 0.0 {
        20.0 * linear.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// One meter's ballistics.
#[derive(Debug, Clone, Copy, Default)]
struct Ballistics {
    shown: Reading,
    /// Seconds the hold line stays, per side.
    hold_left: [f32; 2],
}

impl Ballistics {
    /// Moves on by `dt` seconds with what the engine measured since the
    /// last step (`None`: the node is not routed any more).
    fn step(&mut self, level: Option<Level>, dt: f32) {
        let dt = dt.clamp(0.0, MAX_STEP);
        let level = level.unwrap_or_default();
        let shown = &mut self.shown;
        for side in 0..2 {
            let fresh = db(level.peak[side]);
            let fallen = shown.peak[side] - FALL_DB_PER_SECOND * dt;
            shown.peak[side] = settle(fresh.max(fallen));
            shown.rms[side] = settle(db(level.rms[side]));
            if shown.peak[side] >= shown.hold[side] {
                shown.hold[side] = shown.peak[side];
                self.hold_left[side] = HOLD_SECONDS;
            } else {
                self.hold_left[side] -= dt;
                if self.hold_left[side] <= 0.0 {
                    let fallen = shown.hold[side] - HOLD_FALL_DB_PER_SECOND * dt;
                    shown.hold[side] = settle(fallen.max(shown.peak[side]));
                }
            }
        }
        shown.clipped |= level.clipped;
    }
}

/// Below the floor is silence, so a meter comes to rest exactly.
fn settle(db: f32) -> f32 {
    if db <= FLOOR_DB {
        f32::NEG_INFINITY
    } else {
        db
    }
}

/// The master's loudness, as shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessReading {
    /// LUFS; `f64::NEG_INFINITY` before there is any.
    pub momentary: f64,
    pub short_term: f64,
    pub integrated: f64,
    /// dBTP since the last reset.
    pub true_peak: f32,
}

/// Every track's meter and the master's loudness.
#[derive(Debug)]
pub struct Meters {
    tracks: HashMap<u64, Ballistics>,
    loudness: Loudness,
    true_peak: f32,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            tracks: HashMap::new(),
            loudness: Loudness::new(),
            true_peak: f32::NEG_INFINITY,
        }
    }
}

impl Meters {
    /// Takes what the engine measured for `ids` (tracks and the master) and
    /// moves every meter on by `dt` seconds. Meters of tracks no longer in
    /// `ids` are dropped. While the audio is not `running` the engine
    /// measures nothing, so every meter falls to rest.
    pub fn update(&mut self, mixer: &mut MixerHandle, ids: &[u64], dt: f32, running: bool) {
        self.tracks.retain(|id, _| ids.contains(id));
        for &id in ids {
            let level = mixer.take_level(id).filter(|_| running);
            self.tracks.entry(id).or_default().step(level, dt);
        }
        let loudness = &mut self.loudness;
        mixer.take_loudness(|block| loudness.push(block));
        self.true_peak = self.true_peak.max(db(mixer.take_true_peak()));
    }

    /// What track `id`'s meter shows.
    pub fn reading(&self, id: u64) -> Reading {
        self.tracks.get(&id).map(|b| b.shown).unwrap_or_default()
    }

    /// Whether any meter is moving: the UI keeps drawing until they rest.
    pub fn moving(&self) -> bool {
        self.tracks.values().any(|b| !b.shown.at_rest())
    }

    /// Clears track `id`'s clip light and hold line (a click on it).
    pub fn clear_clip(&mut self, id: u64) {
        if let Some(b) = self.tracks.get_mut(&id) {
            b.shown.clipped = false;
            b.shown.hold = b.shown.peak;
        }
    }

    /// Clears every clip light (Alt+click, or the palette).
    pub fn clear_all_clips(&mut self) {
        for id in self.tracks.keys().copied().collect::<Vec<_>>() {
            self.clear_clip(id);
        }
    }

    /// Whether any track's clip light is on.
    pub fn any_clipped(&self) -> bool {
        self.tracks.values().any(|b| b.shown.clipped)
    }

    /// Starts integrated loudness and true peak again (Play from stop, or
    /// a click on the loudness column).
    pub fn reset_loudness(&mut self) {
        self.loudness.reset();
        self.true_peak = f32::NEG_INFINITY;
    }

    pub fn loudness(&self) -> LoudnessReading {
        LoudnessReading {
            momentary: self.loudness.momentary(),
            short_term: self.loudness.short_term(),
            integrated: self.loudness.integrated(),
            true_peak: self.true_peak,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(peak: f32, rms: f32, clipped: bool) -> Option<Level> {
        Some(Level {
            peak: [peak; 2],
            rms: [rms; 2],
            clipped,
        })
    }

    #[test]
    fn a_peak_jumps_up_then_falls_20_db_a_second() {
        let mut b = Ballistics::default();
        b.step(level(1.0, 0.5, false), 1.0 / 60.0);
        assert_eq!(b.shown.peak[0], 0.0);
        // Silence for half a second: down 10 dB.
        for _ in 0..30 {
            b.step(level(0.0, 0.0, false), 1.0 / 60.0);
        }
        assert!((b.shown.peak[0] + 10.0).abs() < 0.01, "{:?}", b.shown);
        // RMS is the engine's, already averaged.
        assert_eq!(b.shown.rms[0], f32::NEG_INFINITY);
    }

    #[test]
    fn the_hold_line_stays_a_moment_then_falls() {
        let mut b = Ballistics::default();
        b.step(level(0.5, 0.3, false), 0.0);
        let held = b.shown.hold[0];
        // 1.4 s later, still there.
        for _ in 0..14 {
            b.step(level(0.0, 0.0, false), 0.1);
        }
        assert_eq!(b.shown.hold[0], held);
        // Past 1.5 s it falls, never below the peak.
        for _ in 0..3 {
            b.step(level(0.0, 0.0, false), 0.1);
        }
        assert!(b.shown.hold[0] < held);
        assert!(b.shown.hold[0] >= b.shown.peak[0]);
    }

    #[test]
    fn a_clip_stays_until_cleared() {
        let mut meters = Meters::default();
        let mut b = Ballistics::default();
        b.step(level(1.2, 0.8, true), 0.016);
        for _ in 0..600 {
            b.step(level(0.0, 0.0, false), 0.016);
        }
        assert!(b.shown.clipped);
        meters.tracks.insert(7, b);
        assert!(meters.any_clipped());
        meters.clear_clip(7);
        assert!(!meters.reading(7).clipped);
        assert!(!meters.any_clipped());
    }

    #[test]
    fn meters_come_to_rest_so_the_ui_can_idle() {
        let mut meters = Meters::default();
        let mut b = Ballistics::default();
        b.step(level(1.0, 0.7, false), 0.0);
        meters.tracks.insert(1, b);
        assert!(meters.moving());
        // Peaks fall 60 dB in three seconds, and the hold after 1.5 s.
        for _ in 0..300 {
            meters
                .tracks
                .get_mut(&1)
                .unwrap()
                .step(level(0.0, 0.0, false), 1.0 / 60.0);
        }
        assert!(!meters.moving(), "{:?}", meters.reading(1));
        assert!(meters.reading(1).at_rest());
    }

    #[test]
    fn an_unrouted_track_reads_silence() {
        let meters = Meters::default();
        assert_eq!(meters.reading(99), Reading::default());
        let mut b = Ballistics::default();
        b.step(None, 0.016);
        assert!(b.shown.at_rest());
    }
}
