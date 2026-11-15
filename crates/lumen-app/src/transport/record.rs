use lumen_core::time::{PPQ, Ticks};
use lumen_engine::{Timeline, TransportSnapshot};

use crate::commands::Command as Run;
use crate::recording::record_settings::CountIn;
use crate::timefmt::{self, TimeFormat};

const SIXTEENTH: i64 = PPQ / 4;

pub const REST_INK: f32 = 0.9;
const PULSE_FLOOR: f32 = 0.3;
const STEADY_TINT: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Rest,
    CountingIn,
    Recording,
}

pub fn phase(state: &TransportSnapshot) -> Phase {
    if state.count_in_total > 0 {
        Phase::CountingIn
    } else if state.recording {
        Phase::Recording
    } else {
        Phase::Rest
    }
}

pub fn beat_phase(left: i64, total: i64, beats: u32) -> Option<f32> {
    if total <= 0 || beats == 0 {
        return None;
    }
    let done = (total - left).clamp(0, total) as f64;
    let beats_done = done / total as f64 * f64::from(beats);
    Some(beats_done.fract() as f32)
}

pub fn count_in_lit(left: i64, total: i64, beats: u32, reduced: bool) -> f32 {
    if reduced {
        return STEADY_TINT;
    }
    match beat_phase(left, total, beats) {
        Some(phase) => PULSE_FLOOR + (1.0 - PULSE_FLOOR) * (1.0 - phase).powi(2),
        None => STEADY_TINT,
    }
}

pub fn count_in_ticks(timeline: &Timeline, marker: Ticks, left: i64, rate: u32) -> Ticks {
    if rate == 0 {
        return marker;
    }
    timeline.ticks_at(timeline.sample_at(marker, rate) - left, rate)
}

pub fn count_in_text(timeline: &Timeline, t: Ticks) -> String {
    let bbt = timeline.meter.bbt_at(t);
    let bar = if bbt.bar < 0 { bbt.bar } else { bbt.bar + 1 };
    format!("{bar}.{}.{}", bbt.beat + 1, bbt.ticks.0 / SIXTEENTH + 1)
}

pub fn time_text(format: TimeFormat, seconds: f64, rate: u32) -> String {
    if seconds < 0.0 {
        format!("-{}", timefmt::format(format, -seconds, rate))
    } else {
        timefmt::format(format, seconds, rate)
    }
}

pub fn count_in_name(count_in: CountIn) -> &'static str {
    match count_in {
        CountIn::Off => "Count-in: Off",
        CountIn::OneBar => "Count-in: 1 bar",
        CountIn::TwoBars => "Count-in: 2 bars",
        CountIn::FourBars => "Count-in: 4 bars",
    }
}

pub fn count_in_command(count_in: CountIn) -> Run {
    match count_in {
        CountIn::Off => Run::CountInOff,
        CountIn::OneBar => Run::CountInOneBar,
        CountIn::TwoBars => Run::CountInTwoBars,
        CountIn::FourBars => Run::CountInFourBars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::meter::Meter;

    fn t44() -> Timeline {
        Timeline::constant(120.0, Meter::FOUR_FOUR)
    }

    #[test]
    fn the_phase_is_counting_in_first_then_recording_then_rest() {
        let mut state = TransportSnapshot::default();
        assert_eq!(phase(&state), Phase::Rest);
        state.recording = true;
        assert_eq!(phase(&state), Phase::Recording);
        state.count_in_total = 88_200;
        state.count_in_left = 80_000;
        assert_eq!(phase(&state), Phase::CountingIn);
    }

    #[test]
    fn the_count_in_text_counts_up_through_negative_bars_with_no_bar_zero() {
        let tl = t44();
        assert_eq!(count_in_text(&tl, Ticks::quarters(-8)), "-2.1.1");
        assert_eq!(count_in_text(&tl, Ticks::quarters(-5)), "-2.4.1");
        assert_eq!(count_in_text(&tl, Ticks::quarters(-4)), "-1.1.1");
        assert_eq!(count_in_text(&tl, Ticks::quarters(-1)), "-1.4.1");
        assert_eq!(
            count_in_text(&tl, Ticks::quarters(-1) + Ticks(SIXTEENTH * 3)),
            "-1.4.4"
        );
        assert_eq!(count_in_text(&tl, Ticks::ZERO), "1.1.1");
    }

    #[test]
    fn the_count_in_position_moves_back_by_the_frames_left() {
        let tl = t44();
        let rate = 48_000;
        let beat_frames = 24_000;
        let at_start = count_in_ticks(&tl, Ticks::ZERO, 4 * beat_frames, rate);
        assert_eq!(count_in_text(&tl, at_start), "-1.1.1");
        let one_beat_left = count_in_ticks(&tl, Ticks::ZERO, beat_frames, rate);
        assert_eq!(count_in_text(&tl, one_beat_left), "-1.4.1");
        let none_left = count_in_ticks(&tl, Ticks::ZERO, 0, rate);
        assert_eq!(none_left, Ticks::ZERO);
    }

    #[test]
    fn the_count_in_time_has_a_minus_sign_before_zero() {
        assert_eq!(
            time_text(TimeFormat::MinSec, -2.0, 48_000),
            "-0:02.000",
            "a count-in reads as time to go"
        );
        assert_eq!(time_text(TimeFormat::MinSec, 1.5, 48_000), "0:01.500");
        assert_eq!(time_text(TimeFormat::Samples, 0.0, 48_000), "0");
    }

    #[test]
    fn the_beat_phase_rises_through_each_beat() {
        let total = 96;
        assert_eq!(beat_phase(96, total, 4), Some(0.0));
        assert_eq!(beat_phase(72, total, 4), Some(0.0), "the next beat");
        let half = beat_phase(84, total, 4).unwrap();
        assert!((half - 0.5).abs() < 1e-6, "{half}");
        assert_eq!(beat_phase(0, total, 4).map(|p| p < 1.0), Some(true));
        assert_eq!(beat_phase(0, 0, 4), None, "no count-in");
        assert_eq!(beat_phase(10, total, 0), None, "no beats");
    }

    #[test]
    fn the_pulse_is_brightest_on_a_beat_and_steady_with_reduce_motion() {
        let (total, beats) = (96, 4);
        let on_beat = count_in_lit(96, total, beats, false);
        let late_in_beat = count_in_lit(72 + 1, total, beats, false);
        assert!((on_beat - 1.0).abs() < 1e-6, "{on_beat}");
        assert!(late_in_beat < on_beat && late_in_beat >= PULSE_FLOOR);
        assert_eq!(count_in_lit(60, total, beats, true), STEADY_TINT);
        assert_eq!(count_in_lit(60, 0, beats, false), STEADY_TINT);
    }

    #[test]
    fn each_count_in_choice_runs_its_command_and_names_itself() {
        assert_eq!(count_in_command(CountIn::Off), Run::CountInOff);
        assert_eq!(count_in_command(CountIn::FourBars), Run::CountInFourBars);
        assert_eq!(count_in_name(CountIn::TwoBars), "Count-in: 2 bars");
    }
}
