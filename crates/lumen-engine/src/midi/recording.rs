//! Position and window mapping shared by live MIDI playback and capture.

use lumen_core::time::Ticks;

use super::MidiTrack;
use super::capture;
use super::hold::Clock;

/// The take's window in the pass that owns the compensated event position.
pub(super) fn record_window(track: &MidiTrack, pass: u32) -> (Ticks, Ticks) {
    let Some(timing) = track.record_timing else {
        return track.record_window;
    };
    let punch = timing.punch.unwrap_or((Ticks(i64::MIN), Ticks(i64::MAX)));
    match timing.looped {
        Some((from, to)) if pass == track.state.record_pass => {
            (timing.start.max(from).max(punch.0), to.min(punch.1))
        }
        Some((from, to)) => (from.max(punch.0), to.min(punch.1)),
        None => (timing.start.max(punch.0), punch.1),
    }
}

/// The position used to decide whether a live key masks playback. It mirrors
/// the app capture mapping, including the first pass floor and a compensated
/// event that belongs at the end of the previous loop pass.
pub(super) fn live_position(
    track: &MidiTrack,
    clock: Clock<'_>,
    heard_sample: i64,
    pass: u32,
    back: bool,
) -> (Ticks, u32) {
    let actual_pass = pass.wrapping_sub(u32::from(back));
    let heard = Ticks(clock.tick(heard_sample.max(0)));
    let Some(timing) = track.record_timing else {
        return (heard, actual_pass);
    };
    let compensated = Ticks(clock.tick(heard_sample.saturating_sub(timing.latency_samples).max(0)));
    capture::record_position(
        heard,
        compensated,
        actual_pass,
        track.state.record_pass,
        timing.start,
        timing.looped,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;

    use crate::midi::{MidiTrack, RecordTiming, Target};
    use crate::timeline::Timeline;

    #[test]
    fn an_already_held_key_uses_the_compensated_and_floored_start() {
        let timeline = Timeline::default();
        let clock = Clock {
            timeline: &timeline,
            rate: 48_000,
        };
        let start = Ticks(clock.tick(24_000));
        let mut track = MidiTrack::new(1, Target::Instrument, None, Vec::new()).recording_timing(
            Some(RecordTiming {
                start,
                latency_samples: 256,
                looped: None,
                punch: None,
            }),
        );
        track.state.record_pass = 3;

        // The held-key branch calls `live_position` at recording onset.
        // Read-ahead puts its heard position before Record start; the same
        // latency and floor as Capture bring it onto the visible boundary.
        let heard_sample = 23_900;
        let raw = Ticks(clock.tick(heard_sample));
        let (position, pass) = live_position(&track, clock, heard_sample, 3, false);
        let window = record_window(&track, pass);
        assert!(raw < window.0);
        assert!((window.0..window.1).contains(&position));
    }
}
