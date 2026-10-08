//! Canvases drawn by `lumen-render` inside egui (ADR-0002). egui owns the
//! window and widgets; dense content goes through paint callbacks that share
//! egui's wgpu device: the arrangement grid, and clip bodies (M3 slice 4b,
//! ADR-0026).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, wgpu};
use lumen_core::product::PRODUCT_NAME;
use lumen_media::Peaks;

use crate::timeview::TimeView;
use lumen_render::{
    Area, BoxDraw, BoxInstance, BoxLayerId, BoxStyle, LayerId, RectInstance, Renderer, RowSlot,
    View, WaveInstance, WaveStyle, rgba,
};

/// Bars drawn in the empty arrangement grid.
pub const GRID_BARS: usize = 512;

struct Resources {
    renderer: Renderer,
    grid: LayerId,
    /// The meter the grid layer was built for.
    grid_meter: GridMeter,
    clips: BoxLayerId,
    /// The generation of clip boxes and waves on the GPU.
    clips_generation: Option<u64>,
    /// Peak slots already in the atlas.
    peaks_uploaded: HashSet<u32>,
    /// The atlas is full: waves of further files are not drawn.
    atlas_full: bool,
}

/// Drawing targets: each paint callback prepares its own.
const GRID_TARGET: usize = 0;
const CLIPS_TARGET: usize = 1;

/// Bar and beat lengths, in quarter notes, for drawing the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridMeter {
    pub bar: f32,
    pub beat: f32,
    /// Bar and beat line colours (theme tokens `grid_bar`, `grid_beat`).
    pub colors: [egui::Color32; 2],
}

impl GridMeter {
    const FOUR_FOUR: GridMeter = GridMeter {
        bar: 4.0,
        beat: 1.0,
        colors: [crate::theme::DARK.grid_bar, crate::theme::DARK.grid_beat],
    };
}

/// Where compiled GPU pipelines are cached between launches.
fn pipeline_cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let dir = PathBuf::from(base).join(PRODUCT_NAME).join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpu-pipelines.bin"))
}

/// Bar lines (strong) and beat lines (faint), in quarter notes; one row that
/// spans the canvas height. Rebuilt when the meter or the theme changes.
fn grid_instances(meter: GridMeter) -> Vec<RectInstance> {
    let beats_per_bar = (meter.bar / meter.beat).round().max(1.0) as usize;
    (0..GRID_BARS * beats_per_bar)
        .map(|beat| {
            let bar = beat % beats_per_bar == 0;
            let x = beat as f32 * meter.beat;
            let c = meter.colors[usize::from(!bar)];
            RectInstance::new(
                f64::from(x),
                f64::from(x),
                0.0,
                rgba(c.r(), c.g(), c.b(), 255),
            )
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
    let grid_meter = GridMeter::FOUR_FOUR;
    let grid = renderer.add_layer(&state.device, &grid_instances(grid_meter));
    let clips = renderer.add_box_layer(&state.device);
    state
        .renderer
        .write()
        .callback_resources
        .insert(Mutex::new(Resources {
            renderer,
            grid,
            grid_meter,
            clips,
            clips_generation: None,
            peaks_uploaded: HashSet::new(),
            atlas_full: false,
        }));
    Ok(())
}

struct GridCallback {
    view: View,
    port: Port,
    meter: GridMeter,
}

/// A callback's viewport, as egui-wgpu sets it: each edge rounded to a
/// whole pixel. `offset` is where the callback's rect really starts
/// within it (under a pixel), so GPU content rounds to the same pixels as
/// egui's own shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Port {
    size_px: [u32; 2],
    offset: [f32; 2],
}

impl Port {
    fn of(rect: Rect, pixels_per_point: f32) -> Self {
        let edge = |v: f32| (v * pixels_per_point).round();
        let (left, top) = (edge(rect.left()), edge(rect.top()));
        Self {
            size_px: [
                (edge(rect.right()) - left).max(1.0) as u32,
                (edge(rect.bottom()) - top).max(1.0) as u32,
            ],
            offset: [
                rect.left() * pixels_per_point - left,
                rect.top() * pixels_per_point - top,
            ],
        }
    }

    fn area(self) -> Area {
        Area {
            x: self.offset[0],
            y: self.offset[1],
            width: self.size_px[0] as f32,
            height: self.size_px[1] as f32,
        }
    }
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
        if r.grid_meter != self.meter {
            r.renderer
                .update_layer(device, queue, r.grid, &grid_instances(self.meter));
            r.grid_meter = self.meter;
        }
        let view = View {
            row_height: self.port.size_px[1] as f32,
            top_px: 0.0,
            ..self.view
        };
        let area = self.port.area();
        if let Err(e) = r.renderer.prepare(
            device,
            queue,
            GRID_TARGET,
            self.port.size_px,
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
            r.renderer.paint(GRID_TARGET, pass);
        }
    }
}

