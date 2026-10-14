//! The command-line test modes, run around each frame: save as and quit
//! (`--save-as`), toggling play (`--autoplay-ms`), the frame trace, the
//! screenshot (`--screenshot`) and the smoke test (`--smoke-test`). Split
//! from the frame loop in `lib.rs` (M4 review M1).

use std::sync::Arc;
use std::time::Duration;

use eframe::egui;

use crate::{App, trace};

/// Frames the smoke test renders before it quits.
pub(crate) const SMOKE_FRAMES: u32 = 30;
static FIRST_FRAME_MS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

impl App {
    /// Before the frame is drawn.
    pub(crate) fn test_modes_before(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.save_as.clone()
            && !self.lanes.is_loading()
            && !self.document.project().tracks.is_empty()
        {
            if self.saving.is_none() && self.document.is_dirty() {
                self.start_save_as(path);
            } else if self.saving.is_none() {
                println!(
                    "saved {}",
                    self.document.path.as_deref().unwrap_or(&path).display()
                );
                self.save_as = None;
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
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
    }

    /// After the frame is drawn: the screenshot and the smoke test.
    pub(crate) fn test_modes_after(&mut self, ctx: &egui::Context) {
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
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // A screenshot shows the settled state: it waits while a project
        // opens or files load (at most 30 s, so a stuck load cannot hang a
        // test run).
        let settling = self.screenshot.is_some()
            && (self.opening.is_some() || self.lanes.library.is_loading())
            && self.started.elapsed() < Duration::from_secs(30);
        if let Some(frames) = self.smoke_test_frames.as_mut() {
            if !settling {
                *frames = frames.saturating_sub(1);
            }
            if *frames == 0 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_test_frames = None;
            } else if *frames == 0 {
                println!(
                    "smoke test: {} frames rendered; first frame within {:.0} ms of start",
                    SMOKE_FRAMES,
                    FIRST_FRAME_MS.get().copied().unwrap_or(f64::NAN)
                );
                // A deliberate close by a test mode: no unsaved-changes prompt.
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
        FIRST_FRAME_MS.get_or_init(|| self.started.elapsed().as_secs_f64() * 1e3);
    }
}

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
