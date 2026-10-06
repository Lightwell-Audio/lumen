//! The Lumen application. At the end of M1 this is the shell only: the
//! single-window layout from the specification (transport bar, browser,
//! arrangement, detail panel) on egui, with the arrangement canvas drawn by
//! `lumen-render`. Features arrive milestone by milestone; controls that do
//! not work yet are shown disabled and say when they arrive.
//!
//! ```text
//! lumen                     run the application
//! lumen --smoke-test        render a few frames, report start-up time, exit
//! ```

mod canvas;

use std::sync::Arc;
use std::time::Instant;

use eframe::egui::{self, Color32, RichText, Sense, pos2, vec2};
use lumen_core::product::{PRODUCT_NAME, display_name};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature};

const RULER_HEIGHT: f32 = 22.0;

struct App {
    project: Project,
    show_browser: bool,
    show_detail: bool,
    /// Set by the raw-input hook when Shift+Tab is pressed.
    toggle_detail_requested: bool,
    left_beat: f32,
    px_per_beat: f32,
    smoke_test_frames: Option<u32>,
    started: Instant,
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
        ui.horizontal(|ui| {
            ui.label(RichText::new(PRODUCT_NAME).strong());
            ui.separator();
            for label in ["Play", "Stop", "Record", "Loop", "Metronome"] {
                ui.add_enabled(false, egui::Button::new(label))
                    .on_disabled_hover_text(arrives("Milestone 2"));
            }
            ui.separator();
            let bpm = self.project.tempo.first().map_or(120.0, |t| t.bpm);
            let sig = self.project.time_signature;
            ui.label(format!("{bpm:.1} BPM"));
            ui.label(format!("{}/{}", sig.numerator, sig.denominator));
            ui.label(RichText::new("1.1.1").monospace());
            ui.separator();
            ui.label(format!("{} — {}", self.project.name, display_name()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.show_detail, "Detail")
                    .on_hover_text("Show or hide the detail panel (Shift+Tab)");
                ui.toggle_value(&mut self.show_browser, "Browser")
                    .on_hover_text("Show or hide the browser");
            });
        });
    }

    fn arrangement(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(full, Sense::hover());
        let ruler = egui::Rect::from_min_size(full.min, vec2(full.width(), RULER_HEIGHT));
        let canvas = egui::Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);

        if response.hovered() {
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
        painter.rect_filled(full, 0.0, Color32::from_rgb(0x1B, 0x1D, 0x22));
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            self.left_beat,
            self.px_per_beat,
        ));

        // Ruler: bar numbers, thinned so labels never collide.
        painter.rect_filled(ruler, 0.0, Color32::from_rgb(0x23, 0x26, 0x2D));
        let mut step = 1;
        while step as f32 * 4.0 * self.px_per_beat < 40.0 {
            step *= 2;
        }
        let first_bar = (self.left_beat / 4.0).floor() as usize;
        let mut bar = first_bar - first_bar % step;
        loop {
            let x = canvas.left() + (bar as f32 * 4.0 - self.left_beat) * self.px_per_beat;
            if x > full.right() || bar >= canvas::GRID_BARS {
                break;
            }
            if x >= full.left() {
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    egui::Align2::LEFT_CENTER,
                    (bar + 1).to_string(),
                    egui::FontId::monospace(11.0),
                    Color32::from_rgb(0x9A, 0xA0, 0xAC),
                );
            }
            bar += step;
        }

        if self.project.tracks.is_empty() {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                "Empty project\nTracks, clips and editing arrive in Milestone 3",
                egui::FontId::proportional(14.0),
                Color32::from_rgb(0x6E, 0x74, 0x80),
            );
        }
    }
}

impl eframe::App for App {
    /// Shift+Tab toggles the detail panel (spec §5). It is taken here, before
    /// egui sees it, so it does not also move keyboard focus backwards.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
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

        if let Some(frames) = self.smoke_test_frames.as_mut() {
            *frames -= 1;
            if *frames == 0 {
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

fn main() -> eframe::Result {
    let started = Instant::now();
    let smoke_test = std::env::args().any(|a| a == "--smoke-test");
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
            Ok(Box::new(App {
                project: empty_project(),
                show_browser: true,
                show_detail: true,
                toggle_detail_requested: false,
                left_beat: 0.0,
                px_per_beat: 24.0,
                smoke_test_frames: smoke_test.then_some(SMOKE_FRAMES),
                started,
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
