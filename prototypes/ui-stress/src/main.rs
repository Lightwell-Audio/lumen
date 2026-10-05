//! P-UI prototype: can egui/eframe (wgpu) render Lumen's dense views at
//! quality-bar scale fast enough, idle at near-zero CPU, start fast and
//! expose accessible controls? (ADR-0002)
//!
//! ```text
//! ui-stress                     interactive (wheel scrolls, Shift+wheel pans, Ctrl+wheel zooms)
//! ui-stress --bench             scripted pan/zoom scenarios, vsync off, then idle CPU; prints results
//! ui-stress --close-after S     interactive, closes itself after S seconds (accessibility probe)
//! ```

mod data;
mod gpu;
mod paint;
mod process;

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Rect, RichText, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

use data::{NOTE_SPAN_BEATS, Note, Peaks, TRACKS, Track};
use paint::{DrawStats, View};

const FRAMES_PER_SCENARIO: usize = 400;
const WARMUP_FRAMES: usize = 40;
const HEADER_WIDTH: f32 = 180.0;
const IDLE_SECONDS: u64 = 10;

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    scale: f32,
    /// Draw notes with the custom wgpu pipeline instead of egui shapes.
    gpu_notes: bool,
    drive: fn(&mut App, f32),
}

const fn scenario(
    name: &'static str,
    scale: f32,
    gpu_notes: bool,
    drive: fn(&mut App, f32),
) -> Scenario {
    Scenario {
        name,
        scale,
        gpu_notes,
        drive,
    }
}

const SCENARIOS: [Scenario; 10] = [
    scenario(
        "arrangement, 48 px rows, pan + scroll",
        1.0,
        false,
        drive_arrangement_pan,
    ),
    scenario(
        "arrangement, fit all, 20 px rows, scroll",
        1.0,
        false,
        drive_arrangement_fit,
    ),
    scenario(
        "arrangement, zoom sweep 2-400 px/beat",
        1.0,
        false,
        drive_zoom_sweep,
    ),
    scenario(
        "piano roll, all 100k notes, egui shapes",
        1.0,
        false,
        drive_piano_all,
    ),
    scenario(
        "piano roll, all 100k notes, GPU instanced",
        1.0,
        true,
        drive_piano_all,
    ),
    scenario("piano roll, 120 px/beat, pan", 1.0, false, drive_piano_pan),
    scenario(
        "arrangement, 48 px rows, pan + scroll",
        2.0,
        false,
        drive_arrangement_pan,
    ),
    scenario(
        "arrangement, fit all, 20 px rows, scroll",
        2.0,
        false,
        drive_arrangement_fit,
    ),
    scenario(
        "piano roll, all 100k notes, egui shapes",
        2.0,
        false,
        drive_piano_all,
    ),
    scenario(
        "piano roll, all 100k notes, GPU instanced",
        2.0,
        true,
        drive_piano_all,
    ),
];

fn drive_arrangement_pan(app: &mut App, t: f32) {
    app.arrangement = View {
        left_beat: t * 360.0,
        px_per_beat: 24.0,
        top_px: 0.0,
        row_h: 48.0,
    };
    app.arrangement.top_px = t * (TRACKS as f32 * 48.0 - app.arrangement_size.y).max(0.0);
}

fn drive_arrangement_fit(app: &mut App, t: f32) {
    let px_per_beat = app.arrangement_size.x / 404.0;
    app.arrangement = View {
        left_beat: 0.0,
        px_per_beat,
        top_px: 0.0,
        row_h: 20.0,
    };
    app.arrangement.top_px = t * (TRACKS as f32 * 20.0 - app.arrangement_size.y).max(0.0);
}

fn drive_zoom_sweep(app: &mut App, t: f32) {
    let px_per_beat = 2.0 * 200_f32.powf(0.5 - 0.5 * (std::f32::consts::TAU * t).cos());
    let left_beat = 200.0 - app.arrangement_size.x / 2.0 / px_per_beat;
    app.arrangement = View {
        left_beat,
        px_per_beat,
        top_px: 480.0,
        row_h: 32.0,
    };
}