/// Paint shape for the arrangement grid in `rect`, at `time`.
pub fn grid_shape(
    rect: Rect,
    pixels_per_point: f32,
    time: TimeView,
    meter: GridMeter,
) -> egui::Shape {
    let callback = GridCallback {
        meter,
        view: View {
            left: time.left,
            px_per_unit: (time.zoom * f64::from(pixels_per_point)) as f32,
            top_px: 0.0,
            row_height: 1.0,
        },
        port: Port::of(rect, pixels_per_point),
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
}

/// Clip bodies and waveforms for the GPU: the boxes and waves, rebuilt
/// only when they change (`generation` says when), and where each row is
/// this frame.
pub struct ClipFrame {
    /// Samples for waves drawn sample by sample, when they change.
    pub strips: Option<crate::arrangement::StripUpdate>,
    pub boxes: Arc<Vec<BoxInstance>>,
    pub waves: Arc<Vec<WaveInstance>>,
    /// Every peak slot the waves may use; the GPU side uploads the ones it
    /// does not have yet.
    pub peaks: Arc<Vec<(u32, Arc<Peaks>)>>,
    pub generation: u64,
    pub rows: Vec<RowSlot>,
    /// Scroll and zoom; the vertical scroll in points.
    pub time: TimeView,
    pub top: f32,
    /// Clip sizes, in physical pixels ([`box_style`]).
    pub style: BoxStyle,
    pub wave_style: WaveStyle,
}

struct ClipsCallback {
    frame: ClipFrame,
    pixels_per_point: f32,
    port: Port,
}

impl egui_wgpu::CallbackTrait for ClipsCallback {
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
        let frame = &self.frame;
        for (slot, peaks) in frame.peaks.iter() {
            if r.atlas_full || r.peaks_uploaded.contains(slot) {
                continue;
            }
            let levels: Vec<&[[i8; 4]]> = peaks.levels().iter().map(Vec::as_slice).collect();
            if r.renderer.set_peaks(device, queue, r.clips, *slot, &levels) {
                r.peaks_uploaded.insert(*slot);
            } else {
                crate::trace::log(|| "waveforms: the peak atlas is full".into());
                r.atlas_full = true;
            }
        }
        if r.clips_generation != Some(frame.generation) {
            r.renderer
                .update_boxes(device, queue, r.clips, &frame.boxes);
            r.renderer
                .update_waves(device, queue, r.clips, &frame.waves);
            r.clips_generation = Some(frame.generation);
        }
        r.renderer.update_rows(device, queue, r.clips, &frame.rows);
        if let Some(strips) = &frame.strips {
            let writes: Vec<(u32, &[[i16; 2]])> = strips
                .writes
                .iter()
                .map(|(at, samples)| (*at, samples.as_slice()))
                .collect();
            r.renderer
                .update_strips(device, queue, r.clips, &strips.table, &writes);
        }
        let ppp = self.pixels_per_point;
        let area = self.port.area();
        let view = View {
            left: frame.time.left,
            px_per_unit: (frame.time.zoom * f64::from(ppp)) as f32,
            top_px: frame.top * ppp,
            row_height: 0.0,
        };
        r.renderer.prepare_boxes(
            device,
            CLIPS_TARGET,
            self.port.size_px,
            &[BoxDraw {
                layer: r.clips,
                area,
                view,
                style: frame.style,
                waves: frame.wave_style,
            }],
        );
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
            r.renderer.paint(CLIPS_TARGET, pass);
        }
    }
}

/// Paint shape for clip bodies in `rect` (the lanes).
pub fn clips_shape(rect: Rect, pixels_per_point: f32, frame: ClipFrame) -> egui::Shape {
    let callback = ClipsCallback {
        frame,
        pixels_per_point,
        port: Port::of(rect, pixels_per_point),
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
}

/// How clip bodies are drawn, in physical pixels: the same sizes as the
/// design system's clip (DESIGN §7a).
pub fn box_style(pixels_per_point: f32, inset: f32, title: f32) -> BoxStyle {
    BoxStyle {
        inset: inset * pixels_per_point,
        radius: f32::from(crate::theme::radius::SMALL) * pixels_per_point,
        title: title * pixels_per_point,
        border: pixels_per_point,
        min_width: 2.0 * pixels_per_point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_rounds_like_egui_wgpu_and_keeps_the_remainder() {
        let rect = Rect::from_min_max(egui::pos2(10.3, 20.6), egui::pos2(110.3, 70.6));
        let port = Port::of(rect, 1.5);
        // Edges at 15.45, 30.9, 165.45 and 105.9 pixels round to 15, 31,
        // 165 and 106.
        assert_eq!(port.size_px, [150, 75]);
        assert!((port.offset[0] - 0.45).abs() < 1e-4);
        assert!((port.offset[1] + 0.1).abs() < 1e-4);
    }
}
