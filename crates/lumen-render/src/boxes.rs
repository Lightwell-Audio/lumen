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

use crate::{Area, Prepared, Renderer, View, pipeline};

/// One box: a time range (timeline units, as [`View::left`]) on row slot
/// `row`, with straight-alpha colours (`0xAABBGGRR`, from [`crate::rgba`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BoxInstance {
    pub start: f32,
    pub end: f32,
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
}

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
    _pad: [f32; 2],
}

pub(crate) const BOX_SHADER: &str = r#"
struct Layer {
    canvas: vec2<f32>, origin: vec2<f32>, size: vec2<f32>,
    left: f32, ppu: f32, top: f32,
    inset: f32, radius: f32, title: f32, border: f32, min_width: f32,
    pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> l: Layer;
@group(0) @binding(1) var rows: texture_2d<f32>;

struct Box {
    @location(0) start: f32, @location(1) end: f32, @location(2) row: u32,
    @location(3) body: u32, @location(4) bar: u32, @location(5) border: u32,
    @location(6) fill: u32, @location(7) progress: f32,
};
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) body: vec4<f32>,
    @location(3) @interpolate(flat) bar: vec4<f32>,
    @location(4) @interpolate(flat) border: vec4<f32>,
    @location(5) @interpolate(flat) fill: vec4<f32>,
    @location(6) @interpolate(flat) progress: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) v: u32, b: Box) -> Out {
    var out: Out;
    let slot = textureLoad(rows, vec2<i32>(i32(b.row % 256u), i32(b.row / 256u)), 0);
    let height = slot.y - 2.0 * l.inset;
    if (height <= 0.0 || slot.z <= 0.0) {
        // A hidden row: a degenerate triangle, nothing drawn.
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        return out;
    }
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    let x0 = l.origin.x + (b.start - l.left) * l.ppu;
    let x1 = max(l.origin.x + (b.end - l.left) * l.ppu, x0 + l.min_width);
    let y0 = l.origin.y + slot.x - l.top + l.inset;
    // Edges on whole pixels, as egui snaps its rectangles: crisp borders,
    // and the box lines up with text and shapes drawn beside it.
    let lo = round(vec2<f32>(x0, y0));
    let hi = max(round(vec2<f32>(x1, y0 + height)), lo + vec2<f32>(1.0));
    // Clamping to the area replaces scissoring; `local` keeps the box's
    // own coordinates, so a clipped box still draws its shape correctly.
    let p = clamp(mix(lo, hi, corner), l.origin, l.origin + l.size);
    out.pos = vec4<f32>(p.x / l.canvas.x * 2.0 - 1.0, 1.0 - p.y / l.canvas.y * 2.0, 0.0, 1.0);
    out.local = p - lo;
    out.size = hi - lo;
    let a = vec4<f32>(1.0, 1.0, 1.0, slot.z);
    out.body = unpack4x8unorm(b.body) * a;
    out.bar = unpack4x8unorm(b.bar) * a;
    out.border = unpack4x8unorm(b.border) * a;
    out.fill = unpack4x8unorm(b.fill) * a;
    out.progress = b.progress;
    return out;
}

// Signed distance to a rounded rectangle centred on the origin.
fn rounded(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// `top` over `under`, straight alpha.
fn over(top: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    let a = top.a + under.a * (1.0 - top.a);
    if (a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>((top.rgb * top.a + under.rgb * under.a * (1.0 - top.a)) / a, a);
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    let half = i.size * 0.5;
    let r = min(l.radius, min(half.x, half.y));
    let d = rounded(i.local - half, half, r);
    let inside = clamp(0.5 - d, 0.0, 1.0);
    if (inside <= 0.0) {
        discard;
    }
    var c = i.body;
    if (i.local.x < i.progress * i.size.x) {
        c = over(i.fill, c);
    }
    if (i.local.y < min(l.title, i.size.y)) {
        c = over(i.bar, c);
    }
    let ring = inside - clamp(0.5 - (d + l.border), 0.0, 1.0);
    c = over(vec4<f32>(i.border.rgb, i.border.a * ring / max(inside, 1e-4)), c);
    let a = c.a * inside;
    return vec4<f32>(c.rgb * a, a);
}
"#;

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
                    0 => Float32, 1 => Float32, 2 => Uint32, 3 => Uint32,
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
        layers: &[(BoxLayerId, Area, View, BoxStyle)],
    ) {
        use wgpu::util::DeviceExt;
        let boxes = layers
            .iter()
            .map(|&(id, area, view, style)| {
                let uniform = BoxUniform {
                    canvas: [canvas[0] as f32, canvas[1] as f32],
                    origin: [area.x, area.y],
                    size: [area.width, area.height],
                    left: view.left,
                    px_per_unit: view.px_per_unit,
                    top_px: view.top_px,
                    inset: style.inset,
                    radius: style.radius,
                    title: style.title,
                    border: style.border,
                    min_width: style.min_width,
                    _pad: [0.0; 2],
                };
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("lumen boxes view"),
                    contents: bytemuck::bytes_of(&uniform),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let rows = self.box_layers[id.0]
                    .rows
                    .create_view(&wgpu::TextureViewDescriptor::default());
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_layouts_have_the_sizes_the_shader_expects() {
        assert_eq!(size_of::<BoxInstance>(), 32);
        assert_eq!(size_of::<RowSlot>(), 16, "one Rgba32Float texel");
        assert_eq!(size_of::<BoxUniform>(), 64);
    }
}
