use std::time::Instant;

use eframe::egui;
use lumen_core::time::Ticks;

use crate::App;
use crate::callback_bench::{Callback, Phase, Report, Setup};

const COULD_NOT_RUN: i32 = 2;
const OVERRAN: i32 = 3;

impl App {
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
        ctx.request_repaint();
    }
}
