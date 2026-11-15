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
    grid_lines: Option<GridLines>,
    clips: BoxLayerId,
    clips_generation: Option<u64>,
    peaks_uploaded: HashSet<u32>,
    atlas_full: bool,
    peaks_checked: usize,
    mini: BoxLayerId,
    mini_rows: Vec<RowSlot>,
    live: BoxLayerId,
    live_mini: BoxLayerId,
    live_generation: Option<u64>,
    live_mini_rows: Vec<RowSlot>,
    rolls: [Roll; 2],
}

struct Roll {
    grid: LayerId,
    grid_lines: Option<GridLines>,
    notes: BoxLayerId,
    notes_generation: Option<u64>,
    row_height: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    Docked,
    Window,
}

impl Surface {
    fn index(self) -> usize {
        self as usize
    }
    fn grid_target(self) -> usize {
        ROLL_TARGETS + 2 * self.index()
    }
    fn notes_target(self) -> usize {
        self.grid_target() + 1
    }
}

const GRID_TARGET: usize = 0;
const CLIPS_TARGET: usize = 1;
const ROLL_TARGETS: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct GridLook {
    pub grid: Grid,
    pub meter: MeterMap,
    pub colors: [egui::Color32; 3],
}

#[derive(Debug, Clone, PartialEq)]
struct GridLines {
    look: GridLook,
    steps: Vec<Step>,
    from: Ticks,
    to: Ticks,
    zoom: f64,
}

impl GridLines {
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
        if instances.is_empty() {
            instances.push(RectInstance::new(0.0, 0.0, 0.0, 0));
        }
        instances
    }

    fn covers(&self, wanted: &Self) -> bool {
        self.look == wanted.look
            && self.steps == wanted.steps
            && self.from <= wanted.from
            && wanted.to <= self.to
    }
}

pub struct PipelineCache {
    path: PathBuf,
    read: std::thread::JoinHandle<Option<Vec<u8>>>,
}

impl PipelineCache {
    pub fn prefetch() -> Option<Self> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        let path = PathBuf::from(base)
            .join(PRODUCT_NAME)
            .join("cache")
            .join("gpu-pipelines.bin");
        let file = path.clone();
        let read = std::thread::Builder::new()
            .name("lumen-pipeline-cache".into())
            .spawn(move || std::fs::read(file).ok())
            .ok()?;
        Some(Self { path, read })
    }

    fn data(self) -> Option<Vec<u8>> {
        self.read.join().ok().flatten()
    }

    fn store(path: PathBuf, data: Vec<u8>) {
        let _ = std::thread::Builder::new()
            .name("lumen-pipeline-cache".into())
            .spawn(move || {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, data);
            });
    }
}

pub fn install(
    state: &egui_wgpu::RenderState,
    pipelines: Option<PipelineCache>,
) -> Result<(), String> {
    let pipelines = pipelines.filter(|_| lumen_render::caches_pipelines(&state.device));
    let path = pipelines.as_ref().map(|p| p.path.clone());
    let (cache, _) =
        lumen_render::open_pipeline_cache(&state.device, pipelines.and_then(PipelineCache::data));
    let mut renderer = Renderer::new(&state.device, state.target_format, cache.clone())?;
    if let (Some(path), Some(data)) = (path, lumen_render::pipeline_cache_data(cache.as_ref())) {
        PipelineCache::store(path, data);
    }
    let grid = renderer.add_layer(&state.device, &[RectInstance::new(0.0, 0.0, 0.0, 0)]);
    let clips = renderer.add_box_layer(&state.device);
    let mini = renderer.add_box_layer(&state.device);
    let live = renderer.add_box_layer(&state.device);
    let live_mini = renderer.add_box_layer(&state.device);
    let roll = |renderer: &mut Renderer| Roll {
        grid: renderer.add_layer(&state.device, &[RectInstance::new(0.0, 0.0, 0.0, 0)]),
        grid_lines: None,
        notes: renderer.add_box_layer(&state.device),
        notes_generation: None,
        row_height: None,
    };
    let rolls = [roll(&mut renderer), roll(&mut renderer)];
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
            peaks_checked: 0,
            mini,
            mini_rows: Vec::new(),
            live,
            live_mini,
            live_generation: None,
            live_mini_rows: Vec::new(),
            rolls,
        }));
    Ok(())
}

struct GridCallback {
    view: View,
    port: Port,
    lines: GridLines,
    surface: Option<Surface>,
}

