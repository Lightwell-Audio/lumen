//! Canvases drawn by `lumen-render` inside egui (ADR-0002). egui owns the
//! window and widgets; dense content goes through paint callbacks that share
//! egui's wgpu device.

use std::path::PathBuf;
use std::sync::Mutex;

use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, wgpu};
use lumen_core::product::PRODUCT_NAME;
use lumen_render::{Area, LayerId, RectInstance, Renderer, View, rgba};

/// Bars drawn in the empty arrangement grid.
pub const GRID_BARS: usize = 512;

struct Resources {
    renderer: Renderer,
    grid: LayerId,
}

/// Where compiled GPU pipelines are cached between launches.
fn pipeline_cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let dir = PathBuf::from(base).join(PRODUCT_NAME).join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpu-pipelines.bin"))
}

/// Bar lines (strong) and beat lines (faint) for a 4/4 grid; one row that
/// spans the canvas height.
fn grid_instances() -> Vec<RectInstance> {
    (0..GRID_BARS * 4)
        .map(|beat| {
            let bar = beat % 4 == 0;
            let x = beat as f32;
            RectInstance {
                start: x,
                end: x,
                row: 0.0,
                color: if bar {
                    rgba(0x34, 0x38, 0x42, 255)
                } else {
                    rgba(0x26, 0x29, 0x30, 255)
                },
            }
        })
        .collect()
}

pub fn install(state: &egui_wgpu::RenderState) -> Result<(), String> {
    let path = pipeline_cache_path();
    let (cache, _) = match &path {
        Some(p) => lumen_render::open_pipeline_cache(&state.device, p),
        None => (None, lumen_render::CacheState::Unsupported),
    };
    let mut renderer = Renderer::new(&state.device, state.target_format, cache.clone())?;
    if let Some(p) = &path {
        lumen_render::save_pipeline_cache(cache.as_ref(), p);
    }
    let grid = renderer.add_layer(&state.device, &grid_instances());
    state
        .renderer
        .write()
        .callback_resources
        .insert(Mutex::new(Resources { renderer, grid }));
    Ok(())
}

struct GridCallback {
    view: View,
    size_px: [u32; 2],
}

impl egui_wgpu::CallbackTrait for GridCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(Ok(r)) = resources.get_mut::<Mutex<Resources>>().map(Mutex::get_mut) else {
            return Vec::new();
        };
        let [w, h] = self.size_px;
        let area = Area {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        };
        let view = View {
            row_height: h as f32,
            top_px: 0.0,
            ..self.view
        };
        if let Err(e) = r.renderer.prepare(
            device,
            queue,
            0,
            self.size_px,
            &[(r.grid, area, view)],
            None,
        ) {
            eprintln!("arrangement grid: {e}");
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(lock) = resources.get::<Mutex<Resources>>()
            && let Ok(r) = lock.lock()
        {
            r.renderer.paint(0, pass);
        }
    }
}

/// Paint shape for the arrangement grid in `rect`, scrolled to `left_beat`
/// at `px_per_beat` (both in egui points).
pub fn grid_shape(
    rect: Rect,
    pixels_per_point: f32,
    left_beat: f32,
    px_per_beat: f32,
) -> egui::Shape {
    let callback = GridCallback {
        view: View {
            left: left_beat,
            px_per_unit: px_per_beat * pixels_per_point,
            top_px: 0.0,
            row_height: 1.0,
        },
        size_px: [
            (rect.width() * pixels_per_point).round().max(1.0) as u32,
            (rect.height() * pixels_per_point).round().max(1.0) as u32,
        ],
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
}
