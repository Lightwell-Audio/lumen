use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::ParamBlock;
use crate::params::ParamInfo;

pub mod compressor;
pub mod limiter;

pub use compressor::Compressor;
pub use limiter::Limiter;

pub const REFERENCE_RATE: u32 = 48_000;

pub fn frames_of_ms(ms: f32) -> u32 {
    ((ms.max(0.0) / 1000.0) * REFERENCE_RATE as f32).round() as u32
}

pub(super) fn one_pole(hz: f32, rate: u32) -> f32 {
    if rate == 0 || hz <= 0.0 {
        return 0.0;
    }
    let x = -std::f32::consts::TAU * hz / rate as f32;
    (1.0 - x.exp()).clamp(0.0, 1.0)
}

pub(super) fn reach(seconds: f32, rate: u32) -> f32 {
    if rate == 0 || seconds <= 0.0 {
        return 1.0;
    }
    (1.0 - (-1.0 / (rate as f32 * seconds)).exp()).clamp(0.0, 1.0)
}

pub(super) fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

pub(super) const FLOOR_DB: f32 = -120.0;

pub(super) fn db_of(amplitude: f32) -> f32 {
    if amplitude <= 1e-6 {
        FLOOR_DB
    } else {
        (20.0 * amplitude.log10()).max(FLOOR_DB)
    }
}

pub(super) fn gain_of(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub(super) fn value(table: &[ParamInfo], id: u32, x: f32) -> f32 {
    table
        .iter()
        .find(|p| p.id == id)
        .map_or(0.0, |p| p.value(x))
}

pub(super) fn held_at_rest(events: &ParamBlock, id: u32) -> Option<f32> {
    for i in 0..events.len() {
        let (named, points) = events.queue(i);
        if named == id {
            return points.first().map(|&(_, value)| value as f32);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    pub reduction_db: f32,
    pub input_db: f32,
    pub output_db: f32,
}

#[derive(Debug, Clone)]
pub struct Reduction(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    reduction: AtomicU32,
    input: AtomicU32,
    output: AtomicU32,
}

const LEVEL_SCALE: f32 = 1000.0;
const LEVEL_MOST_DB: f32 = 48.0;

fn level_code(db: f32) -> u32 {
    ((db.clamp(FLOOR_DB, LEVEL_MOST_DB) - FLOOR_DB) * LEVEL_SCALE) as u32
}

fn level_of_code(code: u32) -> f32 {
    code as f32 / LEVEL_SCALE + FLOOR_DB
}

impl Default for Reduction {
    fn default() -> Self {
        Self::new()
    }
}

impl Reduction {
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            reduction: AtomicU32::new(0),
            input: AtomicU32::new(0),
            output: AtomicU32::new(0),
        }))
    }

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
            self.0
                .input
                .fetch_max(level_code(input_db), Ordering::Relaxed);
        }
    }

    pub fn record_output(&self, output_db: f32) {
        if output_db.is_finite() {
            self.0
                .output
                .fetch_max(level_code(output_db), Ordering::Relaxed);
        }
    }

    pub fn take(&self) -> Reading {
        Reading {
            reduction_db: f32::from_bits(self.0.reduction.swap(0, Ordering::Relaxed)),
            input_db: level_of_code(self.0.input.swap(0, Ordering::Relaxed)),
            output_db: level_of_code(self.0.output.swap(0, Ordering::Relaxed)),
        }
    }

    pub fn peek(&self) -> Reading {
        Reading {
            reduction_db: f32::from_bits(self.0.reduction.load(Ordering::Relaxed)),
            input_db: level_of_code(self.0.input.load(Ordering::Relaxed)),
            output_db: level_of_code(self.0.output.load(Ordering::Relaxed)),
        }
    }
}

pub(super) fn level_text(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{db:.1} dB").replace('-', "\u{2212}")
    }
}

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

pub(super) fn ms_text(ms: f32) -> String {
    if ms < 0.1 {
        format!("{ms:.2} ms")
    } else if ms < 100.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}

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
        assert!(
            (reading.input_db + 3.0).abs() < 0.01,
            "the level is the loudest since the last read: {}",
            reading.input_db
        );
        let again = meter.take();
        assert_eq!(again.reduction_db, 0.0);
        assert_eq!(again.input_db, FLOOR_DB, "a read starts the level afresh");
        meter.record(f32::NAN, f32::NAN);
        meter.record(-3.0, -20.0);
        assert_eq!(meter.take().reduction_db, 0.0);
    }

    #[test]
    fn the_output_level_is_the_loudest_since_the_last_read_and_peek_does_not_clear_it() {
        let meter = Reduction::new();
        assert_eq!(meter.peek().output_db, FLOOR_DB);
        meter.record_output(-9.0);
        meter.record_output(-2.5);
        meter.record_output(-30.0);
        meter.record_output(f32::NAN);
        assert!((meter.peek().output_db + 2.5).abs() < 0.01);
        assert!((meter.take().output_db + 2.5).abs() < 0.01);
        assert_eq!(meter.take().output_db, FLOOR_DB);
        meter.record_output(f32::NEG_INFINITY);
        meter.record_output(400.0);
        assert!(meter.take().output_db <= LEVEL_MOST_DB + 0.01);
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