impl GridCallback {
    fn target(&self) -> usize {
        self.surface.map_or(GRID_TARGET, Surface::grid_target)
    }
}

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
        let (layer, cache, target) = match self.surface {
            None => (r.grid, &mut r.grid_lines, GRID_TARGET),
            Some(s) => {
                let roll = &mut r.rolls[s.index()];
                (roll.grid, &mut roll.grid_lines, s.grid_target())
            }
        };
        if !cache.as_ref().is_some_and(|l| l.covers(&self.lines)) {
            r.renderer
                .update_layer(device, queue, layer, &self.lines.instances());
            *cache = Some(self.lines.clone());
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
            target,
            self.port.size_px,
            &[(layer, area, view)],
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
            r.renderer.paint(self.target(), pass);
        }
    }
}

pub fn grid_shape(
    rect: Rect,
    pixels_per_point: f32,
    time: TimeView,
    look: &GridLook,
) -> egui::Shape {
    grid_shape_on(None, rect, pixels_per_point, time, look)
}

pub(crate) fn roll_grid_shape(
    surface: Surface,
    rect: Rect,
    pixels_per_point: f32,
    time: TimeView,
    look: &GridLook,
) -> egui::Shape {
    grid_shape_on(Some(surface), rect, pixels_per_point, time, look)
}

