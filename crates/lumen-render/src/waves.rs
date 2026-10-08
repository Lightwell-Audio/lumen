//! Waveforms (M3 slice 4c, ADR-0027), drawn inside the clips of a box
//! layer and sharing its row table.
//!
//! Peaks (minimum and maximum per bucket of frames, at several
//! resolutions) live in one atlas texture; a small table says where each
//! slot's levels start. A wave is a time range on a row that shows a slot's
//! audio from a given frame. For every pixel the fragment shader turns the
//! pixel's width into a range of frames, picks the finest level whose
//! buckets are no wider than that, and takes the extent of the few buckets
//! underneath: crisp at every zoom, with no work on the CPU per frame.

use bytemuck::{Pod, Zeroable};
use vello::wgpu;

use crate::pipeline;

/// Frames per bucket at the finest level of the peaks this draws.
pub const PEAK_BASE: u32 = 16;
/// Buckets of the level below that one bucket covers.
pub const PEAK_FACTOR: u32 = 4;
/// Most levels a slot can have.
pub const MAX_LEVELS: usize = 8;

/// Atlas texels per row; the atlas is a linear run of texels.
const ATLAS_WIDE: u32 = 4096;
/// The atlas never grows past this many rows (4096 × 8192 texels, 128 MB).
const ATLAS_MAX_ROWS: u32 = 8192;

/// One wave: the audio of `slot` from frame `first_frame` (at
/// `frames_per_beat`), shown from beat `start` to `end` on row slot `row`.
/// A clip that spans tempo changes is one wave per constant-tempo segment.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct WaveInstance {
    pub start: f32,
    pub end: f32,
    pub row: u32,
    pub slot: u32,
    pub first_frame: f32,
    pub frames_per_beat: f32,
    /// Straight alpha, `0xAABBGGRR`.
    pub color: u32,
    /// 1 or 2: one waveform, or left over right when there is room.
    pub channels: u32,
}

/// Where waves sit inside their row, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveStyle {
    /// From the row's top to the wave area (inset, title bar, a gap).
    pub top: f32,
    /// From the row's bottom to the wave area.
    pub bottom: f32,
    /// In from the clip's left and right edges (the border).
    pub side: f32,
    /// Below this height, stereo is drawn as one waveform.
    pub split_below: f32,
}

pub(crate) struct Waves {
    instances: wgpu::Buffer,
    count: u32,
    atlas: wgpu::Texture,
    atlas_rows: u32,
    /// Texels in use (a bump allocator; replaced peaks are not reclaimed).
    used: u32,
    table: wgpu::Texture,
    /// Per slot and level: first texel and bucket count. Kept here so the
    /// table can be rewritten when it grows.
    entries: Vec<[[u32; 2]; MAX_LEVELS]>,
}

// Matches the shader's uniform block, or the build fails.
const _: () = assert!(size_of::<WaveUniform>() == crate::shaders::WAVES_UNIFORM);

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WaveUniform {
    canvas: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    left: f32,
    px_per_unit: f32,
    top_px: f32,
    wave_top: f32,
    wave_bottom: f32,
    side: f32,
    split_below: f32,
    _pad: [f32; 3],
}

pub(crate) const WAVE_SHADER: &str = crate::shaders::WAVES;

pub(crate) struct WavePipeline {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) bind_layout: wgpu::BindGroupLayout,
}

