//! The demo song every scene starts from (USER_DOCS.md §6.2): drums and
//! bass made here, so the pictures look like music and no third-party
//! sound appears, opened in the real app on the silent test device.

use std::path::{Path, PathBuf};

use eframe::egui::epaint::{ClippedShape, ColorMode};
use eframe::egui::{Color32, Context, FullOutput, Shape, TexturesDelta};
use eframe::egui::{Event, Key, Modifiers, vec2};
use eframe::egui_wgpu::RendererOptions;
use egui_kittest::wgpu::WgpuTestRenderer;
use egui_kittest::{Harness, TestRenderer};
use lumen_app::testing::InputDeviceTurn;
use lumen_app::{App, Launch};
use lumen_project::testing::TempDir;

use crate::recorder::SCALE;

/// A song to work on: drums and bass, four bars at 120 BPM, fitted to the
/// window with the browser and the detail panel out of the way.
///
/// It plays on the test device whose clock a take sets
/// ([`Take`](crate::recorder::Take)), and comes with the turn on it.
pub fn song(dir: &TempDir, appearance: &str) -> (Harness<'static, App>, InputDeviceTurn) {
    let turn = lumen_app::testing::input_device_turn();
    let launch = Launch::for_test(dir.path())
        .appearing(appearance)
        .with_input_device();
    (open(dir, launch, appearance), turn)
}

fn open(dir: &TempDir, launch: Launch, appearance: &str) -> Harness<'static, App> {
    let mut h = Harness::builder()
        .with_size(vec2(960.0, 420.0))
        .with_pixels_per_point(SCALE)
        .with_theme(if appearance == "light" {
            eframe::egui::Theme::Light
        } else {
            eframe::egui::Theme::Dark
        })
        .with_step_dt(1.0 / 60.0)
        .renderer(WithoutTestCursor(WgpuTestRenderer::with_render_options(
            RendererOptions::PREDICTABLE,
        )))
        .build_eframe(|cc| {
            lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
            App::new(cc, launch)
        });
    h.run_steps(10);
    let files = vec![
        drums(&dir.path().join("Drums.wav")),
        bass(&dir.path().join("Bass.wav")),
    ];
    h.input_mut().dropped_files = files
        .into_iter()
        .map(|p| std::sync::Arc::new(Dropped(p)) as eframe::egui::DroppedFileHandle)
        .collect();
    h.run_steps(60);
    if h.state().browser_shown() {
        command(&mut h, "Show or hide the browser");
    }
    if h.state().detail_shown() {
        h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        h.run_steps(20);
    }
    h.key_press(Key::W);
    h.run_steps(40);
    h
}

/// Runs a command from the palette by its name.
pub fn command(h: &mut Harness<'static, App>, name: &str) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text(name.into()));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(30);
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

const RATE: u32 = 48_000;
/// Four bars at 120 BPM.
const SECONDS: f32 = 8.0;

/// A drum loop: a kick on every beat, a snare on two and four, and hats on
/// the eighths, so the waveform reads as rhythm.
fn drums(path: &Path) -> PathBuf {
    let beat = RATE as usize / 2;
    let mut noise = 0x2545_f491_u32;
    let samples = (0..(SECONDS * RATE as f32) as usize).map(|i| {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let white = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let in_beat = (i % beat) as f32 / RATE as f32;
        let in_eighth = (i % (beat / 2)) as f32 / RATE as f32;
        let beat_no = i / beat;
        let kick = (std::f32::consts::TAU * (48.0 + 90.0 * (-in_beat * 30.0).exp()) * in_beat)
            .sin()
            * (-in_beat * 9.0).exp();
        let snare = if beat_no % 2 == 1 {
            white * (-in_beat * 18.0).exp() * 0.7
        } else {
            0.0
        };
        let hat = white * (-in_eighth * 60.0).exp() * 0.25;
        (kick * 0.8 + snare + hat).clamp(-1.0, 1.0)
    });
    write_wav(path, samples)
}

/// A bass line: one note a bar, plucked and fading.
fn bass(path: &Path) -> PathBuf {
    let bar = RATE as usize * 2;
    let notes = [55.0_f32, 55.0, 65.4, 49.0];
    let samples = (0..(SECONDS * RATE as f32) as usize).map(|i| {
        let t = (i % bar) as f32 / RATE as f32;
        let f = notes[(i / bar) % notes.len()];
        let tone = (std::f32::consts::TAU * f * t).sin()
            + 0.3 * (std::f32::consts::TAU * 2.0 * f * t).sin();
        tone * (-t * 1.4).exp() * 0.55 * (t * 200.0).min(1.0)
    });
    write_wav(path, samples)
}

/// A stereo 16-bit WAV of `samples`.
fn write_wav(path: &Path, samples: impl Iterator<Item = f32>) -> PathBuf {
    let data: Vec<u8> = samples
        .flat_map(|s| {
            let [a, b] = ((s * 30_000.0) as i16).to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the WAV");
    path.to_path_buf()
}

/// The harness's renderer without the triangle it paints at the pointer:
/// the recorder draws the pointer Lumen asks for instead (pointer.rs), and
/// the triangle showed beside every pointer but the arrow.
struct WithoutTestCursor(WgpuTestRenderer);

impl TestRenderer for WithoutTestCursor {
    fn setup_eframe(&self, cc: &mut eframe::CreationContext<'_>, frame: &mut eframe::Frame) {
        self.0.setup_eframe(cc, frame);
    }

    fn handle_delta(&mut self, delta: &mut TexturesDelta) {
        self.0.handle_delta(delta);
    }

    fn render(&mut self, ctx: &Context, output: &FullOutput) -> Result<image::RgbaImage, String> {
        // The harness adds it last, as a white triangle edged in black
        // whose first corner is the pointer.
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let cursor = |shape: &ClippedShape| match (&shape.shape, hover) {
            (Shape::Path(path), Some(at)) => {
                path.points.len() == 3
                    && path.points[0] == at
                    && path.fill == Color32::WHITE
                    && path.stroke.color == ColorMode::Solid(Color32::BLACK)
            }
            _ => false,
        };
        if output.shapes.last().is_some_and(cursor) {
            let mut output = output.clone();
            output.shapes.pop();
            self.0.render(ctx, &output)
        } else {
            self.0.render(ctx, output)
        }
    }
}