fn drive_piano_all(app: &mut App, t: f32) {
    let row_h = (app.piano_size.y / 128.0).max(1.0);
    let px_per_beat = app.piano_size.x / NOTE_SPAN_BEATS;
    app.piano = View {
        left_beat: 0.0,
        px_per_beat,
        top_px: t * 2.0,
        row_h,
    };
}

fn drive_piano_pan(app: &mut App, t: f32) {
    app.piano = View {
        left_beat: t * 3_800.0,
        px_per_beat: 120.0,
        top_px: 40.0 * 12.0,
        row_h: 12.0,
    };
}

#[derive(Default, Clone)]
struct Samples {
    interval_ms: Vec<f32>,
    build_ms: Vec<f32>,
    /// eframe's measure: `App::ui` plus tessellation and rendering of the
    /// frame, excluding vsync waiting. The true per-frame CPU cost.
    cpu_ms: Vec<f32>,
    arrangement: DrawStats,
    piano: DrawStats,
}

enum Mode {
    Interactive {
        close_at: Option<Instant>,
    },
    Bench {
        scenario: usize,
        frame: usize,
        samples: Samples,
        results: Vec<(Scenario, Samples)>,
    },
    Idle {
        started: Instant,
        cpu_at_start: f64,
        results: Vec<(Scenario, Samples)>,
    },
}

struct App {
    tracks: Vec<Track>,
    notes: Vec<Note>,
    peaks: Peaks,
    arrangement: View,
    piano: View,
    arrangement_size: egui::Vec2,
    piano_size: egui::Vec2,
    mode: Mode,
    last_frame_start: Option<Instant>,
    startup_ms: Vec<f64>,
    data_build_ms: f64,
    playing: bool,
    looping: bool,
    tempo: f32,
    vsync_off: bool,
}

impl App {
    fn new(mode: Mode) -> Self {
        let started = Instant::now();
        let (tracks, notes, peaks) = (data::tracks(), data::notes(), Peaks::synthetic());
        Self {
            tracks,
            notes,
            peaks,
            arrangement: View {
                left_beat: 0.0,
                px_per_beat: 24.0,
                top_px: 0.0,
                row_h: 48.0,
            },
            piano: View {
                left_beat: 0.0,
                px_per_beat: 80.0,
                top_px: 48.0 * 10.0,
                row_h: 10.0,
            },
            arrangement_size: vec2(1600.0, 800.0),
            piano_size: vec2(1600.0, 400.0),
            mode,
            last_frame_start: None,
            startup_ms: Vec::new(),
            data_build_ms: started.elapsed().as_secs_f64() * 1e3,
            playing: false,
            looping: false,
            tempo: 120.0,
            vsync_off: false,
        }
    }

