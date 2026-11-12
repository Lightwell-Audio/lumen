//! The player as a [`TrackSource`]: what the engine calls for a part, a
//! jump, a stop and a panic.

use std::sync::atomic::Ordering;

use lumen_core::midi::{Msg, cc};

use crate::mixer::{Buses, TrackSource};
use crate::source::{Jump, Mode, RenderContext};
use crate::timeline::Timeline;

use super::super::Target;
use super::super::hold::{self, Clock};
use super::super::tracks::{play_track, preview_instrument};
use super::MidiPlayer;

impl TrackSource for MidiPlayer {
    fn render(&mut self, cx: &RenderContext<'_>, buses: &mut Buses<'_>) {
        if cx.tail {
            if !cx.stopping {
                // Ahead of a jump: what rings goes on, faded out by the
                // engine while the new position fades in.
                let frames = buses.frames();
                for track in &mut self.current.tracks {
                    preview_instrument(track, track.id, (frames, cx.sample_rate), buses);
                }
            }
            return;
        }
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        // `begin` may have done the first half already.
        if !std::mem::take(&mut self.prepared) && !self.prepare(cx, buses.frames(), clock, false) {
            return;
        }
        for track in &mut self.current.tracks {
            play_track(track, cx, buses, &mut self.out);
        }
    }

    fn hosted(&self) -> bool {
        // A list waiting to be taken counts, so the callback that takes it
        // already has its deadline.
        self.hosted || self.hosted_pending.load(Ordering::Relaxed)
    }

    fn begin(&mut self, cx: &RenderContext<'_>, frames: usize) {
        if !self.hosted || cx.tail || cx.mode == Mode::Scrub {
            return;
        }
        let clock = Clock {
            timeline: cx.timeline,
            rate: cx.sample_rate,
        };
        self.prepared = self.prepare(cx, frames, clock, true);
    }

    fn locate(&mut self, timeline: &Timeline, sample_rate: u32, sample: i64, jump: Jump) {
        self.take_new(None);
        if jump == Jump::LoopWrap {
            self.wrapped();
        }
        self.wrapped_since_locate = jump == Jump::LoopWrap;
        self.clock_out.locate(jump);
        let clock = Clock {
            timeline,
            rate: sample_rate,
        };
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
            if track.sounds() {
                hold::chase_controls(
                    (&track.clips, &track.index),
                    &mut track.state,
                    channel,
                    clock,
                    sample,
                );
                hold::chase_notes(
                    (&track.clips, &track.index),
                    &mut track.state,
                    channel,
                    clock,
                    sample,
                );
            }
        }
    }

    fn idle(&mut self) {
        self.take_new(None);
        self.take_feed();
    }

    fn stopped(&mut self) {
        self.flush_hardware();
        if self.clock_out.stop() {
            Self::to_synced(&mut self.out, self.synced, self.clock, Msg::STOP);
        }
        for track in &mut self.current.tracks {
            let channel = track.channel();
            track.state.release_all(channel);
        }
    }

    fn panic(&mut self) {
        self.flush_hardware();
        let clock = self.clock;
        let mut seen = [u16::MAX; 32];
        let mut count = 0;
        for track in &mut self.current.tracks {
            let state = &mut track.state;
            state.held.clear();
            state.events.clear();
            state.sustain = false;
            state.bend = false;
            state.live.clear();
            state.down.clear();
            if let Some(spec) = track.instrument.as_mut() {
                spec.instrument.silence();
            }
            // An instrument taken away and still ringing out is silenced too;
            // it is kept (not dropped on this thread) and goes with its list.
            if let Some(spec) = track.leaving.as_mut() {
                spec.instrument.silence();
            }
            if let Target::Port { port, .. } = track.target
                && !seen[..count].contains(&port)
            {
                if count < seen.len() {
                    seen[count] = port;
                    count += 1;
                }
                for channel in 0..16 {
                    self.out
                        .at(clock, port, Msg::controller(channel, cc::ALL_NOTES_OFF, 0));
                    self.out
                        .at(clock, port, Msg::controller(channel, cc::ALL_SOUND_OFF, 0));
                }
            }
        }
    }

    fn ringing(&self) -> bool {
        self.live.as_ref().is_some_and(|l| l.waiting())
            || self.current.tracks.iter().any(|t| {
                !t.state.events.is_empty()
                    || t.instrument.as_ref().is_some_and(|i| i.instrument.active())
                    || t.leaving.as_ref().is_some_and(|i| i.instrument.active())
            })
    }
}
