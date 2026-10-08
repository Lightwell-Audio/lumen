//! `lumen-render`: Lumen's own renderer, independent of any UI toolkit (ADR-0002).
//!
//! Dense content (clips, notes; later waveforms and automation) lives on the
//! GPU as instanced rectangles, uploaded once and updated only on edits. A
//! frame writes one small view uniform per layer. Vector graphics (curves,
//! knobs, meters, icons) are rasterised by Vello into a texture and
//! composited. A host (egui callback or a bare winit window) calls
//! [`Renderer::prepare`] with the device and queue, then [`Renderer::paint`]
//! inside its render pass.

mod boxes;
pub mod demo;
mod waves;

/// The WGSL shaders in `src/shaders`, compiled and checked by `build.rs`:
/// each one's source, and the size of its uniform block.
#[allow(dead_code)] // generated: not every shader has a uniform to check
mod shaders {
    include!(concat!(env!("OUT_DIR"), "/shaders.rs"));
}

// The uniform structs match their shaders, or the build fails.
const _: () = assert!(size_of::<LayerUniform>() == shaders::RECTS_UNIFORM);

pub use boxes::{BoxDraw, BoxInstance, BoxLayerId, BoxStyle, RowSlot};
pub use waves::{
    FADE_FAST, FADE_LINEAR, FADE_S_CURVE, FADE_SLOW, MAX_LEVELS, PEAK_BASE, PEAK_FACTOR,
    STRIP_CAPACITY, StripEntry, WaveInstance, WaveStyle,
};

use bytemuck::{Pod, Zeroable};
use vello::wgpu;

pub use vello;

/// A position in timeline units (beats), split into whole and fractional
/// parts so the GPU keeps sub-pixel precision at sample-level zoom deep into
/// a long song (see [`beats`]).
pub type Beats = [f32; 2];

/// `b` as [`Beats`]: exact for whole parts below 2²⁴ (hours of music), and
/// to about 10⁻⁷ of a beat in the fraction.
pub fn beats(b: f64) -> Beats {
    let whole = b.floor();
    [whole as f32, (b - whole) as f32]
}

/// One rectangle: horizontal extent in timeline units (beats), a row index
/// (track or pitch row) and a colour (`0xAABBGGRR`, as from RGBA bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RectInstance {
    pub start: Beats,
    pub end: Beats,
    pub row: f32,
    pub color: u32,
}

impl RectInstance {
    pub fn new(start: f64, end: f64, row: f32, color: u32) -> Self {
        Self {
            start: beats(start),
            end: beats(end),
            row,
            color,
        }
    }
}

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

/// Where a layer is drawn, in physical pixels of the host's canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The scroll and zoom of a layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Timeline units at the area's left edge (f64: see [`Beats`]).
    pub left: f64,
    pub px_per_unit: f32,
    pub top_px: f32,
    pub row_height: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    canvas: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    left: f32,
    px_per_unit: f32,
    top_px: f32,
    row_height: f32,
    left_frac: f32,
    _pad: f32,
}

const RECT_SHADER: &str = shaders::RECTS;

const BLIT_SHADER: &str = shaders::BLIT;

struct RectLayer {
    instances: wgpu::Buffer,
    count: u32,
}

struct Prepared {
    draws: Vec<(usize, wgpu::BindGroup)>,
    boxes: Vec<(usize, wgpu::BindGroup)>,
    waves: Vec<(usize, wgpu::BindGroup)>,
    vector: Option<(wgpu::Texture, wgpu::BindGroup)>,
}

/// Opaque handle to a rectangle layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerId(usize);

pub struct Renderer {
    rect_pipeline: wgpu::RenderPipeline,
    rect_bind_layout: wgpu::BindGroupLayout,
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vello: vello::Renderer,
    layers: Vec<RectLayer>,
    boxes: boxes::BoxPipeline,
    wave_pipeline: waves::WavePipeline,
    box_layers: Vec<boxes::BoxLayer>,
    /// One prepared frame per drawing target (a host may draw several areas).
    targets: Vec<Option<Prepared>>,
}