    fn transport(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .button(if self.playing { "Pause" } else { "Play" })
                .clicked()
            {
                self.playing = !self.playing;
            }
            if ui.button("Stop").clicked() {
                self.playing = false;
            }
            let _ = ui.button(RichText::new("Record").color(Color32::from_rgb(0xE0, 0x6C, 0x5F)));
            ui.toggle_value(&mut self.looping, "Loop");
            ui.label("Tempo");
            ui.add(
                egui::DragValue::new(&mut self.tempo)
                    .range(20.0..=999.0)
                    .speed(0.5)
                    .suffix(" BPM"),
            );
            ui.separator();
            ui.label(match &self.mode {
                Mode::Bench {
                    scenario, frame, ..
                } => {
                    format!(
                        "benchmark {}/{}: {} (frame {frame})",
                        scenario + 1,
                        SCENARIOS.len(),
                        SCENARIOS[*scenario].name
                    )
                }
                Mode::Idle { .. } => {
                    format!("measuring idle CPU for {IDLE_SECONDS} s; do not touch the window")
                }
                Mode::Interactive { .. } => {
                    "wheel: scroll · Shift+wheel: pan · Ctrl+wheel: zoom".to_string()
                }
            });
        });
    }

    fn track_headers(&mut self, ui: &mut Ui, area: Rect) {
        ui.painter()
            .rect_filled(area, 0.0, Color32::from_rgb(0x23, 0x26, 0x2D));
        let view = self.arrangement;
        let first = (view.top_px / view.row_h).floor().max(0.0) as usize;
        let last =
            (((view.top_px + area.height()) / view.row_h).ceil() as usize).min(self.tracks.len());
        for index in first..last {
            let y = area.top() + index as f32 * view.row_h - view.top_px;
            if view.row_h < 18.0 {
                continue; // too small for controls; the colour strip still shows
            }
            let track = &mut self.tracks[index];
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(area.left(), y + 1.0), vec2(4.0, view.row_h - 2.0)),
                0.0,
                track.color,
            );
            let row_center = y + view.row_h / 2.0;
            ui.put(
                Rect::from_center_size(pos2(area.left() + 62.0, row_center), vec2(110.0, 18.0)),
                egui::Label::new(RichText::new(&track.name).size(12.0)).truncate(),
            );
            let name = track.name.clone();
            for (offset, kind) in [(132.0, "Mute"), (158.0, "Solo")] {
                let state = if kind == "Mute" {
                    &mut track.mute
                } else {
                    &mut track.solo
                };
                let response = ui.put(
                    Rect::from_center_size(
                        pos2(area.left() + offset, row_center),
                        vec2(22.0, 18.0),
                    ),
                    egui::Button::new(&kind[..1]).selected(*state),
                );
                // Screen readers hear "Mute Track 12", not "M".
                let selected = *state;
                response.widget_info(|| {
                    WidgetInfo::selected(
                        WidgetType::Checkbox,
                        true,
                        selected,
                        format!("{kind} {name}"),
                    )
                });
                if response.clicked() {
                    *state = !*state;
                }
            }
        }
    }

    fn handle_wheel(ui: &Ui, response: &egui::Response, view: &mut View, content_height: f32) {
        if !response.hovered() {
            return;
        }
        let (delta, modifiers, pointer) =
            ui.input(|i| (i.smooth_scroll_delta, i.modifiers, i.pointer.hover_pos()));
        if delta == egui::Vec2::ZERO {
            return;
        }
        if modifiers.command {
            let anchor_x = pointer.map_or(response.rect.center().x, |p| p.x) - response.rect.left();
            let anchor_beat = view.left_beat + anchor_x / view.px_per_beat;
            view.px_per_beat = (view.px_per_beat * (delta.y * 0.003).exp()).clamp(0.05, 2_000.0);
            view.left_beat = anchor_beat - anchor_x / view.px_per_beat;
        } else if modifiers.shift {
            view.left_beat -= (delta.x + delta.y) / view.px_per_beat;
        } else {
            view.left_beat -= delta.x / view.px_per_beat;
            view.top_px = (view.top_px - delta.y)
                .clamp(0.0, (content_height - response.rect.height()).max(0.0));
        }
    }

    fn step_bench(
        &mut self,
        ctx: &egui::Context,
        interval_ms: Option<f32>,
        build_ms: f32,
        previous_cpu_ms: Option<f32>,
    ) {
        let Mode::Bench {
            scenario,
            frame,
            samples,
            results,
        } = &mut self.mode
        else {
            return;
        };
        // `previous_cpu_ms` belongs to the previous frame of the same scenario.
        if *frame > WARMUP_FRAMES
            && let (Some(interval), Some(cpu)) = (interval_ms, previous_cpu_ms)
        {
            samples.interval_ms.push(interval);
            samples.build_ms.push(build_ms);
            samples.cpu_ms.push(cpu);
        }
        *frame += 1;
        if *frame < FRAMES_PER_SCENARIO {
            return;
        }
        results.push((SCENARIOS[*scenario], std::mem::take(samples)));
        *scenario += 1;
        *frame = 0;
        if *scenario == SCENARIOS.len() {
            let results = std::mem::take(results);
            ctx.set_zoom_factor(1.0);
            self.mode = Mode::Idle {
                started: Instant::now(),
                cpu_at_start: process::cpu_seconds(),
                results,
            };
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        if self.startup_ms.len() < 2 {
            self.startup_ms.push(process::age_ms());
        }
        let interval_ms = self
            .last_frame_start
            .map(|t| (frame_start - t).as_secs_f32() * 1e3);
        self.last_frame_start = Some(frame_start);
        let ctx = ui.ctx().clone();

        let previous_cpu_ms = frame.info().cpu_usage.map(|s| s * 1e3);
        if let Mode::Bench {
            scenario, frame: n, ..
        } = &self.mode
        {
            if !self.vsync_off
                && let Some(state) = frame.wgpu_render_state()
            {
                let info = state.adapter.get_info();
                let present = frame.wgpu_surface_config().map(|c| c.present_mode);
                println!(
                    "GPU: {} ({:?} backend, driver {}), present mode {present:?}",
                    info.name, info.backend, info.driver_info
                );
                self.vsync_off = true;
            }
            let current = SCENARIOS[*scenario];
            ctx.set_zoom_factor(current.scale);
            (current.drive)(self, *n as f32 / FRAMES_PER_SCENARIO as f32);
        }

        egui::Panel::top("transport").show(ui, |ui| self.transport(ui));
        let mut piano_stats = DrawStats::default();
        egui::Panel::bottom("piano_roll")
            .exact_size(ui.available_height() * 0.36)
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let response = ui.allocate_rect(rect, Sense::hover());
                self.piano_size = rect.size();
                let content_height = 128.0 * self.piano.row_h;
                Self::handle_wheel(ui, &response, &mut self.piano, content_height);
                let gpu_notes = matches!(&self.mode, Mode::Bench { scenario, .. } if SCENARIOS[*scenario].gpu_notes);
                let notes: &[Note] = if gpu_notes { &[] } else { &self.notes };
                piano_stats = paint::piano_roll(ui.painter(), rect, notes, &self.piano);
                if gpu_notes {
                    ui.painter_at(rect).add(gpu::notes_shape(rect, &self.piano));
                }
            });
        let mut arrangement_stats = DrawStats::default();
        egui::CentralPanel::default().show(ui, |ui| {
            let full = ui.available_rect_before_wrap();
            let headers = Rect::from_min_size(full.min, vec2(HEADER_WIDTH, full.height()));
            let canvas = Rect::from_min_max(pos2(full.left() + HEADER_WIDTH, full.top()), full.max);
            self.arrangement_size = canvas.size();
            let response = ui.allocate_rect(canvas, Sense::hover());
            let content_height = TRACKS as f32 * self.arrangement.row_h;
            Self::handle_wheel(ui, &response, &mut self.arrangement, content_height);
            arrangement_stats = paint::arrangement(
                &ui.painter_at(canvas),
                canvas,
                &self.tracks,
                &self.peaks,
                &self.arrangement,
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(headers), |ui| {
                ui.set_clip_rect(headers);
                self.track_headers(ui, headers);
            });
        });

        let build_ms = frame_start.elapsed().as_secs_f32() * 1e3;
        match &mut self.mode {
            Mode::Bench { samples, .. } => {
                samples.arrangement = arrangement_stats;
                samples.piano = piano_stats;
                self.step_bench(&ctx, interval_ms, build_ms, previous_cpu_ms);
                ctx.request_repaint();
            }
            Mode::Idle {
                started,
                cpu_at_start,
                results,
            } => {
                if started.elapsed() >= Duration::from_secs(IDLE_SECONDS) {
                    let cpu = process::cpu_seconds() - *cpu_at_start;
                    report(
                        results,
                        &self.startup_ms,
                        self.data_build_ms,
                        cpu,
                        started.elapsed(),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    // Frames can still arrive before the window closes; report once.
                    self.mode = Mode::Interactive {
                        close_at: Some(Instant::now()),
                    };
                } else {
                    // No repaint request: an idle UI must not burn CPU. Wake once at the end.
                    ctx.request_repaint_after(
                        Duration::from_secs(IDLE_SECONDS) - started.elapsed(),
                    );
                }
            }
            Mode::Interactive { close_at } => {
                if close_at.is_some_and(|at| Instant::now() >= at) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if let Some(at) = close_at {
                    ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
                }
            }
        }
    }
}

