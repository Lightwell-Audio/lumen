//! P-RENDER host 1: `lumen-render` inside egui. egui keeps windows, widgets,
//! text and accessibility; canvases are drawn by Lumen's renderer through
//! egui paint callbacks, sharing egui's wgpu device.

use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, wgpu};
use lumen_render::{Area, LayerId, Renderer, View, demo};

use crate::paint;

struct Resources {
    renderer: Renderer,
    clips: LayerId,
    notes: LayerId,
    scene: lumen_render::vello::Scene,
}

pub fn install(state: &egui_wgpu::RenderState) -> Result<(), String> {
    // The device was created with PIPELINE_CACHE (see main.rs), so Vello's
    // shaders can be reused across launches.
    let path = std::env::temp_dir().join("lumen-vello-pipeline-egui.cache");
    let (cache, cache_state) =
        lumen_render::open_pipeline_cache(&state.device, std::fs::read(&path).ok());
    let started = std::time::Instant::now();
    let mut renderer = Renderer::new(&state.device, state.target_format, cache.clone())?;
    println!(
        "lumen-render init: {:.0} ms (pipeline cache: {cache_state:?})",
        started.elapsed().as_secs_f64() * 1e3
    );
    if let Some(data) = lumen_render::pipeline_cache_data(cache.as_ref()) {
        let _ = std::fs::write(&path, data);
    }
    let clips = renderer.add_layer(&state.device, &demo::clip_instances());
    let notes = renderer.add_layer(&state.device, &demo::note_instances());
    state
        .renderer
        .write()
        .callback_resources
        .insert(std::sync::Mutex::new(Resources {
            renderer,
            clips,
            notes,
            scene: lumen_render::vello::Scene::new(),
        }));
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Layer {
    Clips,
    Notes,
}

struct Callback {
    target: usize,
    layer: Layer,
    view: View,
    size_px: [u32; 2],
    overlay_t: Option<f64>,
}

impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(lock) = resources.get_mut::<std::sync::Mutex<Resources>>() else {
            return Vec::new();
        };
        // Exclusive access here, so no locking is needed.
        let Ok(r) = lock.get_mut() else {
            return Vec::new();
        };
        let [w, h] = self.size_px;
        r.scene.reset();
        if let Some(t) = self.overlay_t {
            demo::vector_overlay(&mut r.scene, f64::from(w), f64::from(h), t);
        }
        let id = match self.layer {
            Layer::Clips => r.clips,
            Layer::Notes => r.notes,
        };
        let area = Area {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        };
        let vector = self.overlay_t.map(|_| &r.scene);
        if let Err(e) = r.renderer.prepare(
            device,
            queue,
            self.target,
            self.size_px,
            &[(id, area, self.view)],
            vector,
        ) {
            eprintln!("lumen-render prepare failed: {e}");
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(lock) = resources.get::<std::sync::Mutex<Resources>>()
            && let Ok(r) = lock.lock()
        {
            r.renderer.paint(self.target, pass);
        }
    }
}

/// A paint shape drawing `layer` in `rect` under `view` (egui points), with
/// an optional animated vector overlay at time `overlay_t`.
pub fn shape(
    rect: Rect,
    pixels_per_point: f32,
    target: usize,
    layer: Layer,
    view: &paint::View,
    overlay_t: Option<f64>,
) -> egui::Shape {
    let ppp = pixels_per_point;
    let callback = Callback {
        target,
        layer,
        view: View {
            left: f64::from(view.left_beat),
            px_per_unit: view.px_per_beat * ppp,
            top_px: view.top_px * ppp,
            row_height: view.row_h * ppp,
        },
        size_px: [
            (rect.width() * ppp).round().max(1.0) as u32,
            (rect.height() * ppp).round().max(1.0) as u32,
        ],
        overlay_t,
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
}
