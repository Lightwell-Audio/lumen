//! `--bench-callback`: plays the opened project and measures the audio
//! callback (M6 slice 7c; the model and the report are in
//! `crate::callback_bench`). This is the part that touches the app: it waits
//! for the project and its plugins, starts playback, takes the audio layer's
//! counters every window, and ends the run.

use std::time::Instant;

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::callback_bench::{Callback, Phase, Report, Setup};

/// The process's status when the benchmark could not run.
const COULD_NOT_RUN: i32 = 2;
/// And when a block overran or audio was lost.
const OVERRAN: i32 = 3;

impl App {
    /// Called every frame; does nothing without `--bench-callback`.
    pub(crate) fn drive_callback_bench(&mut self, ctx: &egui::Context) {
        let Some(mut bench) = self.callback_bench.take() else {
            return;
        };
        let now = Instant::now();
        match bench.phase() {
            Phase::Loading => {
                let up = self.callback_up();
                match (bench.settled(up), bench.load_expired(now)) {
                    (_, true) => {
                        let why = "the project did not finish loading".into();
                        self.end_callback_bench(ctx, bench, Err(why));
                        return;
                    }
                    (true, false) => {
                        let missing = self.hosting.not_running(self.document.project());
                        if !missing.is_empty() {
                            let why = format!("plugins are not running: {}", missing.join(", "));
                            self.end_callback_bench(ctx, bench, Err(why));
                            return;
                        }
                        self.transport.locate(Ticks::ZERO);
                        if !self.transport.state().playing {
                            self.transport.play_stop();
                        }
                        bench.play_started(now);
                    }
                    (false, false) => {
                        if let crate::audio::Status::Lost(why) | crate::audio::Status::Off(why) =
                            &self.transport.audio.status
                        {
                            let why = format!("the audio is not running: {why}");
                            self.end_callback_bench(ctx, bench, Err(why));
                            return;
                        }
                    }
                }
            }
            Phase::WarmUp if bench.warmed(now) => {
                // The counters start afresh with the measuring.
                if let Some(meters) = self.transport.audio.meters() {
                    meters.take_block_time();
                    meters.take_peak_load();
                    meters.reset_counters();
                }
                let costs = self.hosting.take_costs(self.document.project());
                bench.measure_started(now, &costs);
            }
            Phase::Measuring if bench.over(now) => {
                let setup = self.callback_setup();
                let (overloads, dropouts) = self
                    .transport
                    .audio
                    .meters()
                    .map_or((0, 0), |m| (m.overloads(), m.dropouts()));
                self.transport.stop();
                let report = bench.finish(setup, overloads, dropouts);
                self.end_callback_bench(ctx, bench, Ok(report));
                return;
            }
            Phase::Measuring if bench.due(now) => {
                if let Some(meters) = self.transport.audio.meters().cloned() {
                    let costs = self.hosting.take_costs(self.document.project());
                    bench.sample(
                        now,
                        meters.take_block_time(),
                        meters.take_peak_load(),
                        &costs,
                    );
                }
            }
            _ => {}
        }
        ctx.request_repaint_after(if bench.phase() == Phase::Measuring {
            bench.next_in(now)
        } else {
            crate::callback_bench::WINDOW
        });
        self.callback_bench = Some(bench);
    }

    /// Whether the project is open, the plugin scan is over, no plugin is
    /// loading and the audio is running.
    fn callback_up(&self) -> bool {
        !self.lanes.is_loading()
            && !self.document.project().tracks.is_empty()
            && matches!(
                self.plugins.status(),
                crate::plugins::Status::Off | crate::plugins::Status::Idle
            )
            && !self.hosting.loading_any()
            && matches!(
                self.transport.audio.status,
                crate::audio::Status::Running { .. }
            )
    }

    fn callback_setup(&self) -> Setup {
        let project = self.document.path.as_deref().map_or_else(
            || self.document.project().name.clone(),
            |p| {
                p.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            },
        );
        let settings = self.transport.audio.settings();
        let info = self.transport.audio.info.as_ref();
        Setup {
            project,
            driver: settings
                .as_ref()
                .map_or_else(String::new, |s| s.driver.clone()),
            device: settings
                .as_ref()
                .map_or_else(String::new, |s| s.device_name.clone()),
            rate: info.map_or(0, |i| i.sample_rate),
            frames: info.map_or(0, |i| i.buffer_frames),
            hosts: self.hosting.host_count(),
        }
    }

    /// Writes and prints the result, and ends the process (or, for a test,
    /// keeps the report).
    fn end_callback_bench(
        &mut self,
        ctx: &egui::Context,
        bench: Callback,
        result: Result<Report, String>,
    ) {
        let (code, text, json) = match &result {
            Ok(report) => (
                if report.pass() { 0 } else { OVERRAN },
                report.text(),
                report.json(),
            ),
            Err(why) => (
                COULD_NOT_RUN,
                format!("Callback benchmark could not run: {why}\n"),
                String::new(),
            ),
        };
        print!("{text}");
        if !json.is_empty() {
            match std::fs::write(bench.out(), &json) {
                Ok(()) => println!("report: {}", bench.out().display()),
                Err(e) => eprintln!("report: {}: {e}", bench.out().display()),
            }
        }
        self.callback_report = Some(result);
        if bench.exits() {
            std::process::exit(code);
        }
        // A test keeps the app; this stops asking for frames.
        ctx.request_repaint();
    }
}