fn grid_shape_on(
    surface: Option<Surface>,
    rect: Rect,
    pixels_per_point: f32,
    time: TimeView,
    look: &GridLook,
) -> egui::Shape {
    let callback = GridCallback {
        surface,
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

pub struct ClipFrame {
    pub strips: Option<crate::arrangement::StripUpdate>,
    pub boxes: Arc<Vec<BoxInstance>>,
    pub waves: Arc<Vec<WaveInstance>>,
    pub peaks: Arc<Vec<(u32, Arc<Peaks>)>>,
    pub generation: u64,
    pub rows: Vec<RowSlot>,
    pub time: TimeView,
    pub top: f32,
    pub style: BoxStyle,
    pub wave_style: WaveStyle,
    pub mini: Arc<Vec<BoxInstance>>,
    pub mini_rows: Vec<RowSlot>,
    pub mini_style: BoxStyle,
    pub live: Arc<Vec<BoxInstance>>,
    pub live_mini: Arc<Vec<BoxInstance>>,
    pub live_mini_rows: Vec<RowSlot>,
    pub live_generation: u64,
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
        let list = Arc::as_ptr(&frame.peaks) as usize;
        if r.peaks_checked != list
            && r.peaks_uploaded
                .iter()
                .any(|slot| !frame.peaks.iter().any(|(s, _)| s == slot))
        {
            crate::trace::log(|| "waveforms: the peak atlas is rebuilt".into());
            r.renderer.clear_peaks(queue, r.clips);
            r.peaks_uploaded.clear();
            r.atlas_full = false;
        }
        r.peaks_checked = list;
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
            r.renderer.update_boxes(device, queue, r.mini, &frame.mini);
            r.clips_generation = Some(frame.generation);
        }
        if r.live_generation != Some(frame.live_generation) {
            r.renderer.update_boxes(device, queue, r.live, &frame.live);
            r.renderer
                .update_boxes(device, queue, r.live_mini, &frame.live_mini);
            r.live_generation = Some(frame.live_generation);
        }
        r.renderer.update_rows(device, queue, r.clips, &frame.rows);
        if !frame.live.is_empty() {
            r.renderer.update_rows(device, queue, r.live, &frame.rows);
        }
        if r.live_mini_rows != frame.live_mini_rows {
            r.renderer
                .update_rows(device, queue, r.live_mini, &frame.live_mini_rows);
            r.live_mini_rows.clone_from(&frame.live_mini_rows);
        }
        if r.mini_rows != frame.mini_rows {
            r.renderer
                .update_rows(device, queue, r.mini, &frame.mini_rows);
            r.mini_rows.clone_from(&frame.mini_rows);
        }
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
            &[
                BoxDraw {
                    layer: r.clips,
                    area,
                    view,
                    style: frame.style,
                    waves: frame.wave_style,
                },
                BoxDraw {
                    layer: r.mini,
                    area,
                    view,
                    style: frame.mini_style,
                    waves: frame.wave_style,
                },
                BoxDraw {
                    layer: r.live,
                    area,
                    view,
                    style: frame.style,
                    waves: frame.wave_style,
                },
                BoxDraw {
                    layer: r.live_mini,
                    area,
                    view,
                    style: frame.mini_style,
                    waves: frame.wave_style,
                },
            ],
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

pub fn clips_shape(rect: Rect, pixels_per_point: f32, frame: ClipFrame) -> egui::Shape {
    let callback = ClipsCallback {
        frame,
        pixels_per_point,
        port: Port::of(rect, pixels_per_point),
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
}

pub fn box_style(pixels_per_point: f32, inset: f32, title: f32) -> BoxStyle {
    BoxStyle {
        inset: inset * pixels_per_point,
        radius: f32::from(crate::theme::radius::SMALL) * pixels_per_point,
        title: title * pixels_per_point,
        border: pixels_per_point,
        min_width: 2.0 * pixels_per_point,
    }
}

pub fn mini_style(pixels_per_point: f32) -> BoxStyle {
    BoxStyle {
        inset: 0.0,
        radius: pixels_per_point,
        title: 0.0,
        border: 0.0,
        min_width: pixels_per_point.max(1.0),
    }
}

pub(crate) struct NoteFrame {
    pub boxes: Arc<Vec<BoxInstance>>,
    pub generation: u64,
    pub time: TimeView,
    pub scroll: f32,
    pub row_height: f32,
    pub style: BoxStyle,
}

pub(crate) fn note_style(pixels_per_point: f32) -> BoxStyle {
    BoxStyle {
        inset: 0.5 * pixels_per_point,
        radius: f32::from(crate::theme::radius::SMALL) * pixels_per_point / 2.0,
        title: 1.0,
        border: 1.5 * pixels_per_point,
        min_width: 2.0 * pixels_per_point,
    }
}

struct NotesCallback {
    frame: NoteFrame,
    surface: Surface,
    pixels_per_point: f32,
    port: Port,
}

impl egui_wgpu::CallbackTrait for NotesCallback {
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
        let ppp = self.pixels_per_point;
        let roll = &mut r.rolls[self.surface.index()];
        if roll.notes_generation != Some(frame.generation) {
            r.renderer
                .update_boxes(device, queue, roll.notes, &frame.boxes);
            roll.notes_generation = Some(frame.generation);
        }
        let row = frame.row_height * ppp;
        if roll.row_height != Some(row) {
            let rows: Vec<RowSlot> = (0..crate::piano_roll::PITCHES)
                .map(|pitch| {
                    let from_top = (crate::piano_roll::PITCHES - 1 - pitch) as f32;
                    RowSlot::new(from_top * row, row, 1.0)
                })
                .collect();
            r.renderer.update_rows(device, queue, roll.notes, &rows);
            roll.row_height = Some(row);
        }
        let view = View {
            left: frame.time.left,
            px_per_unit: (frame.time.zoom * f64::from(ppp)) as f32,
            top_px: frame.scroll * ppp,
            row_height: 0.0,
        };
        r.renderer.prepare_boxes(
            device,
            self.surface.notes_target(),
            self.port.size_px,
            &[BoxDraw {
                layer: roll.notes,
                area: self.port.area(),
                view,
                style: frame.style,
                waves: WaveStyle {
                    top: 0.0,
                    bottom: 0.0,
                    side: 0.0,
                    split_below: 0.0,
                },
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
            r.renderer.paint(self.surface.notes_target(), pass);
        }
    }
}

pub(crate) fn notes_shape(
    surface: Surface,
    rect: Rect,
    pixels_per_point: f32,
    frame: NoteFrame,
) -> egui::Shape {
    let callback = NotesCallback {
        frame,
        surface,
        pixels_per_point,
        port: Port::of(rect, pixels_per_point),
    };
    egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, callback))
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
        let moved = TimeView {
            left: 8_001.0,
            zoom: 25.0,
        };
        assert!(lines.covers(&GridLines::around(moved, 1_000.0, &look)));
        let triplet = self::look(Grid {
            feel: lumen_core::grid::Feel::Triplet,
            ..Grid::default()
        });
        assert!(!lines.covers(&GridLines::around(view, 1_000.0, &triplet)));
    }

    #[test]
    fn grid_lines_stay_calm_far_out() {
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
        assert_eq!(port.size_px, [150, 75]);
        assert!((port.offset[0] - 0.45).abs() < 1e-4);
        assert!((port.offset[1] + 0.1).abs() < 1e-4);
    }
}