fn pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &str,
    layout: &wgpu::BindGroupLayout,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Renderer {
    /// Creates pipelines for a host whose render target has `format`.
    ///
    /// `pipeline_cache` (Vulkan only) lets Vello reuse compiled shaders from a
    /// previous run, which cuts start-up time.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        pipeline_cache: Option<wgpu::PipelineCache>,
    ) -> Result<Self, String> {
        let rect_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen layer view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let rect_pipeline = pipeline(
            device,
            "lumen rects",
            RECT_SHADER,
            &rect_bind_layout,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<RectInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32, 3 => Uint32],
            })],
            wgpu::PrimitiveTopology::TriangleStrip,
            format,
        );
        let blit_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen vector blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let blit_pipeline = pipeline(
            device,
            "lumen vector blit",
            BLIT_SHADER,
            &blit_bind_layout,
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            format,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let options = vello::RendererOptions {
            pipeline_cache,
            ..Default::default()
        };
        let vello = vello::Renderer::new(device, options).map_err(|e| format!("vello: {e}"))?;
        let boxes = boxes::BoxPipeline::new(device, format);
        let wave_pipeline = waves::WavePipeline::new(device, format);
        Ok(Self {
            rect_pipeline,
            rect_bind_layout,
            blit_pipeline,
            blit_bind_layout,
            sampler,
            vello,
            layers: Vec::new(),
            boxes,
            wave_pipeline,
            box_layers: Vec::new(),
            targets: Vec::new(),
        })
    }

    /// Uploads a layer once. Call [`Renderer::update_layer`] after edits.
    pub fn add_layer(&mut self, device: &wgpu::Device, instances: &[RectInstance]) -> LayerId {
        use wgpu::util::DeviceExt;
        let instances_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lumen layer"),
            contents: bytemuck::cast_slice(instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        self.layers.push(RectLayer {
            instances: instances_buffer,
            count: instances.len() as u32,
        });
        LayerId(self.layers.len() - 1)
    }

    /// Replaces a layer's contents (an edit). Reallocates only if it grew.
    pub fn update_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: LayerId,
        instances: &[RectInstance],
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(instances);
        let layer = &mut self.layers[id.0];
        if bytes.len() as u64 > layer.instances.size() {
            *layer = RectLayer {
                instances: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lumen layer"),
                    size: bytes.len() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                count: 0,
            };
        }
        queue.write_buffer(&layer.instances, 0, bytes);
        layer.count = instances.len() as u32;
    }

    /// Prepares drawing target `target` of size `canvas` (physical pixels):
    /// writes view uniforms and rasterises `vector` with Vello.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: usize,
        canvas: [u32; 2],
        layers: &[(LayerId, Area, View)],
        vector: Option<&vello::Scene>,
    ) -> Result<(), String> {
        use wgpu::util::DeviceExt;
        let draws = layers
            .iter()
            .map(|&(id, area, view)| {
                let uniform = LayerUniform {
                    canvas: [canvas[0] as f32, canvas[1] as f32],
                    origin: [area.x, area.y],
                    size: [area.width, area.height],
                    left: beats(view.left)[0],
                    px_per_unit: view.px_per_unit,
                    top_px: view.top_px,
                    row_height: view.row_height,
                    left_frac: beats(view.left)[1],
                    _pad: 0.0,
                };
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("lumen layer view"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lumen layer view"),
                    layout: &self.rect_bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });
                (id.0, bind)
            })
            .collect();
        let vector = match vector {
            Some(scene) if canvas[0] > 0 && canvas[1] > 0 => {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("lumen vector"),
                    size: wgpu::Extent3d {
                        width: canvas[0],
                        height: canvas[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.vello
                    .render_to_texture(
                        device,
                        queue,
                        scene,
                        &view,
                        &vello::RenderParams {
                            base_color: vello::peniko::Color::TRANSPARENT,
                            width: canvas[0],
                            height: canvas[1],
                            antialiasing_method: vello::AaConfig::Area,
                        },
                    )
                    .map_err(|e| format!("vello render: {e}"))?;
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lumen vector blit"),
                    layout: &self.blit_bind_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                Some((texture, bind))
            }
            _ => None,
        };
        self.set_target(
            target,
            Prepared {
                draws,
                boxes: Vec::new(),
                waves: Vec::new(),
                vector,
            },
        );
        Ok(())
    }

    fn set_target(&mut self, target: usize, prepared: Prepared) {
        if self.targets.len() <= target {
            self.targets.resize_with(target + 1, || None);
        }
        self.targets[target] = Some(prepared);
    }

    /// Draws a prepared target into the host's render pass. The pass's
    /// viewport must cover the canvas given to [`Renderer::prepare`].
    pub fn paint(&self, target: usize, pass: &mut wgpu::RenderPass<'_>) {
        let Some(Some(prepared)) = self.targets.get(target) else {
            return;
        };
        pass.set_pipeline(&self.rect_pipeline);
        for (layer, bind) in &prepared.draws {
            let layer = &self.layers[*layer];
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, layer.instances.slice(..));
            pass.draw(0..4, 0..layer.count);
        }
        self.paint_boxes(prepared, pass);
        if let Some((_, bind)) = &prepared.vector {
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Whether a pipeline cache was available and already populated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    Unsupported,
    Cold,
    Warm,
}

/// Opens the shader pipeline cache stored at `path` (Vulkan only; the device
/// must have been created with [`wgpu::Features::PIPELINE_CACHE`]).
pub fn open_pipeline_cache(
    device: &wgpu::Device,
    path: &std::path::Path,
) -> (Option<wgpu::PipelineCache>, CacheState) {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return (None, CacheState::Unsupported);
    }
    let data = std::fs::read(path).ok();
    let state = if data.is_some() {
        CacheState::Warm
    } else {
        CacheState::Cold
    };
    // SAFETY: the data was written by `save_pipeline_cache`; with `fallback`
    // the driver ignores data that does not match this adapter or driver.
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("lumen pipeline cache"),
            data: data.as_deref(),
            fallback: true,
        })
    };
    (Some(cache), state)
}

/// Writes the cache back so the next start-up can reuse compiled shaders.
pub fn save_pipeline_cache(cache: Option<&wgpu::PipelineCache>, path: &std::path::Path) {
    if let Some(bytes) = cache.and_then(wgpu::PipelineCache::get_data) {
        let _ = std::fs::write(path, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_layouts_have_the_sizes_the_shaders_expect() {
        assert_eq!(size_of::<RectInstance>(), 24);
        assert_eq!(size_of::<LayerUniform>(), 48);
    }

    #[test]
    fn beats_split_exactly_and_keep_the_fraction_fine() {
        assert_eq!(beats(12.25), [12.0, 0.25]);
        assert_eq!(beats(-0.5), [-1.0, 0.5]);
        // An hour at 120 BPM, plus a fiftieth of a sample at 48 kHz: one
        // f32 would round it away; the split keeps it.
        let b = 7_200.0 + 1.0 / 24_000.0 / 50.0;
        let [whole, frac] = beats(b);
        assert_eq!(whole, 7_200.0);
        assert!((f64::from(frac) - (b - 7_200.0)).abs() < 1e-9);
    }

    #[test]
    fn colours_pack_as_rgba_bytes() {
        assert_eq!(rgba(1, 2, 3, 4).to_le_bytes(), [1, 2, 3, 4]);
    }
}
