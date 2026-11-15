use lumen_core::time::Ticks;

use super::{COMMAND_CAPACITY, Engine};
use crate::metronome::{CountIn, MetronomeSettings};
use crate::scrub::ScrubStyle;
use crate::source::{Jump, Source};

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Play,
    Continue,
    Pause,
    Stop,
    Locate(Ticks),
    Record { count_in: u32 },
    EndRecord,
    SetLoop(Region),
    SetPunch(Region),
    SetMetronome(MetronomeSettings),
    TestTone(bool),
    Scrub { at: Ticks, style: ScrubStyle },
    EndScrub,
    Panic,
}

impl<S: Source> Engine<S> {
    pub(super) fn apply_commands(&mut self, channels: usize) {
        for _ in 0..COMMAND_CAPACITY {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            match command {
                Command::Play | Command::Continue => {
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

    fn cancel_count_in(&mut self) -> bool {
        let was = self.counting.take().is_some();
        if was {
            self.recording = false;
        }
        was
    }
}
