mod taps;

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::app::track_freeze::FROZEN_TIMING;

use self::taps::{GAP, Tap, Taps};

#[derive(Default)]
pub(crate) struct TapRun {
    taps: Taps,
    at: Ticks,
    state: (u64, u64),
    seen: f64,
    pub(crate) next_time: Option<f64>,
}

impl TapRun {
    pub(crate) fn count(&self) -> usize {
        self.taps.count()
    }

    pub(crate) fn end(&mut self) {
        self.taps.end();
    }
}

impl App {
    pub(crate) fn tap_tempo(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let time = self.tap.next_time.take().unwrap_or(now);
        if !self.tap_run_open(ctx) {
            if self
                .document
                .project()
                .tracks
                .iter()
                .any(|t| t.frozen.is_some())
            {
                self.say(FROZEN_TIMING.to_owned());
                return;
            }
            self.tap.taps.end();
            self.tap.at = self.transport.state().ticks;
        }
        if let Tap::Tempo(bpm) = self.tap.taps.tap(time) {
            self.document.tap_tempo(self.tap.at, bpm);
        }
        self.tap.state = (self.document.session(), self.document.state());
        self.tap.seen = now;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(GAP));
    }

    pub(crate) fn tap_run_open(&mut self, ctx: &egui::Context) -> bool {
        if self.tap.count() == 0 {
            return false;
        }
        let now = ctx.input(|i| i.time);
        let left = self.tap.seen + GAP - now;
        let state = (self.document.session(), self.document.state());
        if left >= 0.0 && state == self.tap.state {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
            return true;
        }
        self.tap.taps.end();
        false
    }
}
