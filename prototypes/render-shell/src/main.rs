//! P-RENDER host 2: `lumen-render` in a bare winit window, with no UI toolkit.
//! Same scene and metrics as `ui-stress`, so the two hosts compare directly.
//!
//! ```text
//! render-shell --bench    scripted scenarios, vsync off, then idle CPU; prints results
//! render-shell            interactive window (static scene)
//! ```

mod process;

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_render::vello::wgpu;
use lumen_render::{Area, LayerId, Renderer, View, demo};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const FRAMES: usize = 400;
const WARMUP: usize = 40;
const IDLE_SECONDS: u64 = 10;

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    overlay: bool,
    drive: fn(f32, [f32; 2], [f32; 2]) -> (View, View),
}

fn fit_all(t: f32, arr: [f32; 2], piano: [f32; 2]) -> (View, View) {
    let rows = 20.0;
    (
        View {
            left: 0.0,
            px_per_unit: arr[0] / demo::ARRANGEMENT_BEATS,
            top_px: t * (200.0 * rows - arr[1]).max(0.0),
            row_height: rows,
        },
        View {
            left: 0.0,
            px_per_unit: piano[0] / demo::NOTE_SPAN_BEATS,
            top_px: t * 2.0,
            row_height: (piano[1] / 128.0).max(1.0),
        },
    )
}

fn editing(t: f32, arr: [f32; 2], _piano: [f32; 2]) -> (View, View) {
    (
        View {
            left: t * 360.0,
            px_per_unit: 24.0,
            top_px: t * (200.0 * 48.0 - arr[1]).max(0.0),
            row_height: 48.0,
        },
        View {
            left: t * 3_800.0,
            px_per_unit: 120.0,
            top_px: 480.0,
            row_height: 12.0,
        },
    )
}

fn zoom_sweep(t: f32, arr: [f32; 2], piano: [f32; 2]) -> (View, View) {
    let ppu = 2.0 * 200_f32.powf(0.5 - 0.5 * (std::f32::consts::TAU * t).cos());
    (
        View {
            left: 200.0 - arr[0] / 2.0 / ppu,
            px_per_unit: ppu,
            top_px: 480.0,
            row_height: 32.0,
        },
        fit_all(t, arr, piano).1,
    )
}

const SCENARIOS: [Scenario; 4] = [
    Scenario {
        name: "worst case: fit-all clips + all 100k notes + vector overlay",
        overlay: true,
        drive: fit_all,
    },
    Scenario {
        name: "fit-all clips + all 100k notes, no overlay",
        overlay: false,
        drive: fit_all,
    },
    Scenario {
        name: "editing views (48 px rows, 120 px/beat) + overlay",
        overlay: true,
        drive: editing,
    },
    Scenario {
        name: "zoom sweep 2-400 px/beat + all notes + overlay",
        overlay: true,
        drive: zoom_sweep,
    },
];

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    clips: LayerId,
    notes: LayerId,
    scene: lumen_render::vello::Scene,
    adapter: String,
}

