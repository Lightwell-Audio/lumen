//! MIDI clock out (M5 slice 7, ADR-0062; owner's F3: only while playing).
//!
//! Ports with Sync on get 24 clocks a quarter note, each at the frame its
//! tick falls on through the tempo map (the tempo map's rounding, so within
//! a sample of it), with the transport messages hardware sequencers expect:
//!
//! - **Start** when playing from the song's start, **Song Position Pointer**
//!   then **Continue** from anywhere else;
//! - **Stop, SPP, Continue** at a loop wrap or a jump while playing;
//! - **Stop** on stop.
//!
//! A follower starts on the first clock after Start or Continue, at the
//! position the pointer gave. The pointer counts sixteenth notes (six
//! clocks), so from a place between two the clock waits for the next: the
//! first clock sent is the one at the pointer's position, and the
//! follower lines up exactly. The count-in sends nothing: the song, and
//! the hardware with it, starts after it.
//!
//! The generator is plain arithmetic on the tempo map, real-time safe and
//! tested on its own; the player hands each part to it and stamps what it
//! makes with device frames, like every message for hardware.

use lumen_core::midi::Msg;
use lumen_core::time::{PPQ, Ticks};

use crate::source::Jump;
use crate::timeline::Timeline;

/// Ticks between two clocks: a 24th of a quarter note.
pub const CLOCK_TICKS: i64 = PPQ / 24;
/// Clocks in a sixteenth note: the Song Position Pointer's step.
const PER_SIXTEENTH: i64 = 6;

/// What the next part begins with.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Restart {
    #[default]
    Nothing,
    /// Playback started: Start, or the pointer and Continue.
    Play,
    /// A jump while running: Stop, the pointer and Continue.
    Jump,
}

/// The clock's state between parts.
#[derive(Debug, Default)]
pub struct ClockOut {
    /// The next clock to send, counted from the song's start; `None` to
    /// find it from the next part's start (after a tempo change).
    next: Option<i64>,
    restart: Restart,
    /// Start or Continue went out, and Stop has not since.
    running: bool,
}

impl ClockOut {
    /// The transport moved: playback started, or jumped.
    pub fn locate(&mut self, jump: Jump) {
        match jump {
            Jump::Play => self.restart = Restart::Play,
            Jump::Locate | Jump::LoopWrap => {
                self.restart = if self.running {
                    Restart::Jump
                } else {
                    Restart::Play
                };
            }
            // The same musical place on another sample: clocks carry on.
            Jump::Reposition => {}
        }
        self.next = None;
    }

    /// Playback stopped: whether Stop goes out (it does if the clock was
    /// running).
    pub fn stop(&mut self) -> bool {
        self.restart = Restart::Nothing;
        self.next = None;
        std::mem::take(&mut self.running)
    }

    /// Forgets the clock without a Stop (no port has Sync on any more).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether Start or Continue went out and Stop has not since.
    pub fn running(&self) -> bool {
        self.running
    }

    /// The messages for the part from timeline sample `start`, `frames`
    /// long, each with its frame in the part. Real-time safe.
    pub fn part(
        &mut self,
        timeline: &Timeline,
        rate: u32,
        start: i64,
        frames: usize,
        mut emit: impl FnMut(u32, Msg),
    ) {
        let end = start + frames as i64;
        let sample_of = |k: i64| timeline.sample_at(Ticks(k * CLOCK_TICKS), rate);
        match std::mem::take(&mut self.restart) {
            Restart::Nothing if !self.running => return,
            Restart::Nothing => {
                if self.next.is_none() {
                    self.next = Some(first_clock(timeline, rate, start));
                }
            }
            restart => {
                // From the next sixteenth (or the song's start), where the
                // pointer can say the follower is.
                let k = first_clock(timeline, rate, start).max(0);
                let k = (k + PER_SIXTEENTH - 1) / PER_SIXTEENTH * PER_SIXTEENTH;
                if restart == Restart::Jump {
                    emit(0, Msg::STOP);
                }
                if k == 0 && restart == Restart::Play {
                    emit(0, Msg::START);
                } else {
                    let sixteenths = u16::try_from(k / PER_SIXTEENTH).unwrap_or(u16::MAX);
                    emit(0, Msg::song_position(sixteenths));
                    emit(0, Msg::CONTINUE);
                }
                self.running = true;
                self.next = Some(k);
            }
        }
        let Some(mut k) = self.next else {
            return;
        };
        loop {
            let at = sample_of(k);
            if at >= end {
                break;
            }
            emit((at - start).max(0) as u32, Msg::CLOCK);
            k += 1;
        }
        self.next = Some(k);
    }
}

/// The first clock at or after timeline sample `start`.
fn first_clock(timeline: &Timeline, rate: u32, start: i64) -> i64 {
    let sample_of = |k: i64| timeline.sample_at(Ticks(k * CLOCK_TICKS), rate);
    let t = timeline.ticks_at(start, rate).0;
    let mut k = t.div_euclid(CLOCK_TICKS) + i64::from(t.rem_euclid(CLOCK_TICKS) != 0);
    while sample_of(k) < start {
        k += 1;
    }
    while sample_of(k - 1) >= start {
        k -= 1;
    }
    k
}

#[cfg(test)]
mod tests {
    use lumen_core::meter::{Meter, MeterMap};
    use lumen_core::time::{TempoMap, TempoPoint};

    use super::*;

    const RATE: u32 = 48_000;

