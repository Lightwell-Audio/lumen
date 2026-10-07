//! The Lumen application: the single-window layout from the specification
//! (transport bar, browser, arrangement, detail panel) on egui, with the
//! arrangement canvas drawn by `lumen-render` and audio from `lumen-engine`.
//! Features arrive milestone by milestone; controls that do not work yet are
//! shown disabled and say when they arrive.
//!
//! ```text
//! lumen                          run the application
//! lumen --audio asio:"Focusrite USB ASIO"
//!                                use this device (driver: asio, wasapi,
//!                                exclusive; cpal on macOS and Linux)
//! lumen --open-audio drums.wav   add an audio file at bar 1 (or drag files
//!                                from Explorer onto the arrangement)
//! lumen --scale 1.5              UI scale
//! lumen --smoke-test             render a few frames, report start-up time, exit
//! lumen --screenshot out.png     render, save a screenshot, exit
//! ```

mod audio;
mod canvas;
mod icons;
mod lanes;
mod media;
mod position;
mod ruler;
mod theme;
mod trace;
mod transport;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Sense, Stroke, pos2, vec2};
use lumen_core::product::{PRODUCT_NAME, display_name};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature};

use transport::Transport;

const RULER_HEIGHT: f32 = 22.0;

struct App {
    project: Project,
    transport: Transport,
    /// Audio clips on the simple timeline (M2 slice 6).
    lanes: lanes::Lanes,
    /// Where dropped files go: under the pointer, snapped, or the playhead.
    drop_target: Ticks,
    show_browser: bool,
    show_detail: bool,
    /// Set by the raw-input hook when Shift+Tab is pressed.
    toggle_detail_requested: bool,
    left_beat: f32,
    px_per_beat: f32,
    smoke_test_frames: Option<u32>,
    /// Where `--screenshot` saves, once the frames have rendered.
    screenshot: Option<PathBuf>,
    started: Instant,
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    autoplay: Option<Duration>,
    autoplay_count: u32,
    /// Playing or not, as last traced, to log only changes.
    traced: Option<bool>,
}

fn empty_project() -> Project {
    Project {
        name: "Untitled".into(),
        sample_rate: 48_000,
        tempo: vec![TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
        }],
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tracks: Vec::new(),
        media: Vec::new(),
    }
}

fn arrives(milestone: &str) -> String {
    format!("Arrives in {milestone}")
}

