//! Live transport commands and the loop and punch regions they set.

use lumen_core::time::Ticks;

use super::{COMMAND_CAPACITY, Engine};
use crate::metronome::{CountIn, MetronomeSettings};
use crate::scrub::ScrubStyle;
use crate::source::{Jump, Source};

/// A loop or punch range. It takes effect only when enabled and not empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    pub start: Ticks,
    pub end: Ticks,
    pub enabled: bool,
}

impl Region {
    pub fn new(start: Ticks, end: Ticks, enabled: bool) -> Self {
        Self {
            start,
            end,
            enabled,
        }
    }

    pub(super) fn active(self) -> bool {
        self.enabled && self.end > self.start
    }
}

/// Live transport commands. Each takes effect at the start of the next
/// block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Play from the play marker.
    Play,
    /// Play from the playhead, where playback last stopped.
    Continue,
    /// Stop where playback is.
    Pause,
    /// Stop where playback is; when already stopped, return to the start.
    Stop,
    Locate(Ticks),
    /// Play from the play marker and record (M5 slice 6): MIDI tracks that
    /// record keep what arrives, with its timeline position. First the song
    /// waits at the marker for `count_in` metronome beats (0 for none),
    /// clicking whether or not the metronome is on. While already playing,
    /// recording starts at once. Stop, Pause, Locate while counting and
    /// Panic end it.
    Record {
        count_in: u32,
    },
    /// Stop recording and carry on playing.
    EndRecord,
    SetLoop(Region),
    SetPunch(Region),
    SetMetronome(MetronomeSettings),
    /// 1 kHz at −20 dBFS on every output, playing or not.
    TestTone(bool),
    /// While stopped: play around `at` in `style`, and move the play
    /// marker and the playhead there. Ignored while playing.
    Scrub {
        at: Ticks,
        style: ScrubStyle,
    },
    /// Fade the scrub out.
    EndScrub,
    /// Stop at once and silence everything: playback, the metronome, the
    /// test tone, scrubbing and any fade in progress. MIDI notes are cut too (M5 slice 1).
    Panic,
}

impl<S: Source> Engine<S> {
    pub(super) fn apply_commands(&mut self, channels: usize) {
        // Bounded, so a flood of commands cannot exhaust the block's budget.
        for _ in 0..COMMAND_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            match command {
                Command::Play | Command::Continue => {
                    // A count-in already leads to playing.
                    if !self.playing && self.counting.is_none() {
                        self.scrub.end();
                        self.ahead = if command == Command::Play {
                            self.sample_at(self.play_marker)
                        } else {
                            self.position
                        };
                        self.start(channels);
                    }
                }
                Command::Record { count_in } => self.record(count_in, channels),
                Command::EndRecord => self.recording = false,
                Command::Pause => {
                    if self.cancel_count_in() {
                    } else if self.playing {
                        self.halt(channels);
                    }
                }
                Command::Stop => {
                    if self.cancel_count_in() {
                    } else if self.playing {
                        self.halt(channels);
                    } else {
                        self.play_marker = Ticks::ZERO;
                        self.place(0);
                    }
                }
                Command::Locate(at) => {
                    self.play_marker = at;
                    let sample = self.sample_at(at);
                    self.cancel_count_in();
                    if self.playing {
                        self.declicked(channels, |engine| {
                            engine.ahead = sample;
                            engine.locate_source(Jump::Locate);
                        });
                        self.position = self.heard();
                    } else {
                        self.place(sample);
                    }
                }
                Command::SetLoop(region) => {
                    self.loop_region = region;
                    self.refresh_regions();
                }
                Command::SetPunch(region) => {
                    self.punch = region;
                    self.refresh_regions();
                }
                Command::Scrub { at, style } => {
                    if !self.playing {
                        let at = at.max(Ticks::ZERO);
                        self.play_marker = at;
                        self.place(self.sample_at(at));
                        let end = match style {
                            ScrubStyle::Loop { length } => self.sample_at(at + length),
                            ScrubStyle::Tape => self.position,
                        };
                        self.scrub
                            .scrub(self.position, end, style, self.sample_rate);
                    }
                }
                Command::EndScrub => self.scrub.end(),
                Command::SetMetronome(settings) => self.metronome.settings = settings,
                Command::TestTone(on) => self.test_tone.on = on,
                Command::Panic => {
                    self.playing = false;
                    self.recording = false;
                    self.counting = None;
                    self.ahead = self.position;
                    self.declick.clear();
                    self.metronome.silence();
                    self.test_tone.silence();
                    self.scrub.silence();
                    self.source.panic();
                }
            }
        }
    }

    /// Starts recording (Command::Record).
    fn record(&mut self, count_in: u32, channels: usize) {
        if self.playing {
            self.recording = true;
        } else if self.counting.is_none() {
            self.scrub.end();
            let marker = self.sample_at(self.play_marker);
            self.recording = true;
            if count_in == 0 {
                self.ahead = marker;
                self.start(channels);
                return;
            }
            self.place(marker);
            let meter = self.timeline.meter.meter_at(self.play_marker);
            let beat = self.sample_at(self.play_marker + meter.beat()) - marker;
            self.counting = Some(CountIn {
                beat: beat.max(1),
                beats: count_in,
                per_bar: meter.numerator.max(1),
                done: 0,
            });
        }
    }

    /// Ends a count-in and the recording that waits behind it. Whether
    /// there was one.
    fn cancel_count_in(&mut self) -> bool {
        let was = self.counting.take().is_some();
        if was {
            self.recording = false;
        }
        was
    }
}