    /// Every message for `frames` from `start`, in parts of `block`, with
    /// its timeline sample.
    fn run(
        clock: &mut ClockOut,
        timeline: &Timeline,
        start: i64,
        frames: i64,
        block: i64,
    ) -> Vec<(i64, Msg)> {
        let mut out = Vec::new();
        let mut at = start;
        while at < start + frames {
            let n = block.min(start + frames - at);
            clock.part(timeline, RATE, at, n as usize, |offset, msg| {
                out.push((at + i64::from(offset), msg));
            });
            at += n;
        }
        out
    }

    fn clocks(messages: &[(i64, Msg)]) -> Vec<i64> {
        messages
            .iter()
            .filter(|(_, m)| *m == Msg::CLOCK)
            .map(|&(at, _)| at)
            .collect()
    }

    #[test]
    fn from_the_start_it_sends_start_then_24_clocks_a_quarter_on_their_samples() {
        let timeline = Timeline::default();
        let mut clock = ClockOut::default();
        clock.locate(Jump::Play);
        // One 4/4 bar at 120 BPM is two seconds; odd blocks change nothing.
        let messages = run(&mut clock, &timeline, 0, 2 * i64::from(RATE), 127);
        assert_eq!(messages[0], (0, Msg::START));
        let at = clocks(&messages);
        assert_eq!(at.len(), 96, "a bar of 4/4");
        for (k, &sample) in at.iter().enumerate() {
            assert_eq!(
                sample,
                timeline.sample_at(Ticks(k as i64 * CLOCK_TICKS), RATE)
            );
        }
        assert_eq!(
            at[1] - at[0],
            1_000,
            "120 BPM at 48 kHz: 1,000 samples a clock"
        );
        assert!(clock.stop(), "running: Stop goes out");
        assert!(!clock.stop(), "once");
    }

    #[test]
    fn clocks_follow_a_tempo_change_to_the_sample() {
        let tempo = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(4),
                bpm: 90.0,
                ramp: false,
            },
        ])
        .unwrap();
        let timeline = Timeline::new(tempo, MeterMap::constant(Meter::FOUR_FOUR).unwrap());
        let mut clock = ClockOut::default();
        clock.locate(Jump::Play);
        let end = timeline.sample_at(Ticks::quarters(8), RATE);
        let at = clocks(&run(&mut clock, &timeline, 0, end, 256));
        assert_eq!(at.len(), 8 * 24, "two bars, one at each tempo");
        for (k, &sample) in at.iter().enumerate() {
            assert_eq!(
                sample,
                timeline.sample_at(Ticks(k as i64 * CLOCK_TICKS), RATE)
            );
        }
        // 90 BPM at 48 kHz: 1,333⅓ samples a clock.
        assert!((1_333..=1_334).contains(&(at[100] - at[99])));
    }

    #[test]
    fn from_elsewhere_the_pointer_names_the_next_sixteenth_and_its_clock_comes_first() {
        let timeline = Timeline::default();
        let mut clock = ClockOut::default();
        clock.locate(Jump::Play);
        // A little after the second beat: the next sixteenth is the 5th
        // (beat 2 is sixteenth 4), at clock 30.
        let start = timeline.sample_at(Ticks::quarters(1), RATE) + 10;
        let messages = run(&mut clock, &timeline, start, 24_000, 512);
        assert_eq!(messages[0], (start, Msg::song_position(5)));
        assert_eq!(messages[1], (start, Msg::CONTINUE));
        let at = clocks(&messages);
        assert_eq!(at[0], timeline.sample_at(Ticks(30 * CLOCK_TICKS), RATE));
    }

    #[test]
    fn a_loop_wrap_or_jump_sends_stop_the_pointer_and_continue() {
        let timeline = Timeline::default();
        let mut clock = ClockOut::default();
        clock.locate(Jump::Play);
        let bar = 2 * i64::from(RATE);
        run(&mut clock, &timeline, 0, bar, 256);
        clock.locate(Jump::LoopWrap);
        let messages = run(&mut clock, &timeline, 0, 4_000, 256);
        assert_eq!(
            messages[..3],
            [
                (0, Msg::STOP),
                (0, Msg::song_position(0)),
                (0, Msg::CONTINUE)
            ]
        );
        assert_eq!(messages[3], (0, Msg::CLOCK));
        clock.locate(Jump::Locate);
        let at = timeline.sample_at(Ticks::quarters(8), RATE);
        let messages = run(&mut clock, &timeline, at, 100, 256);
        assert_eq!(messages[1], (at, Msg::song_position(32)));
    }

    #[test]
    fn a_tempo_change_while_playing_carries_the_count_on() {
        let timeline = Timeline::default();
        let mut clock = ClockOut::default();
        clock.locate(Jump::Play);
        let second = i64::from(RATE);
        let before = run(&mut clock, &timeline, 0, second, 256);
        clock.locate(Jump::Reposition);
        let after = run(&mut clock, &timeline, second, second, 256);
        assert!(after.iter().all(|(_, m)| *m == Msg::CLOCK), "no pointer");
        // Two seconds at 120 BPM: four quarters.
        assert_eq!(clocks(&before).len() + clocks(&after).len(), 96);
    }

    #[test]
    fn stopped_it_sends_nothing() {
        let timeline = Timeline::default();
        let mut clock = ClockOut::default();
        assert!(run(&mut clock, &timeline, 0, 4_800, 256).is_empty());
        assert!(!clock.stop());
    }
}