enum Phase {
    Bench {
        scenario: usize,
        frame: usize,
        cpu_ms: Vec<f32>,
        results: Vec<(&'static str, Vec<f32>)>,
    },
    Idle {
        started: Instant,
        cpu: f64,
        results: Vec<(&'static str, Vec<f32>)>,
    },
    Interactive,
}

struct App {
    gfx: Option<Gfx>,
    phase: Phase,
    startup_ms: Option<f64>,
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attributes = Window::default_attributes()
            .with_title("Lumen render shell")
            .with_inner_size(PhysicalSize::new(3_400, 1_300));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface not supported")?;
        // Vello writes gamma-encoded colour; match egui's non-sRGB target.
        let caps = surface.get_capabilities(&adapter);
        if let Some(format) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
            config.format = format;
        }
        if matches!(self.phase, Phase::Bench { .. }) {
            config.present_mode = wgpu::PresentMode::AutoNoVsync;
        }
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(&device, config.format)?;
        let clips = renderer.add_layer(&device, &demo::clip_instances());
        let notes = renderer.add_layer(&device, &demo::note_instances());
        self.gfx = Some(Gfx {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            clips,
            notes,
            scene: lumen_render::vello::Scene::new(),
            adapter: format!("{} ({:?})", info.name, info.backend),
        });
        Ok(())
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let started = Instant::now();
        let Some(gfx) = self.gfx.as_mut() else { return };
        let (w, h) = (gfx.config.width as f32, gfx.config.height as f32);
        let arr_area = Area {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h * 0.64,
        };
        let piano_area = Area {
            x: 0.0,
            y: h * 0.64,
            width: w,
            height: h * 0.36,
        };
        let (scenario, t) = match &self.phase {
            Phase::Bench {
                scenario, frame, ..
            } => (SCENARIOS[*scenario], *frame as f32 / FRAMES as f32),
            _ => (SCENARIOS[0], 0.0),
        };
        let (arr_view, piano_view) = (scenario.drive)(
            t,
            [arr_area.width, arr_area.height],
            [piano_area.width, piano_area.height],
        );
        gfx.scene.reset();
        if scenario.overlay {
            demo::vector_overlay(
                &mut gfx.scene,
                f64::from(w),
                f64::from(arr_area.height),
                f64::from(t),
            );
        }
        let layers = [
            (gfx.clips, arr_area, arr_view),
            (gfx.notes, piano_area, piano_view),
        ];
        let vector = scenario.overlay.then_some(&gfx.scene);
        if let Err(e) = gfx.renderer.prepare(
            &gfx.device,
            &gfx.queue,
            0,
            [gfx.config.width, gfx.config.height],
            &layers,
            vector,
        ) {
            eprintln!("prepare failed: {e}");
            event_loop.exit();
            return;
        }
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.106,
                            g: 0.114,
                            b: 0.133,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            gfx.renderer.paint(0, &mut pass);
        }
        gfx.queue.submit([encoder.finish()]);
        gfx.queue.present(frame);
        let cpu_ms = started.elapsed().as_secs_f32() * 1e3;
        if self.startup_ms.is_none() {
            self.startup_ms = Some(process::age_ms());
        }
        self.advance(cpu_ms, event_loop);
    }

    fn advance(&mut self, cpu_ms: f32, event_loop: &ActiveEventLoop) {
        let Some(gfx) = self.gfx.as_ref() else { return };
        match &mut self.phase {
            Phase::Bench {
                scenario,
                frame,
                cpu_ms: samples,
                results,
            } => {
                if *frame >= WARMUP {
                    samples.push(cpu_ms);
                }
                *frame += 1;
                if *frame == FRAMES {
                    results.push((SCENARIOS[*scenario].name, std::mem::take(samples)));
                    *scenario += 1;
                    *frame = 0;
                    if *scenario == SCENARIOS.len() {
                        let results = std::mem::take(results);
                        self.phase = Phase::Idle {
                            started: Instant::now(),
                            cpu: process::cpu_seconds(),
                            results,
                        };
                        event_loop.set_control_flow(ControlFlow::WaitUntil(
                            Instant::now() + Duration::from_secs(IDLE_SECONDS),
                        ));
                        return;
                    }
                }
                gfx.window.request_redraw();
            }
            Phase::Idle { .. } | Phase::Interactive => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_none() {
            if let Err(e) = self.init(event_loop) {
                eprintln!("init failed: {e}");
                event_loop.exit();
                return;
            }
            let gfx = self.gfx.as_ref().expect("initialised");
            println!(
                "GPU: {}, present mode {:?}",
                gfx.adapter, gfx.config.present_mode
            );
            gfx.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.device, &gfx.config);
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Phase::Idle {
            started,
            cpu,
            results,
        } = &self.phase
            && started.elapsed() >= Duration::from_secs(IDLE_SECONDS)
        {
            let idle_cpu = process::cpu_seconds() - cpu;
            report(results, self.startup_ms, idle_cpu, started.elapsed());
            self.phase = Phase::Interactive;
            event_loop.exit();
        }
    }
}

fn report(
    results: &[(&'static str, Vec<f32>)],
    startup_ms: Option<f64>,
    idle_cpu: f64,
    idle: Duration,
) {
    println!(
        "\nP-RENDER, bare winit host. cpu = whole frame on the CPU (scene build, Vello encode, prepare, submit, present), in ms.\n"
    );
    println!(
        "{:<60} | {:>6} {:>6} {:>6} {:>6}",
        "scenario", "cpu50", "cpu95", "cpu99", "cpuMx"
    );
    for (name, samples) in results {
        let mut s = samples.clone();
        s.sort_by(f32::total_cmp);
        let at = |p: f64| s[((s.len() - 1) as f64 * p) as usize];
        println!(
            "{name:<60} | {:>6.2} {:>6.2} {:>6.2} {:>6.2}",
            at(0.5),
            at(0.95),
            at(0.99),
            at(1.0)
        );
    }
    println!(
        "\nstart-up: first frame presented {:.0} ms after process creation",
        startup_ms.unwrap_or(f64::NAN)
    );
    println!(
        "idle: {idle_cpu:.3} s CPU over {:.1} s = {:.2}% of one core",
        idle.as_secs_f64(),
        idle_cpu / idle.as_secs_f64() * 100.0
    );
}

fn main() {
    let bench = std::env::args().any(|a| a == "--bench");
    let phase = if bench {
        Phase::Bench {
            scenario: 0,
            frame: 0,
            cpu_ms: Vec::with_capacity(FRAMES),
            results: Vec::new(),
        }
    } else {
        Phase::Interactive
    };
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        gfx: None,
        phase,
        startup_ms: None,
    };
    event_loop.run_app(&mut app).expect("run");
}
