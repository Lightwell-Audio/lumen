//! A part's first half: new lists and live input, the clock, and handing
//! each hosted instrument its block.

use std::sync::atomic::Ordering;

use lumen_core::midi::Msg;

use crate::source::{Jump, Mode, RenderContext};

use super::super::capture::{self, Captured, Heard};
use super::super::hold::Clock;
use super::super::tracks::{gather_track, start_host};
use super::super::{MidiOut, Target};
use super::MidiPlayer;
use super::out::Out;

impl MidiPlayer {
    /// A part's first half: takes new lists and live input, places the live
    /// and clock messages, reads every track's clips and, with `hosts`, hands
    /// each hosted instrument its block. False for a scrub, which plays no
    /// MIDI.
    pub(super) fn prepare(
        &mut self,
        cx: &RenderContext<'_>,
        frames: usize,
        clock: Clock<'_>,
        hosts: bool,
    ) -> bool {
        self.take_new((cx.mode == Mode::Play).then_some((clock, cx.start)));
        self.take_feed();
        self.clock = cx.stream + frames as u64;
        if cx.mode == Mode::Scrub {
            // Live input waits until the scrub ends.
            return false;
        }
        self.play_live(cx, frames);
        if cx.mode == Mode::Play {
            self.play_clock(cx, frames);
        }
        for track in &mut self.current.tracks {
            gather_track(track, cx, frames, clock);
            if hosts {
                start_host(track, cx, frames);
            }
        }
        true
    }

    /// The loop wrapped: the next messages are of a new pass, and each
    /// recording track is told so.
    pub(super) fn wrapped(&mut self) {
        self.pass = self.pass.wrapping_add(1);
        if !self.recording {
            return;
        }
        for track in &self.current.tracks {
            if !track.record.is_on() {
                continue;
            }
            let marker = Captured {
                track: track.id,
                source: capture::WRAP,
                at: 0,
                pass: self.pass,
                frame: 0,
                state: Heard::Recording,
                msg: Msg([0, 0, 0]),
            };
            if self.capture.push(marker).is_err() {
                self.lost = self.lost.saturating_add(1);
            }
        }
    }

    /// Tells the sender to forget what it has not sent yet, if any track
    /// goes to hardware or any port gets the clock.
    pub(super) fn flush_hardware(&mut self) {
        let ports = self
            .current
            .tracks
            .iter()
            .any(|t| matches!(t.target, Target::Port { .. }));
        if ports || self.synced != 0 {
            self.out.push(MidiOut::Flush);
        }
    }

    /// Sends `msg` to every port in `ports` at device frame `frame`.
    pub(super) fn to_synced(out: &mut Out, ports: u64, frame: u64, msg: Msg) {
        let mut left = ports;
        while left != 0 {
            let port = left.trailing_zeros() as u16;
            out.at(frame, port, msg);
            left &= left - 1;
        }
    }

    /// The clock's messages for a part being played (M5 slice 7). Ports
    /// that turn Sync on while playing pick the song up from here.
    fn play_clock(&mut self, cx: &RenderContext<'_>, frames: usize) {
        let ports = self.sync.load(Ordering::Relaxed);
        if ports != self.synced {
            if ports == 0 {
                self.clock_out.reset();
            } else if self.clock_out.running() || self.synced == 0 {
                self.clock_out.locate(Jump::Play);
            }
            self.synced = ports;
        }
        if ports == 0 {
            return;
        }
        let (out, stream) = (&mut self.out, cx.stream);
        self.clock_out.part(
            cx.timeline,
            cx.sample_rate,
            cx.start,
            frames,
            |offset, msg| {
                Self::to_synced(out, ports, stream + u64::from(offset), msg);
            },
        );
    }
}