impl WavePipeline {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen waves"),
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
                texture(1, wgpu::TextureSampleType::Float { filterable: false }),
                texture(2, wgpu::TextureSampleType::Float { filterable: false }),
                texture(3, wgpu::TextureSampleType::Uint),
            ],
        });
        let pipeline = pipeline(
            device,
            "lumen waves",
            WAVE_SHADER,
            &bind_layout,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<WaveInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32, 1 => Float32, 2 => Uint32, 3 => Uint32,
                    4 => Float32, 5 => Float32, 6 => Uint32, 7 => Uint32
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

fn atlas_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lumen peaks"),
        size: wgpu::Extent3d {
            width: ATLAS_WIDE,
            height: rows,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Snorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn table_texture(device: &wgpu::Device, slots: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lumen peak table"),
        size: wgpu::Extent3d {
            width: MAX_LEVELS as u32,
            height: slots.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg32Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// A rectangle of the atlas: `x, y, width, height`, in texels.
type Rect = [u32; 4];

/// The rectangles that hold `texels` texels from linear texel `start`: the
/// rest of the first row, whole rows, then the start of the last row.
fn spans(start: u32, texels: u32) -> Vec<Rect> {
    let mut out = Vec::new();
    let (mut at, mut left) = (start, texels);
    while left > 0 {
        let (x, y) = (at % ATLAS_WIDE, at / ATLAS_WIDE);
        let (width, height) = if x == 0 && left >= ATLAS_WIDE {
            (ATLAS_WIDE, left / ATLAS_WIDE)
        } else {
            ((ATLAS_WIDE - x).min(left), 1)
        };
        out.push([x, y, width, height]);
        at += width * height;
        left -= width * height;
    }
    out
}

/// Writes `bytes` (whole texels) at linear texel `start` of an atlas.
fn write_span(queue: &wgpu::Queue, atlas: &wgpu::Texture, start: u32, bytes: &[u8]) {
    const TEXEL: u32 = 4;
    let mut rest = bytes;
    for [x, y, width, height] in spans(start, bytes.len() as u32 / TEXEL) {
        let n = (width * height * TEXEL) as usize;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &rest[..n],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * TEXEL),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        rest = &rest[n..];
    }
}

impl Waves {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            instances: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lumen waves"),
                size: size_of::<WaveInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            count: 0,
            atlas: atlas_texture(device, 64),
            atlas_rows: 64,
            used: 0,
            table: table_texture(device, 64),
            entries: Vec::new(),
        }
    }

    pub(crate) fn count(&self) -> u32 {
        self.count
    }

    pub(crate) fn instances(&self) -> &wgpu::Buffer {
        &self.instances
    }

    pub(crate) fn set_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        waves: &[WaveInstance],
    ) {
        let bytes: &[u8] = bytemuck::cast_slice(waves);
        if bytes.len() as u64 > self.instances.size() {
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lumen waves"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
        self.count = waves.len() as u32;
    }

    /// Stores `levels` (finest first) as slot `slot`. False if the atlas
    /// is full; that slot then draws nothing.
    pub(crate) fn set_peaks(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: u32,
        levels: &[&[[i8; 4]]],
    ) -> bool {
        let total: u32 = levels.iter().map(|l| l.len() as u32).sum();
        let needed = (self.used + total).div_ceil(ATLAS_WIDE);
        if needed > ATLAS_MAX_ROWS {
            return false;
        }
        if needed > self.atlas_rows {
            // Grow, keeping what is there. The copy is submitted at once:
            // the queue writes below run before any command buffer of the
            // next submission, so a copy recorded in the frame's encoder
            // would land after them and overwrite the new peaks with the old
            // atlas's empty rows (owner, MT-APP-11: the waveform vanished at
            // the finest zoom near the start of a long file).
            let rows = needed.next_power_of_two().min(ATLAS_MAX_ROWS);
            let bigger = atlas_texture(device, rows);
            let mut copy = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumen peaks grow"),
            });
            copy.copy_texture_to_texture(
                self.atlas.as_image_copy(),
                bigger.as_image_copy(),
                wgpu::Extent3d {
                    width: ATLAS_WIDE,
                    height: self.atlas_rows,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([copy.finish()]);
            self.atlas = bigger;
            self.atlas_rows = rows;
        }
        let mut entry = [[0_u32; 2]; MAX_LEVELS];
        for (n, level) in levels.iter().take(MAX_LEVELS).enumerate() {
            entry[n] = [self.used, level.len() as u32];
            write_span(queue, &self.atlas, self.used, bytemuck::cast_slice(level));
            self.used += level.len() as u32;
        }
        let slot = slot as usize;
        if self.entries.len() <= slot {
            self.entries.resize(slot + 1, [[0; 2]; MAX_LEVELS]);
        }
        self.entries[slot] = entry;
        if self.entries.len() as u32 > self.table.height() {
            self.table = table_texture(device, (self.entries.len() as u32).next_power_of_two());
            for row in 0..self.entries.len() {
                self.write_entry(queue, row);
            }
        } else {
            self.write_entry(queue, slot);
        }
        true
    }

    fn write_entry(&self, queue: &wgpu::Queue, slot: usize) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.table,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: slot as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&self.entries[slot]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(MAX_LEVELS as u32 * 8),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: MAX_LEVELS as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn bind(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        rows: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let atlas = self
            .atlas
            .create_view(&wgpu::TextureViewDescriptor::default());
        let table = self
            .table
            .create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumen waves"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(rows),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&table),
                },
            ],
        })
    }
}