impl App {
    fn transport(&mut self, ui: &mut egui::Ui) {
        // The panel toggles are laid out first, from the right, so they keep
        // their place at any width or scale; the transport takes the rest and
        // the status text shortens to fit.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut self.show_detail, "Detail")
                .on_hover_text("Show or hide the detail panel (Shift+Tab)");
            ui.toggle_value(&mut self.show_browser, "Browser")
                .on_hover_text("Show or hide the browser");
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(PRODUCT_NAME).strong());
                ui.separator();
                self.transport.ui(ui, &mut self.project);
            });
        });
    }

    fn arrangement(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(full, Sense::hover());
        let ruler = egui::Rect::from_min_size(full.min, vec2(full.width(), RULER_HEIGHT));
        let canvas = egui::Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);

        // `contains_pointer`, not `hovered`: the ruler's own click area sits on
        // top, and scrolling and zooming must work over it too.
        if response.contains_pointer() {
            // egui turns Ctrl+wheel (and trackpad pinch) into a zoom factor
            // and reports no scroll for it, so zoom must read `zoom_delta`.
            let (delta, zoom, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0 {
                let anchor = pointer.map_or(canvas.center().x, |p| p.x) - canvas.left();
                let beat = self.left_beat + anchor / self.px_per_beat;
                self.px_per_beat = (self.px_per_beat * zoom).clamp(2.0, 400.0);
                self.left_beat = beat - anchor / self.px_per_beat;
            } else {
                self.left_beat -= (delta.x + delta.y) / self.px_per_beat;
            }
            self.left_beat = self.left_beat.max(0.0);
        }

        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, theme::CANVAS);
        let meter = self.transport.timeline.meter.meter_at(Ticks::ZERO);
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            self.left_beat,
            self.px_per_beat,
            canvas::GridMeter {
                bar: meter.bar().as_quarters() as f32,
                beat: meter.beat().as_quarters() as f32,
            },
        ));
        let x_of = |quarters: f32| canvas.left() + (quarters - self.left_beat) * self.px_per_beat;
        self.lanes
            .paint(&painter, canvas, &self.transport.timeline, x_of);
        // A hover card with the file's details.
        let hover = ui.interact(canvas, ui.id().with("clips"), Sense::hover());
        if let Some(pointer) = hover.hover_pos()
            && let Some(item) = self
                .lanes
                .file_at(pointer, canvas, &self.transport.timeline, x_of)
        {
            hover.on_hover_ui_at_pointer(|ui| lanes::file_card(ui, item));
        }

        // Dropped files land under the pointer (snapped) when the system
        // reports it, otherwise at the playhead.
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p));
        self.drop_target = match pointer {
            Some(p) => ruler::snap(
                &self.transport.timeline.meter,
                ruler::ticks_at_x(p.x, canvas.left(), self.left_beat, self.px_per_beat),
                self.px_per_beat,
            ),
            None => self.transport.state().ticks,
        };

        // Ruler: loop range, then bar numbers thinned so labels never collide.
        painter.rect_filled(ruler, 0.0, theme::RULER);
        let looped = self.transport.loop_region;
        if looped.enabled {
            let span = egui::Rect::from_x_y_ranges(
                x_of(looped.start.as_quarters() as f32)..=x_of(looped.end.as_quarters() as f32),
                ruler.y_range(),
            );
            painter.rect_filled(span.intersect(ruler), 0.0, theme::LOOP_RANGE);
        }
        let bar_quarters = meter.bar().as_quarters() as f32;
        let mut step = 1;
        while step as f32 * bar_quarters * self.px_per_beat < 40.0 {
            step *= 2;
        }
        let first_bar = (self.left_beat / bar_quarters).floor() as i64;
        let mut bar = first_bar - first_bar.rem_euclid(step);
        loop {
            let start = self.transport.timeline.meter.bar_start(bar);
            let x = x_of(start.as_quarters() as f32);
            if x > full.right() || bar >= canvas::GRID_BARS as i64 {
                break;
            }
            if x >= full.left() {
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    egui::Align2::LEFT_CENTER,
                    (bar + 1).to_string(),
                    egui::FontId::monospace(11.0),
                    theme::RULER_TEXT,
                );
            }
            bar += step;
        }

        // Click-to-locate: the ruler moves the playhead to the nearest line of
        // the zoom-adaptive grid (Alt bypasses snapping). A ghost line shows
        // where a click will land.
        let ruler_response = ui
            .interact(ruler, ui.id().with("ruler"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to move the playhead. Hold Alt to place it freely");
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Ruler: click to move the playhead",
            )
        });
        if let Some(pointer) = ruler_response.hover_pos() {
            let free = ui.input(|i| i.modifiers.alt);
            let raw = ruler::ticks_at_x(pointer.x, canvas.left(), self.left_beat, self.px_per_beat);
            let target = if free {
                raw
            } else {
                ruler::snap(&self.transport.timeline.meter, raw, self.px_per_beat)
            };
            let gx = x_of(target.as_quarters() as f32);
            painter.line_segment(
                [pos2(gx, ruler.top()), pos2(gx, full.bottom())],
                Stroke::new(1.0, theme::GHOST),
            );
            if ruler_response.clicked() {
                trace::log(|| format!("ruler: locate {target:?} (snapped: {})", !free));
                self.transport.locate(target);
            }
        }

        // The start marker: where Play starts. A hollow arrow on the ruler and
        // a faint line, so it reads as a marker next to the solid playhead.
        let state = self.transport.state();
        let mx = x_of(state.marker.as_quarters() as f32);
        if (full.left()..=full.right()).contains(&mx) {
            painter.line_segment(
                [pos2(mx, ruler.bottom()), pos2(mx, full.bottom())],
                Stroke::new(1.0, theme::GHOST),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(mx - 5.0, ruler.top() + 1.0),
                    pos2(mx + 5.0, ruler.top() + 1.0),
                    pos2(mx, ruler.top() + 8.0),
                ],
                egui::Color32::TRANSPARENT,
                Stroke::new(1.5, theme::ACCENT),
            ));
        }

        // Playhead.
        let x = x_of(state.ticks.as_quarters() as f32);
        if (full.left()..=full.right()).contains(&x) {
            painter.line_segment(
                [pos2(x, ruler.top()), pos2(x, full.bottom())],
                Stroke::new(1.0, theme::ACCENT),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - 5.0, ruler.top()),
                    pos2(x + 5.0, ruler.top()),
                    pos2(x, ruler.top() + 7.0),
                ],
                theme::ACCENT,
                Stroke::NONE,
            ));
        }

        // While files are dragged over the window, say which will be added.
        let hovered: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !hovered.is_empty() {
            let supported = hovered.iter().filter(|p| media::is_audio_file(p)).count();
            let color = if supported > 0 {
                theme::ACCENT
            } else {
                theme::WARNING
            };
            painter.rect_stroke(
                canvas.shrink(2.0),
                4.0,
                Stroke::new(2.0, color),
                egui::StrokeKind::Inside,
            );
            let mut lines: Vec<String> = hovered
                .iter()
                .take(8)
                .map(|p| {
                    let name = p
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    match media::format_name(p) {
                        Some(format) => format!("✓  {name}  ({format})"),
                        None => format!("✕  {name}  (not an audio file Lumen reads)"),
                    }
                })
                .collect();
            if hovered.len() > 8 {
                lines.push(format!("…and {} more", hovered.len() - 8));
            }
            let headline = match (supported, hovered.len()) {
                (0, _) => "Nothing here can be added".to_owned(),
                (n, total) if n == total => {
                    format!(
                        "Drop to add {n} audio file{}",
                        if n == 1 { "" } else { "s" }
                    )
                }
                (n, total) => format!("Drop to add {n} of {total} files"),
            };
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                format!("{headline}\n\n{}", lines.join("\n")),
                egui::FontId::proportional(15.0),
                color,
            );
        } else if self.lanes.is_empty() {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                "Drop audio files here\nWAV, AIFF, FLAC, MP3 or Ogg Vorbis",
                egui::FontId::proportional(14.0),
                theme::HINT_TEXT,
            );
        }
        self.lanes
            .panel(ui.ctx(), pos2(full.left() + 8.0, full.bottom() - 8.0));
    }
}

