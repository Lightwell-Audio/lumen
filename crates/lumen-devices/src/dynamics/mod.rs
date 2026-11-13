//! Gravity, the compressor, and Horizon, the limiter (M8 slice B2).
//!
//! Both are built-ins: Lumen's own DSP, in-process on the audio thread, with no
//! allocation, lock or panic after their constructors (M8 §6 E1, E7; ADR-0145
//! is the path they plug into). Their parameters are a static table like every
//! built-in's, so the card's strip, All parameters, lanes, MIDI learn, overrides
//! and presets are the same code as a plugin's.
//!
//! Gravity is a feed-forward compressor whose detector reads either the peak or
//! the mean square of its input, or of a sidechain key (the engine's `Key`,
//! ADR-0041) through a high-pass filter; its gain computer is the standard
//! soft-knee curve, and its gain reduction is published to the interface
//! through a lock-free [`Reduction`] the card's face reads once a frame.
//!
//! Horizon is a look-ahead limiter: the signal is delayed by its lookahead, the
//! gain each frame needs is averaged over that same window, and the average is
//! what the delayed sample is played through, so nothing exceeds the ceiling.
//! Its lookahead is reported as latency (the routing compensates it) and as its
//! tail (the line drains after the input stops).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::ParamBlock;
use crate::params::ParamInfo;

pub mod compressor;
pub mod limiter;

pub use compressor::Compressor;
pub use limiter::Limiter;

/// The sample rate a device's latency and tail are counted at, in frames.
///
/// A [`crate::Description`]'s `latency` and `tail` are pure functions of the
/// values alone (ADR-0145, review H1), with no rate to ask: they are frames at
/// this reference rate, which is what the routing's delay compensation is in.
/// A device whose timing follows a time setting therefore delays by that many
/// frames at any rate, so compensation stays exact; only the millisecond a
/// round number of frames buys differs at another rate.
pub const REFERENCE_RATE: u32 = 48_000;

/// Frames a time in milliseconds is, at [`REFERENCE_RATE`].
pub fn frames_of_ms(ms: f32) -> u32 {
    ((ms.max(0.0) / 1000.0) * REFERENCE_RATE as f32).round() as u32
}

/// A one-pole's coefficient for a corner at `hz`, at `rate`.
pub(super) fn one_pole(hz: f32, rate: u32) -> f32 {
    if rate == 0 || hz <= 0.0 {
        return 0.0;
    }
    let x = -std::f32::consts::TAU * hz / rate as f32;
    (1.0 - x.exp()).clamp(0.0, 1.0)
}

/// How far a value reaches its target in one frame, for a time constant of
/// `seconds`: `1 - exp(-1 / (rate * seconds))`.
pub(super) fn reach(seconds: f32, rate: u32) -> f32 {
    if rate == 0 || seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-1.0 / (rate as f32 * seconds)).exp()).clamp(0.0, 1.0)
}

/// A tiny value becomes zero, so a decaying detector never reaches denormals.
pub(super) fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

/// A level in decibels, with silence at `FLOOR`.
pub(super) const FLOOR_DB: f32 = -120.0;

/// A linear amplitude as a level in decibels, floored at [`FLOOR_DB`].
pub(super) fn db_of(amplitude: f32) -> f32 {
    if amplitude <= 1e-6 {
        FLOOR_DB
    } else {
        (20.0 * amplitude.log10()).max(FLOOR_DB)
    }
}

/// A level in decibels as a linear amplitude.
pub(super) fn gain_of(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// The normalised value (0 to 1) of parameter `id` of `table` as its own units.
pub(super) fn value(table: &[ParamInfo], id: u32, x: f32) -> f32 {
    table
        .iter()
        .find(|p| p.id == id)
        .map_or(0.0, |p| p.value(x))
}

/// The value the project holds for parameter `id` in this block, unglided:
/// the last point of the last queue that names it, or `None` when the block
/// does not name it. A parameter that is a device's timing (the limiter's
/// lookahead) is read this way, so the frames it delays by are exactly the
/// frames the routing was told about, without a glide through another number.
pub(super) fn held(events: &ParamBlock, id: u32) -> Option<f32> {
    let mut found = None;
    for i in (0..events.len()).rev() {
        let (named, points) = events.queue(i);
        if named == id
            && let Some(&(_, value)) = points.last()
        {
            found = Some(value as f32);
            break;
        }
    }
    found
}

/// What a device's gain-reduction meter reads: the most reduction since the
/// interface last looked, and the level that drove it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    /// Gain reduction in decibels, 0 or more (0 is nothing taken away).
    pub reduction_db: f32,
    /// The detector's level when the reduction was greatest, in dBFS.
    pub input_db: f32,
}

/// The interface's end of a device's gain-reduction meter.
///
/// The audio side [`record`](Reduction::record)s each block (an atomic
/// read-modify-write, as the mixer's meter bank does), and the interface
/// [`take`](Reduction::take)s what it says once a frame. No lock, no queue to
/// drain, and a slow interface only misses detail, never the most reduction.
/// The handle allocates once, when the device is built on the interface side.
#[derive(Debug, Clone)]
pub struct Reduction(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    /// The most reduction since the last read, as `f32` bits: non-negative
    /// floats order like their bits.
    reduction: AtomicU32,
    /// The level the detector last saw, as `f32` bits.
    input: AtomicU32,
}

