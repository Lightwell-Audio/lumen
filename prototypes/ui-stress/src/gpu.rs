//! The escape hatch from ADR-0002: dense content drawn by a custom wgpu
//! pipeline inside egui. Notes live on the GPU as instances; a frame only
//! updates a small uniform with the view, so CPU cost no longer grows with
//! the number of notes.

use bytemuck::{Pod, Zeroable};
use eframe::egui::{Color32, Rect};
use eframe::egui_wgpu::{self, wgpu};
use eframe::wgpu::util::DeviceExt;

use crate::data::Note;
use crate::paint::View;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    start: f32,
    end: f32,
    pitch: f32,
    color: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    size: [f32; 2],
    left_beat: f32,
    px_per_beat: f32,
    top_px: f32,
    row_h: f32,
    _pad: [f32; 2],
}

const SHADER: &str = r#"
struct Uniforms { size: vec2<f32>, left_beat: f32, px_per_beat: f32, top_px: f32, row_h: f32, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct Note { @location(0) start: f32, @location(1) end: f32, @location(2) pitch: f32, @location(3) color: u32 };
struct Out { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };

@vertex
fn vs_main(@builtin(vertex_index) v: u32, n: Note) -> Out {
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    let x0 = (n.start - u.left_beat) * u.px_per_beat;
    let x1 = max((n.end - u.left_beat) * u.px_per_beat, x0 + 1.0);
    let y0 = (127.0 - n.pitch) * u.row_h - u.top_px + 0.5;
    let y1 = y0 + max(u.row_h - 1.0, 1.0);
    let p = vec2<f32>(mix(x0, x1, corner.x), mix(y0, y1, corner.y));
    var out: Out;
    out.pos = vec4<f32>(p.x / u.size.x * 2.0 - 1.0, 1.0 - p.y / u.size.y * 2.0, 0.0, 1.0);
    out.color = unpack4x8unorm(n.color);
    return out;
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    return i.color;
}
"#;

struct NotesGpu {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    count: u32,
}

/// Uploads the notes once and registers the pipeline with egui's renderer.
pub fn install(state: &egui_wgpu::RenderState, notes: &[Note]) {
    let device = &state.device;
    let base = Color32::from_rgb(0x5B, 0x9B, 0xE6);
    let instances: Vec<Instance> = notes
        .iter()
        .map(|n| Instance {
            start: n.start,
            end: n.start + n.len,
            pitch: f32::from(n.pitch),
            color: u32::from_le_bytes(
                base.gamma_multiply(0.45 + f32::from(n.velocity) / 127.0 * 0.55)
                    .to_array(),
            ),
        })
        .collect();
    let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("notes"),
        contents: bytemuck::cast_slice(&instances),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("notes view"),
        size: size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("notes view"),
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
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("notes view"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("notes"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("notes"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("notes"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32, 1 => Float32, 2 => Float32, 3 => Uint32],
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: state.target_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    let count = notes.len() as u32;
    state.renderer.write().callback_resources.insert(NotesGpu {
        pipeline,
        bind_group,
        uniforms,
        instances,
        count,
    });
}

struct NotesCallback(Uniforms);

impl egui_wgpu::CallbackTrait for NotesCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<NotesGpu>() {
            queue.write_buffer(&gpu.uniforms, 0, bytemuck::bytes_of(&self.0));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<NotesGpu>() {
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.set_vertex_buffer(0, gpu.instances.slice(..));
            pass.draw(0..4, 0..gpu.count);
        }
    }
}

/// The paint shape for all notes in `rect` under `view`.
pub fn notes_shape(rect: Rect, view: &View) -> eframe::egui::Shape {
    let uniforms = Uniforms {
        size: [rect.width(), rect.height()],
        left_beat: view.left_beat,
        px_per_beat: view.px_per_beat,
        top_px: view.top_px,
        row_h: view.row_h,
        _pad: [0.0; 2],
    };
    eframe::egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(
        rect,
        NotesCallback(uniforms),
    ))
}
