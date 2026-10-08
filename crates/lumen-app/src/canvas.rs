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
use lumen_core::grid::{Grid, Level, Step};
use lumen_core::meter::MeterMap;
use lumen_core::time::{PPQ, Ticks};
use lumen_render::{
    Area, BoxDraw, BoxInstance, BoxLayerId, BoxStyle, LayerId, RectInstance, Renderer, RowSlot,
    View, WaveInstance, WaveStyle, rgba,
};

struct Resources {
    renderer: Renderer,
    grid: LayerId,
    /// The lines the grid layer holds.
    grid_lines: Option<GridLines>,
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

/// What the arrangement grid shows: the editing grid, the time signatures
/// it counts from, and the line colours (theme tokens `grid_bar`,
/// `grid_beat`, `grid_sub`).
#[derive(Debug, Clone, PartialEq)]
pub struct GridLook {
    pub grid: Grid,
    pub meter: MeterMap,
    pub colors: [egui::Color32; 3],
}

/// The grid lines around the view, covering a few screens so scrolling
/// rarely rebuilds them. The lines depend on the zoom only through the
/// step drawn in each time signature (`steps`), so zooming rebuilds them
/// only when a step changes.
#[derive(Debug, Clone, PartialEq)]
struct GridLines {
    look: GridLook,
    steps: Vec<Step>,
    from: Ticks,
    to: Ticks,
    zoom: f64,
}

impl GridLines {
    /// The lines for `time` across `width` points: whole windows of a
    /// power of two quarter notes, one before the view and two after it.
    fn around(time: TimeView, width: f32, look: &GridLook) -> Self {
        let steps = look
            .meter
            .points()
            .iter()
            .map(|p| look.grid.display_step(p.meter, time.zoom))
            .collect();
        let span = (f64::from(width) / time.zoom).max(1e-9);
        let window = 2_f64.powi(span.log2().ceil() as i32);
        let from = ((time.left / window).floor() - 1.0).max(0.0) * window;
        let ticks = |quarters: f64| Ticks((quarters * PPQ as f64).round() as i64);
        Self {
            look: look.clone(),
            steps,
            from: ticks(from),
            to: ticks(from + 4.0 * window),
            zoom: time.zoom,
        }
    }

    /// One line instance per grid line; one row spans the canvas height.
    fn instances(&self) -> Vec<RectInstance> {
        let [bar, beat, sub] = self.look.colors;
        let mut instances: Vec<RectInstance> = self
            .look
            .grid
            .lines(&self.look.meter, self.from, self.to, self.zoom)
            .into_iter()
            .map(|line| {
                let c = match line.level {
                    Level::Bar => bar,
                    Level::Beat => beat,
                    Level::Sub => sub,
                };
                let at = line.at.as_quarters();
                RectInstance::new(at, at, 0.0, rgba(c.r(), c.g(), c.b(), 255))
            })
            .collect();
        // A layer is never empty.
        if instances.is_empty() {
            instances.push(RectInstance::new(0.0, 0.0, 0.0, 0));
        }
        instances
    }

    /// Whether these lines also do for `wanted`: the same grid, steps and
    /// colours, over a range that covers it.
    fn covers(&self, wanted: &Self) -> bool {
        self.look == wanted.look
            && self.steps == wanted.steps
            && self.from <= wanted.from
            && wanted.to <= self.to
    }
}

/// Where compiled GPU pipelines are cached between launches.
fn pipeline_cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let dir = PathBuf::from(base).join(PRODUCT_NAME).join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpu-pipelines.bin"))
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
    // Filled in on the first frame (`GridLines`); a buffer cannot be empty.
    let grid = renderer.add_layer(&state.device, &[RectInstance::new(0.0, 0.0, 0.0, 0)]);
    let clips = renderer.add_box_layer(&state.device);
    state
        .renderer
        .write()
        .callback_resources
        .insert(Mutex::new(Resources {
            renderer,
            grid,
            grid_lines: None,
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
    lines: GridLines,
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
        if !r.grid_lines.as_ref().is_some_and(|l| l.covers(&self.lines)) {
            r.renderer
                .update_layer(device, queue, r.grid, &self.lines.instances());
            r.grid_lines = Some(self.lines.clone());
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
    look: &GridLook,
) -> egui::Shape {
    let callback = GridCallback {
        lines: GridLines::around(time, rect.width(), look),
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

    fn look(grid: Grid) -> GridLook {
        GridLook {
            grid,
            meter: MeterMap::constant(lumen_core::meter::Meter::FOUR_FOUR).unwrap(),
            colors: [
                egui::Color32::WHITE,
                egui::Color32::GRAY,
                egui::Color32::DARK_GRAY,
            ],
        }
    }

    #[test]
    fn grid_lines_cover_the_view_wherever_it_is() {
        // Bar 2,000, 24 points a beat: 1,000 points show about 10 bars; the
        // Narrow grid draws eighths there.
        let view = TimeView {
            left: 8_000.0,
            zoom: 24.0,
        };
        let look = look(Grid::default());
        let lines = GridLines::around(view, 1_000.0, &look);
        assert!(lines.from <= Ticks::quarters(8_000) && Ticks::quarters(8_042) <= lines.to);
        let instances = lines.instances();
        let quarters = (lines.to - lines.from).as_quarters();
        assert_eq!(instances.len() as f64, quarters * 2.0, "an eighth apart");
        // Scrolling a little: the same lines do; zooming within the same
        // step does too.
        let moved = TimeView {
            left: 8_001.0,
            zoom: 25.0,
        };
        assert!(lines.covers(&GridLines::around(moved, 1_000.0, &look)));
        // A different grid does not.
        let triplet = self::look(Grid {
            feel: lumen_core::grid::Feel::Triplet,
            ..Grid::default()
        });
        assert!(!lines.covers(&GridLines::around(view, 1_000.0, &triplet)));
    }

    #[test]
    fn grid_lines_stay_calm_far_out() {
        // 0.1 points a beat: every 32 bars (12.8 points).
        let far = TimeView {
            left: 0.0,
            zoom: 0.1,
        };
        let lines = GridLines::around(far, 1_000.0, &look(Grid::default()));
        let at: Vec<f64> = lines
            .look
            .grid
            .lines(&lines.look.meter, lines.from, lines.to, far.zoom)
            .iter()
            .map(|l| l.at.as_quarters())
            .collect();
        assert_eq!(at[1] - at[0], 128.0);
    }

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
