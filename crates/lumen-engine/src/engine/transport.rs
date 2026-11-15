use crate::declick::{MAX_FADE, fade_frames};
use crate::metronome;
use crate::source::{Jump, Mode, Output, RenderContext, Source};

use super::Engine;

impl<S: Source> Engine<S> {
    pub(super) fn count_in(&mut self, data: &mut [f32], channels: usize, frames: usize) -> usize {
        let Some(mut count) = self.counting else {
            return 0;
        };
        let rate = self.sample_rate;
        let n = (frames as i64).min(count.left()).max(0) as usize;
        if self.source.ringing() {
            let cx = RenderContext {
                timeline: &self.timeline,
                sample_rate: rate,
                start: self.position,
                tail: false,
                stopping: false,
                mode: Mode::Ring,
                stream: self.stream,
                device: self.stream,
                recording: false,
                looping: None,
                deadline: self.deadline,
            };
            let mut out = Output::new(data, frames, 0, n, channels);
            self.source.render(&cx, &mut out);
        } else {
            self.source.idle(rate, n);
        }
        if self.declick.active() {
            self.declick.apply(data, frames, channels, 0, n);
        }
        let window = metronome::Window {
            stride: frames,
            channels,
            offset: 0,
            count: n,
        };
        self.metronome.count(&mut count, rate, data, window);
        if count.left() > 0 {
            self.counting = Some(count);
        } else {
            self.counting = None;
            self.start(channels);
        }
        n
    }

    pub(super) fn render(&mut self, data: &mut [f32], channels: usize, frames: usize, from: usize) {
        let latent = self.lookahead != 0;
        let mut offset = from;
        while offset < frames {
            let mut n = frames - offset;
            let wraps = self.loop_samples.filter(|&(_, end)| self.ahead < end);
            if let Some((_, end)) = wraps {
                n = n.min((end - self.ahead) as usize);
            }
            if latent && let Some(until) = self.playhead.until_jump() {
                n = n.min(until.max(1) as usize);
            }
            let cx = RenderContext {
                timeline: &self.timeline,
                sample_rate: self.sample_rate,
                start: self.ahead,
                tail: false,
                stopping: false,
                mode: Mode::Play,
                stream: self.stream + offset as u64 + self.lookahead.max(0) as u64,
                device: self.stream + offset as u64,
                recording: self.recording,
                looping: self.loop_samples,
                deadline: self.deadline,
            };
            let mut out = Output::new(data, frames, offset, n, channels);
            self.source.render(&cx, &mut out);
            self.declick.apply(data, frames, channels, offset, n);
            if !(latent && self.playhead.prerolling()) {
                self.metronome.render(
                    &self.timeline,
                    self.sample_rate,
                    self.position,
                    data,
                    metronome::Window {
                        stride: frames,
                        channels,
                        offset,
                        count: n,
                    },
                );
            }
            self.ahead += n as i64;
            if latent {
                self.playhead.advance(n as i64);
            }
            offset += n;
            if let Some((start, end)) = wraps
                && self.ahead == end
            {
                self.declicked(channels, |engine| {
                    engine.ahead = start;
                    engine.locate_source(Jump::LoopWrap);
                });
            }
            self.position = self.heard();
        }
    }

    pub(super) fn heard(&self) -> i64 {
        if self.lookahead == 0 {
            self.ahead
        } else {
            self.playhead.position()
        }
    }

    pub(super) fn declicked(&mut self, channels: usize, jump: impl FnOnce(&mut Self)) {
        let here = self.lookahead == 0;
        if here {
            self.capture_tail(channels);
        }
        jump(self);
        if here {
            self.declick.fade_in(fade_frames(self.sample_rate));
        } else {
            self.playhead.jump(self.ahead);
        }
    }

    pub(super) fn follow_latency(&mut self, channels: usize) {
        let lookahead = i64::from(self.source.latency());
        if lookahead == self.lookahead {
            return;
        }
        if !self.playing {
            self.lookahead = lookahead;
            return;
        }
        let playhead = self.position;
        let mut ahead = playhead + lookahead;
        if let Some((start, end)) = self.loop_samples
            && playhead < end
            && ahead >= end
        {
            ahead = start + (ahead - end) % (end - start);
        }
        if lookahead == 0 {
            self.capture_tail(channels);
        }
        self.lookahead = lookahead;
        self.playhead.resync(playhead, lookahead);
        self.ahead = ahead;
        self.locate_source(Jump::Locate);
        if lookahead == 0 {
            self.declick.fade_in(fade_frames(self.sample_rate));
        }
        self.playhead.jump(ahead);
        self.position = self.playhead.position();
    }

    pub(super) fn start(&mut self, channels: usize) {
        let ringing = self.source.ringing();
        if ringing && self.lookahead == 0 {
            self.render_tail(channels, Mode::Ring, false);
        }
        self.playing = true;
        self.locate_source(Jump::Play);
        self.lookahead = i64::from(self.source.latency());
        self.playhead.start(self.ahead, self.lookahead);
        self.position = self.playhead.position();
        if !ringing || self.lookahead == 0 {
            self.declick.fade_in(fade_frames(self.sample_rate));
        }
    }

    pub(super) fn halt(&mut self, channels: usize) {
        self.capture(channels, true);
        self.playing = false;
        self.recording = false;
        self.ahead = self.position;
        self.source.stopped();
    }

    pub(super) fn place(&mut self, sample: i64) {
        self.position = sample;
        self.ahead = sample;
    }

    pub(super) fn capture_tail(&mut self, channels: usize) {
        self.capture(channels, false);
    }

    fn capture(&mut self, channels: usize, stopping: bool) {
        if self.playing {
            self.render_tail(channels, Mode::Play, stopping);
        }
    }

    fn render_tail(&mut self, channels: usize, mode: Mode, stopping: bool) {
        let frames = fade_frames(self.sample_rate);
        let Some(buffer) = self.declick.tail_buffer(channels) else {
            return;
        };
        let cx = RenderContext {
            timeline: &self.timeline,
            sample_rate: self.sample_rate,
            start: self.ahead,
            tail: true,
            stopping,
            mode,
            stream: 0,
            device: 0,
            recording: false,
            looping: None,
            deadline: self.deadline,
        };
        let mut out = Output::new(buffer, MAX_FADE, 0, frames, channels);
        self.source.render(&cx, &mut out);
        self.declick.tail_rendered(channels, frames);
    }
}