impl eframe::App for App {
    /// Shift+Tab toggles the detail panel (spec §5). It is taken here, before
    /// egui sees it, so it does not also move keyboard focus backwards.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        for event in &raw_input.events {
            if let egui::Event::PointerButton { pos, pressed, .. } = event {
                trace::log(|| {
                    format!(
                        "input: mouse {} at ({:.0}, {:.0})",
                        if *pressed { "down" } else { "up" },
                        pos.x,
                        pos.y
                    )
                });
            }
            if let egui::Event::Key {
                key, pressed: true, ..
            } = event
            {
                trace::log(|| format!("input: key {key:?}"));
            }
        }
        raw_input.events.retain(|event| {
            let toggle = matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                    if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            );
            if toggle {
                self.toggle_detail_requested = true;
            }
            // Drop the release too, so egui never sees half of the shortcut.
            !matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            )
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.transport.update();
        self.transport.shortcuts(&ctx);
        let rate = self.transport.state().sample_rate;
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            trace::log(|| format!("drop: {} file(s) at {:?}", dropped.len(), self.drop_target));
            self.lanes.add_files(dropped, self.drop_target, rate, &ctx);
        }
        self.lanes
            .update(rate, &ctx, &mut self.transport.audio.clips);
        self.transport.clip_memory = self.lanes.library.memory();
        if let Some(every) = self.autoplay {
            // Toggle play and stop every `every`, as Space would.
            let elapsed = self.started.elapsed();
            let due = every * self.autoplay_count;
            if elapsed >= due {
                self.autoplay_count += 1;
                trace::log(|| format!("autoplay: toggle {}", self.autoplay_count));
                self.transport.play_stop();
            } else {
                ctx.request_repaint_after(due - elapsed);
            }
        }
        if trace::enabled() {
            let state = self.transport.state();
            if self.traced != Some(state.playing) {
                trace::log(|| {
                    format!(
                        "frame: shows {} at sample {}",
                        if state.playing { "PLAYING" } else { "STOPPED" },
                        state.sample
                    )
                });
                self.traced = Some(state.playing);
            }
        }
        let panels_before = (self.show_browser, self.show_detail);
        if std::mem::take(&mut self.toggle_detail_requested) {
            self.show_detail = !self.show_detail;
        }

        egui::Panel::top("transport").show(ui, |ui| self.transport(ui));

        // A focused control must not vanish with its panel: AccessKit
        // rejects focus on a node that no longer exists. Release focus
        // whenever a panel is hidden, whatever hid it.
        let hid_a_panel =
            (panels_before.0 && !self.show_browser) || (panels_before.1 && !self.show_detail);
        if hid_a_panel {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        if self.show_browser {
            egui::Panel::left("browser")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| {
                    ui.heading("Browser");
                    ui.add_space(8.0);
                    ui.label(RichText::new("Devices, samples, projects and your places").weak());
                    ui.label(RichText::new(arrives("Milestones 3 and 6")).weak().small());
                });
        }
        if self.show_detail {
            egui::Panel::bottom("detail")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (tab, milestone) in [
                            ("Devices", "Milestone 6"),
                            ("Clip", "Milestone 5"),
                            ("Mixer", "Milestone 4"),
                        ] {
                            ui.add_enabled(false, egui::Button::new(tab))
                                .on_disabled_hover_text(arrives(milestone));
                        }
                    });
                    ui.label(
                        RichText::new(
                            "Select a track or clip to see its devices, editor or mixer strip.",
                        )
                        .weak(),
                    );
                });
        }
        egui::CentralPanel::default().show(ui, |ui| self.arrangement(ui));

        // Scheduled after drawing, because buttons are clicked while drawing.
        // Every frame while playing or settling after a command; otherwise
        // once a second for device events, so idle CPU stays near zero.
        if self.transport.animating() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        if let Some(path) = &self.screenshot {
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                    _ => None,
                })
            });
            if let Some(image) = image {
                match save_png(path, &image) {
                    Ok(()) => println!("screenshot saved to {}", path.display()),
                    Err(e) => eprintln!("screenshot: {e}"),
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Some(frames) = self.smoke_test_frames.as_mut() {
            *frames = frames.saturating_sub(1);
            if *frames == 0 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_test_frames = None;
            } else if *frames == 0 {
                println!(
                    "smoke test: {} frames rendered; first frame within {:.0} ms of start",
                    SMOKE_FRAMES,
                    FIRST_FRAME_MS.get().copied().unwrap_or(f64::NAN)
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
        FIRST_FRAME_MS.get_or_init(|| self.started.elapsed().as_secs_f64() * 1e3);
    }
}

const SMOKE_FRAMES: u32 = 30;
static FIRST_FRAME_MS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = image.size;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .map_err(|e| e.to_string())
}

/// The value after `flag`, if given.
fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> eframe::Result {
    let started = Instant::now();
    let screenshot = arg_value("--screenshot").map(PathBuf::from);
    let smoke_test = screenshot.is_some() || std::env::args().any(|a| a == "--smoke-test");
    let scale: f32 = arg_value("--scale")
        .and_then(|s| s.parse().ok())
        .filter(|s| (0.5..=3.0).contains(s))
        .unwrap_or(1.0);
    let choice = match arg_value("--audio") {
        Some(arg) => audio::Choice::parse(&arg).unwrap_or_else(|e| {
            eprintln!("{e}; using the default output");
            audio::Choice::Default
        }),
        None => audio::Choice::Default,
    };
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(display_name())
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    // Compiled GPU pipelines are cached between launches (ADR-0002).
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |=
                adapter.features() & eframe::wgpu::Features::PIPELINE_CACHE;
            descriptor
        });
    }
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(move |cc| {
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                canvas::install(state).map_err(|e| format!("renderer: {e}"))?;
            }
            cc.egui_ctx.set_zoom_factor(scale);
            let project = empty_project();
            let transport = Transport::new(&project, &choice);
            let mut lanes = lanes::Lanes::default();
            if let Some(path) = arg_value("--open-audio") {
                lanes.add_files(
                    vec![PathBuf::from(path)],
                    Ticks::ZERO,
                    transport.state().sample_rate,
                    &cc.egui_ctx,
                );
            }
            Ok(Box::new(App {
                transport,
                lanes,
                drop_target: Ticks::ZERO,
                project,
                show_browser: true,
                show_detail: true,
                toggle_detail_requested: false,
                left_beat: 0.0,
                px_per_beat: 24.0,
                smoke_test_frames: smoke_test.then_some(SMOKE_FRAMES),
                screenshot,
                started,
                autoplay: arg_value("--autoplay-ms")
                    .and_then(|ms| ms.parse().ok())
                    .map(Duration::from_millis),
                autoplay_count: 1,
                traced: None,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_project_is_valid_and_survives_save_and_load() {
        let project = empty_project();
        assert_eq!(lumen_project::validate(&project), Ok(()));
        let bytes = lumen_project::to_bytes(&project).expect("saves");
        assert_eq!(lumen_project::from_bytes(&bytes).expect("loads"), project);
    }

    #[test]
    fn the_window_title_comes_from_the_product_name() {
        assert!(display_name().starts_with(PRODUCT_NAME));
    }
}
