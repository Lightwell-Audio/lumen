//! Box layers (M3 slice 4b): clip bodies on the GPU. Each box is a time
//! range on a row slot, drawn as a rounded body with a title bar, a border
//! and an optional loading fill, antialiased in the fragment shader.
//!
//! Boxes are uploaded once and replaced only when they change (an edit, a
//! file finishing loading). Where rows sit (their top, height and dimming)
//! is a separate small table, a float texture read in the vertex shader, so
//! folding, resizing and their animations upload a few hundred rows, not
//! every box. A texture rather than a storage buffer, so every backend can
//! read it from the vertex stage.

use bytemuck::{Pod, Zeroable};
use vello::wgpu;

use crate::waves::{self, StripEntry, WaveInstance, WaveStyle, Waves};
use crate::{Area, Beats, Prepared, Renderer, View, beats, pipeline};

/// One box: a time range (timeline units, as [`View::left`]) on row slot
/// `row`, with straight-alpha colours (`0xAABBGGRR`, from [`crate::rgba`]).
/// `row`'s low 24 bits are the slot; its top 8 bits, when set, are how many
/// further slots below it the box also covers (see [`spanning`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BoxInstance {
    pub start: Beats,
    pub end: Beats,
    pub row: u32,
    pub body: u32,
    /// The title bar along the top.
    pub bar: u32,
    /// The border, drawn inside the edge.
    pub border: u32,
    /// Covers the body's left `progress` (0 to 1): a loading file.
    pub fill: u32,
    pub progress: f32,
}

/// A box's `row` for one that starts on slot `first` and covers `extra`
/// further slots below it (at most 255; slots up to 2^24).
pub fn spanning(first: u32, extra: u32) -> u32 {
    debug_assert!(first < 1 << 24);
    (first & 0xFF_FFFF) | (extra.min(255) << 24)
}

/// Where a row slot is: its top and height in content pixels (before the
/// view's vertical scroll), and an opacity for everything on it. A height
/// of 0 hides the row's boxes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RowSlot {
    pub top: f32,
    pub height: f32,
    pub alpha: f32,
    _pad: f32,
}

impl RowSlot {
    pub fn new(top: f32, height: f32, alpha: f32) -> Self {
        Self {
            top,
            height,
            alpha,
            _pad: 0.0,
        }
    }

    pub const HIDDEN: RowSlot = RowSlot {
        top: 0.0,
        height: 0.0,
        alpha: 0.0,
        _pad: 0.0,
    };
}

/// How boxes are drawn, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxStyle {
    /// Space between the row's edges and the box, above and below.
    pub inset: f32,
    pub radius: f32,
    /// Height of the title bar.
    pub title: f32,
    /// Border width.
    pub border: f32,
    /// Boxes are never narrower than this.
    pub min_width: f32,
}

/// Opaque handle to a box layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxLayerId(usize);

/// Row slots per texture row.
const SLOTS_WIDE: u32 = 256;

pub(crate) struct BoxLayer {
    instances: wgpu::Buffer,
    count: u32,
    rows: wgpu::Texture,
    /// Slots the texture holds.
    row_capacity: u32,
    /// Waveforms inside the boxes, on the same rows.
    waves: Waves,
}

/// One box layer to draw: where, at which scroll and zoom, and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxDraw {
    pub layer: BoxLayerId,
    pub area: Area,
    pub view: View,
    pub style: BoxStyle,
    pub waves: WaveStyle,
}

// Matches the shader's uniform block, or the build fails.
const _: () = assert!(size_of::<BoxUniform>() == crate::shaders::BOXES_UNIFORM);

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BoxUniform {
    canvas: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    left: f32,
    px_per_unit: f32,
    top_px: f32,
    inset: f32,
    radius: f32,
    title: f32,
    border: f32,
    min_width: f32,
    left_frac: f32,
    _pad: f32,
}

pub(crate) const BOX_SHADER: &str = crate::shaders::BOXES;

pub(crate) struct BoxPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
}

impl BoxPipeline {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen boxes"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = pipeline(
            device,
            "lumen boxes",
            BOX_SHADER,
            &bind_layout,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<BoxInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2, 1 => Float32x2, 2 => Uint32, 3 => Uint32,
                    4 => Uint32, 5 => Uint32, 6 => Uint32, 7 => Float32
                ],
            })],
            wgpu::PrimitiveTopology::TriangleStrip,
            format,
        );
        Self {
            pipeline,
            bind_layout,
        }
    }
}