impl Default for Reduction {
    fn default() -> Self {
        Self::new()
    }
}

impl Reduction {
    /// A meter nothing is written to yet. Allocates: build it on the
    /// interface side.
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            reduction: AtomicU32::new(0),
            input: AtomicU32::new(FLOOR_DB.to_bits()),
        }))
    }

    /// Records what the block did: `reduction_db` decibels taken away (0 or
    /// more) at a detector level of `input_db`. Real-time safe.
    pub fn record(&self, reduction_db: f32, input_db: f32) {
        let reduction = if reduction_db.is_finite() {
            reduction_db.max(0.0)
        } else {
            0.0
        };
        if reduction > 0.0 {
            self.0
                .reduction
                .fetch_max(reduction.to_bits(), Ordering::Relaxed);
        }
        if input_db.is_finite() {
            self.0.input.store(input_db.to_bits(), Ordering::Relaxed);
        }
    }

    /// What the meter says now; reading starts the most-reduction count again.
    pub fn take(&self) -> Reading {
        Reading {
            reduction_db: f32::from_bits(self.0.reduction.swap(0, Ordering::Relaxed)),
            input_db: f32::from_bits(self.0.input.load(Ordering::Relaxed)),
        }
    }

    /// What the meter says, without reading it: for a picture drawn twice in
    /// one frame.
    pub fn peek(&self) -> Reading {
        Reading {
            reduction_db: f32::from_bits(self.0.reduction.load(Ordering::Relaxed)),
            input_db: f32::from_bits(self.0.input.load(Ordering::Relaxed)),
        }
    }
}

/// A level's text, always signed: "−18.0 dB" for a threshold or a ceiling.
pub(super) fn level_text(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:.1} dB").replace('-', "\u{2212}")
    }
}

/// A number in `text`, with any of `units` after it and either minus.
pub(super) fn number_in(text: &str, units: &[&str]) -> Option<f32> {
    let mut t = text.trim().replace('\u{2212}', "-").to_lowercase();
    for unit in units {
        if let Some(rest) = t.strip_suffix(unit) {
            t = rest.trim_end().to_owned();
        }
    }
    t.trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A switch's value read from its own word, or from 0 and 1.
pub(super) fn word_of(text: &str, choices: &[&str]) -> Option<f32> {
    let t = text.trim();
    if let Some(index) = choices.iter().position(|c| c.eq_ignore_ascii_case(t)) {
        return Some(if index > 0 { 1.0 } else { 0.0 });
    }
    match t {
        "0" => Some(0.0),
        "1" => Some(1.0),
        _ => None,
    }
}

/// Milliseconds, as a person reads them: "0.05 ms", "5.0 ms", "120 ms".
pub(super) fn ms_text(ms: f32) -> String {
    if ms < 0.1 {
        format!("{ms:.2} ms")
    } else if ms < 100.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

/// Milliseconds from what a person typed: "120", "120 ms", "0.5".
pub(super) fn ms_parse(text: &str) -> Option<f32> {
    number_in(text, &["ms", "milliseconds"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_holds_the_most_reduction_until_it_is_read() {
        let meter = Reduction::new();
        assert_eq!(meter.take().reduction_db, 0.0);
        meter.record(2.0, -6.0);
        meter.record(4.5, -3.0);
        meter.record(1.0, -12.0);
        let reading = meter.take();
        assert_eq!(reading.reduction_db, 4.5);
        assert_eq!(reading.input_db, -12.0, "the level is the last one");
        // Read: the count starts again.
        assert_eq!(meter.take().reduction_db, 0.0);
        // Nonsense never reaches the meter.
        meter.record(f32::NAN, f32::NAN);
        meter.record(-3.0, -20.0);
        assert_eq!(meter.take().reduction_db, 0.0);
    }

    #[test]
    fn a_time_in_frames_is_counted_at_the_reference_rate() {
        assert_eq!(frames_of_ms(1.5), 72);
        assert_eq!(frames_of_ms(0.0), 0);
        assert_eq!(frames_of_ms(10.0), 480);
        assert_eq!(frames_of_ms(-4.0), 0);
    }

    #[test]
    fn a_level_and_a_gain_are_each_other() {
        assert!((gain_of(-6.0) - 0.501_187).abs() < 1e-5);
        assert!((db_of(gain_of(-6.0)) + 6.0).abs() < 1e-4);
        assert_eq!(db_of(0.0), FLOOR_DB);
        assert_eq!(gain_of(FLOOR_DB - 1.0), 0.0);
    }

    #[test]
    fn a_level_reads_and_is_read_back_in_both_units() {
        assert_eq!(level_text(-18.0), "\u{2212}18.0 dB");
        assert_eq!(ms_text(5.0), "5.0 ms");
        assert_eq!(ms_text(0.05), "0.05 ms");
        assert_eq!(ms_text(120.0), "120 ms");
        assert_eq!(ms_parse("120 ms"), Some(120.0));
        assert_eq!(ms_parse("\u{2212}3"), Some(-3.0));
        assert_eq!(word_of("on", &["Off", "On"]), Some(1.0));
        assert_eq!(word_of("loud", &["Off", "On"]), None);
    }
}
