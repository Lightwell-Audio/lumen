//! Playing, stopping, jumping and everything that follows the playhead: the
//! count-in, loop wraps, latency and the click-free tails.

use crate::declick::{MAX_FADE, fade_frames};
use crate::metronome;
use crate::source::{Jump, Mode, Output, RenderContext, Source};

use super::Engine;

impl<S: Source> Engine<S> {
    /// The count-in's part of this block: the song waits at the play marker
    /// while the metronome clicks, and a live instrument still sounds. When
    /// the beats are done, playback starts mid-block and recording with it.
    /// Returns the frames used.
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

    /// Renders the block from frame `from` (earlier frames were a count-in).
    pub(super) fn render(&mut self, data: &mut [f32], channels: usize, frames: usize, from: usize) {
        // Without latency the playhead is the render position, and the
        // `Playhead` is left alone (the transport gate counts every step).
        let latent = self.lookahead != 0;
        let mut offset = from;
        while offset < frames {
            let mut n = frames - offset;
            // The render position wraps at the loop end if it is before it.
            let wraps = self.loop_samples.filter(|&(_, end)| self.ahead < end);
            if let Some((_, end)) = wraps {
                n = n.min((end - self.ahead) as usize);
            }
            // The playhead moves on smoothly up to the next jump heard.
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
            // After the crossfade, so a click on a loop start keeps its
            // attack; none before the start is heard.
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
                // Crossfade what would follow the loop end into the loop start.
                self.declicked(channels, |engine| {
                    engine.ahead = start;
                    engine.locate_source(Jump::LoopWrap);
                });
            }
            self.position = self.heard();
        }
    }

    /// The playhead while playing: the render position, or behind it.
    pub(super) fn heard(&self) -> i64 {
        if self.lookahead == 0 {
            self.ahead
        } else {
            self.playhead.position()
        }
    }

    /// Moves the render position with `jump`, crossfaded: here while the
    /// source is not latent, by the source otherwise. Only while playing.
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

    /// Follows a change in the source's latency. While playing, the
    /// playhead carries on and the render position moves to stay ahead of
    /// it by the new amount (wrapped into the loop), crossfaded.
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
        // Crossfaded by whichever side is not latent after the change.
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

    /// Playback starts rendering at `self.ahead`, fading in from silence.
    /// What still rings from before (an instrument's release after a Stop)
    /// carries on instead: fading the whole output in would cut the ringing
    /// note to nothing for a frame. Without latency it is crossfaded here,
    /// as a jump while playing is; a latent source carries it through its
    /// delay lines itself, and the first frames heard are from them.
    pub(super) fn start(&mut self, channels: usize) {
        let ringing = self.source.ringing();
        if ringing && self.lookahead == 0 {
            self.render_tail(channels, Mode::Ring, false);
        }
        self.playing = true;
        self.locate_source(Jump::Play);
        // Read after the source's locate, which takes up a waiting
        // routing.
        self.lookahead = i64::from(self.source.latency());
        self.playhead.start(self.ahead, self.lookahead);
        self.position = self.playhead.position();
        if !ringing || self.lookahead == 0 {
            self.declick.fade_in(fade_frames(self.sample_rate));
        }
    }

    /// Stops where the playhead is: playback continues from what was
    /// heard.
    pub(super) fn halt(&mut self, channels: usize) {
        self.capture(channels, true);
        self.playing = false;
        self.recording = false;
        self.ahead = self.position;
        self.source.stopped();
    }

    /// Places the stopped transport at `sample`.
    pub(super) fn place(&mut self, sample: i64) {
        self.position = sample;
        self.ahead = sample;
    }

    /// Renders what would follow the current position into the declick
    /// tail, faded out, ahead of a jump. Only while playing.
    pub(super) fn capture_tail(&mut self, channels: usize) {
        self.capture(channels, false);
    }

    /// As [`Self::capture_tail`]; `stopping` when playback is about to
    /// stop rather than jump (instruments carry on ringing after a stop,
    /// so they leave the tail out).
    fn capture(&mut self, channels: usize, stopping: bool) {
        if self.playing {
            self.render_tail(channels, Mode::Play, stopping);
        }
    }

    /// Renders the next frames from the render position in `mode` into the
    /// declick tail, faded out, without moving the source on.
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