fn row_texture(device: &wgpu::Device, slots: u32) -> (wgpu::Texture, u32) {
    let height = slots.div_ceil(SLOTS_WIDE).max(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lumen box rows"),
        size: wgpu::Extent3d {
            width: SLOTS_WIDE,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    (texture, height * SLOTS_WIDE)
}

impl Renderer {
    /// Adds an empty box layer.
    pub fn add_box_layer(&mut self, device: &wgpu::Device) -> BoxLayerId {
        let (rows, row_capacity) = row_texture(device, SLOTS_WIDE);
        self.box_layers.push(BoxLayer {
            instances: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lumen boxes"),
                size: size_of::<BoxInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            count: 0,
            rows,
            row_capacity,
            waves: Waves::new(device),
        });
        BoxLayerId(self.box_layers.len() - 1)
    }

    /// Replaces a layer's boxes. Reallocates only if it grew.
    pub fn update_boxes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: BoxLayerId,
        boxes: &[BoxInstance],
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(boxes);
        let layer = &mut self.box_layers[id.0];
        if bytes.len() as u64 > layer.instances.size() {
            layer.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lumen boxes"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&layer.instances, 0, bytes);
        }
        layer.count = boxes.len() as u32;
    }

    /// Replaces a layer's waveforms. Reallocates only if it grew.
    pub fn update_waves(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: BoxLayerId,
        waves: &[WaveInstance],
    ) {
        self.box_layers[id.0]
            .waves
            .set_instances(device, queue, waves);
    }

    /// Stores waveform peaks as `slot` of a layer: `levels` finest first,
    /// [`waves::PEAK_BASE`] frames per bucket at level 0, each level
    /// [`waves::PEAK_FACTOR`] times coarser. False if the atlas is full.
    pub fn set_peaks(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: BoxLayerId,
        slot: u32,
        levels: &[&[[i8; 4]]],
    ) -> bool {
        self.box_layers[id.0]
            .waves
            .set_peaks(device, queue, slot, levels)
    }

    /// Empties a layer's peak atlas, to store the peaks still drawn again
    /// from its start (M9 slice R6: space held by peaks no longer drawn is
    /// reclaimed).
    pub fn clear_peaks(&mut self, queue: &wgpu::Queue, id: BoxLayerId) {
        self.box_layers[id.0].waves.clear_peaks(queue);
    }

    /// Sample strips for waves drawn sample by sample (deep zoom): see
    /// [`StripEntry`].
    pub fn update_strips(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: BoxLayerId,
        table: &[StripEntry],
        writes: &[(u32, &[[i16; 2]])],
    ) {
        self.box_layers[id.0]
            .waves
            .set_strips(device, queue, table, writes);
    }

    /// Replaces a layer's row slots (every frame, if rows move).
    pub fn update_rows(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: BoxLayerId,
        rows: &[RowSlot],
    ) {
        let layer = &mut self.box_layers[id.0];
        let slots = rows.len() as u32;
        if slots > layer.row_capacity {
            (layer.rows, layer.row_capacity) = row_texture(device, slots.next_power_of_two());
        }
        // Whole texture rows, so the copy's row pitch is the texture's.
        let height = slots.div_ceil(SLOTS_WIDE).max(1);
        let mut data = rows.to_vec();
        data.resize((height * SLOTS_WIDE) as usize, RowSlot::HIDDEN);
        queue.write_texture(
            layer.rows.as_image_copy(),
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SLOTS_WIDE * size_of::<RowSlot>() as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width: SLOTS_WIDE,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Prepares drawing target `target` of size `canvas` (physical pixels)
    /// with box layers.
    pub fn prepare_boxes(
        &mut self,
        device: &wgpu::Device,
        target: usize,
        canvas: [u32; 2],
        layers: &[BoxDraw],
    ) {
        use wgpu::util::DeviceExt;
        let mut waves = Vec::new();
        let boxes = layers
            .iter()
            .map(|draw| {
                let BoxDraw {
                    layer: id,
                    area,
                    view,
                    style,
                    ..
                } = *draw;
                let uniform = BoxUniform {
                    canvas: [canvas[0] as f32, canvas[1] as f32],
                    origin: [area.x, area.y],
                    size: [area.width, area.height],
                    left: beats(view.left)[0],
                    px_per_unit: view.px_per_unit,
                    top_px: view.top_px,
                    inset: style.inset,
                    radius: style.radius,
                    title: style.title,
                    border: style.border,
                    min_width: style.min_width,
                    left_frac: beats(view.left)[1],
                    _pad: 0.0,
                };
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("lumen boxes view"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let layer = &self.box_layers[id.0];
                let rows = layer
                    .rows
                    .create_view(&wgpu::TextureViewDescriptor::default());
                if layer.waves.count() > 0 {
                    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("lumen waves view"),
                        contents: bytemuck::bytes_of(&waves::uniform(
                            canvas,
                            area,
                            view,
                            draw.waves,
                            layer.waves.strips_on,
                        )),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    waves.push((
                        id.0,
                        layer
                            .waves
                            .bind(device, &self.wave_pipeline.bind_layout, &rows, &uniform),
                    ));
                }
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("lumen boxes view"),
                    layout: &self.boxes.bind_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&rows),
                        },
                    ],
                });
                (id.0, bind)
            })
            .collect();
        self.set_target(
            target,
            Prepared {
                draws: Vec::new(),
                boxes,
                waves,
                vector: None,
            },
        );
    }

    pub(crate) fn paint_boxes(&self, prepared: &Prepared, pass: &mut wgpu::RenderPass<'_>) {
        if prepared.boxes.is_empty() {
            return;
        }
        pass.set_pipeline(&self.boxes.pipeline);
        for (layer, bind) in &prepared.boxes {
            let layer = &self.box_layers[*layer];
            if layer.count == 0 {
                continue;
            }
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, layer.instances.slice(..));
            pass.draw(0..4, 0..layer.count);
        }
        if prepared.waves.is_empty() {
            return;
        }
        pass.set_pipeline(&self.wave_pipeline.pipeline);
        for (layer, bind) in &prepared.waves {
            let waves = &self.box_layers[*layer].waves;
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, waves.instances().slice(..));
            pass.draw(0..4, 0..waves.count());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_layouts_have_the_sizes_the_shader_expects() {
        assert_eq!(size_of::<BoxInstance>(), 40);
        assert_eq!(size_of::<RowSlot>(), 16, "one Rgba32Float texel");
        assert_eq!(size_of::<BoxUniform>(), 64);
    }
}
