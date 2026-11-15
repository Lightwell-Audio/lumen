use std::collections::HashMap;

use lumen_core::loudness::Loudness;
use lumen_engine::{InputHandle, InputLevel, Level, MixerHandle};

pub const FLOOR_DB: f32 = -60.0;
pub const FALL_DB_PER_SECOND: f32 = 20.0;
pub const HOLD_SECONDS: f32 = 1.5;
const HOLD_FALL_DB_PER_SECOND: f32 = 30.0;
const MAX_STEP: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub peak: [f32; 2],
    pub rms: [f32; 2],
    pub hold: [f32; 2],
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
    pub fn hold_db(&self) -> f32 {
        self.hold[0].max(self.hold[1])
    }

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

#[derive(Debug, Clone, Copy, Default)]
struct Ballistics {
    shown: Reading,
    hold_left: [f32; 2],
}

impl Ballistics {
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

fn settle(db: f32) -> f32 {
    if db <= FLOOR_DB {
        f32::NEG_INFINITY
    } else {
        db
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessReading {
    pub momentary: f64,
    pub short_term: f64,
    pub integrated: f64,
    pub true_peak: f32,
}

#[derive(Debug)]
pub struct Meters {
    tracks: HashMap<u64, Ballistics>,
    inputs: HashMap<usize, Ballistics>,
    loudness: Loudness,
    true_peak: f32,
    history: std::collections::VecDeque<f64>,
}

pub const HISTORY: usize = 600;

impl Default for Meters {
    fn default() -> Self {
        Self {
            tracks: HashMap::new(),
            inputs: HashMap::new(),
            loudness: Loudness::new(),
            true_peak: f32::NEG_INFINITY,
            history: std::collections::VecDeque::with_capacity(HISTORY),
        }
    }
}

impl Meters {
    pub fn update(&mut self, mixer: &mut MixerHandle, ids: &[u64], dt: f32, running: bool) {
        self.tracks.retain(|id, _| ids.contains(id));
        for &id in ids {
            let level = mixer.take_level(id).filter(|_| running);
            self.tracks.entry(id).or_default().step(level, dt);
        }
        let (loudness, history) = (&mut self.loudness, &mut self.history);
        mixer.take_loudness(|block| {
            loudness.push(block);
            if history.len() == HISTORY {
                history.pop_front();
            }
            history.push_back(loudness.short_term());
        });
        self.true_peak = self.true_peak.max(db(mixer.take_true_peak()));
    }

    pub fn update_inputs(&mut self, handle: &InputHandle, open: &[usize], dt: f32, running: bool) {
        self.inputs.retain(|channel, _| open.contains(channel));
        for &channel in open {
            let InputLevel { peak, rms, clipped } = handle.take_level(channel);
            let level = running.then_some(Level {
                peak: [peak; 2],
                rms: [rms; 2],
                clipped,
            });
            self.inputs.entry(channel).or_default().step(level, dt);
        }
    }

    pub fn input_reading(&self, channel: usize) -> Reading {
        self.inputs
            .get(&channel)
            .map(|b| b.shown)
            .unwrap_or_default()
    }

    pub fn clear_input_clip(&mut self, channel: usize) {
        if let Some(b) = self.inputs.get_mut(&channel) {
            b.shown.clipped = false;
            b.shown.hold = b.shown.peak;
        }
    }

    pub fn reading(&self, id: u64) -> Reading {
        self.tracks.get(&id).map(|b| b.shown).unwrap_or_default()
    }

    pub fn moving(&self) -> bool {
        self.tracks
            .values()
            .chain(self.inputs.values())
            .any(|b| !b.shown.at_rest())
    }

    pub fn clear_clip(&mut self, id: u64) {
        if let Some(b) = self.tracks.get_mut(&id) {
            b.shown.clipped = false;
            b.shown.hold = b.shown.peak;
        }
    }

    pub fn clear_all_clips(&mut self) {
        for id in self.tracks.keys().copied().collect::<Vec<_>>() {
            self.clear_clip(id);
        }
        for channel in self.inputs.keys().copied().collect::<Vec<_>>() {
            self.clear_input_clip(channel);
        }
    }

    pub fn any_clipped(&self) -> bool {
        self.tracks.values().any(|b| b.shown.clipped)
    }

    pub fn reset_loudness(&mut self) {
        self.loudness.reset();
        self.true_peak = f32::NEG_INFINITY;
    }

    pub fn history(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        self.history.iter().copied()
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
        for _ in 0..30 {
            b.step(level(0.0, 0.0, false), 1.0 / 60.0);
        }
        assert!((b.shown.peak[0] + 10.0).abs() < 0.01, "{:?}", b.shown);
        assert_eq!(b.shown.rms[0], f32::NEG_INFINITY);
    }

    #[test]
    fn the_hold_line_stays_a_moment_then_falls() {
        let mut b = Ballistics::default();
        b.step(level(0.5, 0.3, false), 0.0);
        let held = b.shown.hold[0];
        for _ in 0..14 {
            b.step(level(0.0, 0.0, false), 0.1);
        }
        assert_eq!(b.shown.hold[0], held);
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
    #[test]
    fn input_meters_follow_the_open_inputs_and_rest_without_signal() {
        let (handle, _player) = lumen_engine::input_player();
        let mut meters = Meters::default();
        meters.update_inputs(&handle, &[0, 1], 0.016, true);
        assert!(!meters.moving());
        assert_eq!(meters.input_reading(1), Reading::default());
        meters
            .inputs
            .entry(5)
            .or_default()
            .step(level(1.0, 0.5, true), 0.0);
        assert!(meters.moving() && meters.input_reading(5).clipped);
        meters.update_inputs(&handle, &[0, 1], 0.016, true);
        assert!(!meters.input_reading(5).clipped);
    }
}