/// The uniform for drawing waves with `style` in an area.
pub(crate) fn uniform(
    canvas: [u32; 2],
    area: crate::Area,
    view: crate::View,
    style: WaveStyle,
) -> WaveUniform {
    WaveUniform {
        canvas: [canvas[0] as f32, canvas[1] as f32],
        origin: [area.x, area.y],
        size: [area.width, area.height],
        left: view.left,
        px_per_unit: view.px_per_unit,
        top_px: view.top_px,
        wave_top: style.top,
        wave_bottom: style.bottom,
        side: style.side,
        split_below: style.split_below,
        _pad: [0.0; 3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GPU device, or `None` where there is none (some CI machines).
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// The atlas's first `texels` texels, read back from the GPU.
    fn read_atlas(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        waves: &Waves,
        texels: u32,
    ) -> Vec<[i8; 4]> {
        let rows = texels.div_ceil(ATLAS_WIDE);
        let row_bytes = ATLAS_WIDE * 4;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(rows * row_bytes),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            waves.atlas.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(rows),
                },
            },
            wgpu::Extent3d {
                width: ATLAS_WIDE,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .unwrap();
        let data = buffer.slice(..).get_mapped_range().expect("mapped");
        bytemuck::cast_slice::<u8, [i8; 4]>(&data)[..texels as usize].to_vec()
    }

    #[test]
    fn peaks_survive_the_atlas_growing() {
        // Owner, MT-APP-11: a long file's waveform vanished at the finest
        // zoom. Its peaks were written, then the atlas grew and the copy of
        // the old, smaller atlas landed over them.
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU here: skipped");
            return;
        };
        let mut waves = Waves::new(&device);
        let first: Vec<[i8; 4]> = (0..1_000).map(|i| [(i % 100) as i8, 1, 2, 3]).collect();
        // Far more than the 64 rows the atlas starts with.
        let second: Vec<[i8; 4]> = (0..ATLAS_WIDE * 100)
            .map(|i| [-((i % 90) as i8), 5, 6, 7])
            .collect();
        assert!(waves.set_peaks(&device, &queue, 0, &[&first]));
        assert!(waves.set_peaks(&device, &queue, 1, &[&second]));
        assert!(waves.atlas_rows > 64, "the atlas grew");
        let read = read_atlas(&device, &queue, &waves, 1_000 + ATLAS_WIDE * 100);
        assert_eq!(read[..1_000], first[..], "the first peaks were kept");
        assert_eq!(read[1_000..], second[..], "the second peaks are whole");
    }

    #[test]
    fn a_run_of_texels_splits_into_whole_rows_and_partial_ends() {
        const W: u32 = ATLAS_WIDE;
        // Within one row.
        assert_eq!(spans(10, 5), [[10, 0, 5, 1]]);
        // From mid-row, across two whole rows, into a fourth.
        assert_eq!(
            spans(W - 3, 3 + 2 * W + 7),
            [[W - 3, 0, 3, 1], [0, 1, W, 2], [0, 3, 7, 1]]
        );
        // Exactly whole rows.
        assert_eq!(spans(2 * W, W), [[0, 2, W, 1]]);
        assert!(spans(5, 0).is_empty());
        // Every texel once, in order.
        let total: u32 = spans(123, 9 * W + 77).iter().map(|r| r[2] * r[3]).sum();
        assert_eq!(total, 9 * W + 77);
    }

    #[test]
    fn gpu_layouts_have_the_sizes_the_shader_expects() {
        assert_eq!(size_of::<WaveInstance>(), 32);
        assert_eq!(size_of::<WaveUniform>(), 64);
    }
}
