//! Tap tempo in the app (M9 R7): the Tap piece and the command time each tap
//! by frame time and give it to the model (`crate::tap_tempo`); from the
//! third tap the tempo of the segment where the run started follows, as one
//! undo step per run. The run ends after 3 s without a tap or at any other
//! edit, and `sync_document` keeps the step open while it lasts.

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::app::track_freeze::FROZEN_TIMING;
use crate::tap_tempo::{GAP, Tap, Taps};

/// The run in progress, between frames.
#[derive(Default)]
pub(crate) struct TapRun {
    taps: Taps,
    /// Where the run started: it edits the segment in force there.
    at: Ticks,
    /// The document as the run left it: another edit ends the run.
    state: u64,
    /// The frame time of the last tap, for the 3 s gap.
    seen: f64,
    /// A tap's time given by a test instead of frame time.
    pub(crate) next_time: Option<f64>,
}

impl TapRun {
    /// Taps in the run, for the bar's counter; 0 with no run.
    pub(crate) fn count(&self) -> usize {
        self.taps.count()
    }
}

impl App {
    /// The Tap tempo command: one tap, timed now.
    pub(crate) fn tap_tempo(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let time = self.tap.next_time.take().unwrap_or(now);
        if !self.tap_run_open(ctx) {
            // A new run: refused at once while a track is frozen, since every
            // tempo it set would be.
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
            // The step of the run before was sealed when it ended (the
            // frame after, in `sync_document`).
            self.tap.taps.end();
            self.tap.at = self.transport.state().ticks;
        }
        if let Tap::Tempo(bpm) = self.tap.taps.tap(time) {
            self.document.tap_tempo(self.tap.at, bpm);
        }
        self.tap.state = self.document.state();
        self.tap.seen = now;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(GAP));
    }

    /// Whether a tap run is open; one that has lapsed (3 s without a tap)
    /// or that another edit ended is forgotten here, so the counter goes
    /// back to "Tap" and the next tap starts a new run.
    pub(crate) fn tap_run_open(&mut self, ctx: &egui::Context) -> bool {
        if self.tap.count() == 0 {
            return false;
        }
        let now = ctx.input(|i| i.time);
        let left = self.tap.seen + GAP - now;
        if left >= 0.0 && self.document.state() == self.tap.state {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
            return true;
        }
        self.tap.taps.end();
        false
    }
}