fn report(
    results: &[(Scenario, Samples)],
    startup_ms: &[f64],
    data_ms: f64,
    idle_cpu: f64,
    idle: Duration,
) {
    let pct = |sorted: &[f32], p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    println!("\nP-UI results, all in ms.");
    println!("cpu = whole frame on the CPU (UI code + tessellation + rendering, no vsync wait)");
    println!("interval = time between frames; build = Lumen's own UI code only\n");
    println!(
        "{:<44} {:>5} | {:>6} {:>6} {:>6} {:>6} | {:>6} {:>6} {:>6} | {:>5} {:>5} | {:>5} {:>6} {:>6} {:>6} {:>6}",
        "scenario",
        "scale",
        "cpu50",
        "cpu95",
        "cpu99",
        "cpuMx",
        "int50",
        "int99",
        "intMx",
        "bld50",
        "bld99",
        "items",
        "spans",
        "labels",
        "waves",
        "notes"
    );
    for (scenario, samples) in results {
        let mut frames = samples.interval_ms.clone();
        frames.sort_by(f32::total_cmp);
        let mut builds = samples.build_ms.clone();
        builds.sort_by(f32::total_cmp);
        let mut cpu = samples.cpu_ms.clone();
        cpu.sort_by(f32::total_cmp);
        if frames.is_empty() || cpu.is_empty() {
            continue;
        }
        println!(
            "{:<44} {:>4.0}% | {:>6.2} {:>6.2} {:>6.2} {:>6.2} | {:>6.2} {:>6.2} {:>6.2} | {:>5.2} {:>5.2} | {:>5} {:>6} {:>6} {:>6} {:>6}",
            scenario.name,
            scenario.scale * 100.0,
            pct(&cpu, 0.5),
            pct(&cpu, 0.95),
            pct(&cpu, 0.99),
            pct(&cpu, 1.0),
            pct(&frames, 0.5),
            pct(&frames, 0.99),
            pct(&frames, 1.0),
            pct(&builds, 0.5),
            pct(&builds, 0.99),
            samples.arrangement.items,
            samples.arrangement.merged_spans + samples.piano.merged_spans,
            samples.arrangement.labels,
            samples.arrangement.wave_columns,
            samples.piano.items,
        );
    }
    println!("\nall times in ms; counts are for the last frame of each scenario");
    println!(
        "start-up: synthetic data built in {data_ms:.0} ms; first ui() call {:.0} ms after process creation; second (first frame on screen) {:.0} ms",
        startup_ms.first().copied().unwrap_or(f64::NAN),
        startup_ms.get(1).copied().unwrap_or(f64::NAN)
    );
    println!(
        "idle: {:.3} s CPU over {:.1} s = {:.2}% of one core",
        idle_cpu,
        idle.as_secs_f64(),
        idle_cpu / idle.as_secs_f64() * 100.0
    );
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = if args.iter().any(|a| a == "--bench") {
        Mode::Bench {
            scenario: 0,
            frame: 0,
            samples: Samples::default(),
            results: Vec::new(),
        }
    } else {
        let close_at = args
            .iter()
            .position(|a| a == "--close-after")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .map(|s| Instant::now() + Duration::from_secs(s));
        Mode::Interactive { close_at }
    };
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lumen UI stress test")
            .with_inner_size([3_400.0, 1_300.0]),
        ..Default::default()
    };
    if matches!(mode, Mode::Bench { .. }) {
        // Uncapped presentation, so frame intervals show cost rather than refresh rate.
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "Lumen UI stress test",
        options,
        Box::new(|cc| {
            let app = App::new(mode);
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                gpu::install(state, &app.notes);
            }
            Ok(Box::new(app))
        }),
    )
}
