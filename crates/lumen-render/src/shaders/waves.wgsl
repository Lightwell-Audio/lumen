struct Layer {
    canvas: vec2<f32>, origin: vec2<f32>, size: vec2<f32>,
    left: f32, ppu: f32, top: f32,
    wave_top: f32, wave_bottom: f32, side: f32, split_below: f32,
    // Three floats, not a vec3, which would align to 16 bytes.
    pad0: f32, pad1: f32, pad2: f32,
};
@group(0) @binding(0) var<uniform> l: Layer;
@group(0) @binding(1) var rows: texture_2d<f32>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var table: texture_2d<u32>;

const BASE: f32 = 16.0;
const LEVELS: i32 = 8;
const ATLAS_WIDE: u32 = 4096u;

struct Wave {
    @location(0) start: f32, @location(1) end: f32, @location(2) row: u32,
    @location(3) slot: u32, @location(4) first_frame: f32,
    @location(5) frames_per_beat: f32, @location(6) color: u32,
    @location(7) channels: u32,
};
struct Out {
    @builtin(position) pos: vec4<f32>,
    // Viewport pixels, so the fragment knows its own x and y.
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) area: vec4<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) wave: vec4<f32>,
    @location(4) @interpolate(flat) slot: u32,
    @location(5) @interpolate(flat) channels: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) v: u32, w: Wave) -> Out {
    var out: Out;
    let slot = textureLoad(rows, vec2<i32>(i32(w.row % 256u), i32(w.row / 256u)), 0);
    // Everything on whole pixels, as the clip box is: while scrolling, the
    // waveform moves by whole pixels with its clip instead of shimmering
    // as the pixel grid slides across its buckets.
    let y0 = round(l.origin.y + slot.x - l.top + l.wave_top);
    let y1 = round(l.origin.y + slot.x - l.top + slot.y - l.wave_bottom);
    let edge = round(l.origin.x + (w.start - l.left) * l.ppu);
    let x0 = edge + l.side;
    let x1 = round(l.origin.x + (w.end - l.left) * l.ppu) - l.side;
    if (y1 - y0 < 2.0 || x1 <= x0 || slot.z <= 0.0) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        return out;
    }
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    let p = clamp(mix(vec2<f32>(x0, y0), vec2<f32>(x1, y1), corner), l.origin, l.origin + l.size);
    out.pos = vec4<f32>(p.x / l.canvas.x * 2.0 - 1.0, 1.0 - p.y / l.canvas.y * 2.0, 0.0, 1.0);
    out.px = p;
    out.area = vec4<f32>(x0, y0, x1, y1);
    let c = unpack4x8unorm(w.color);
    out.color = vec4<f32>(c.rgb, c.a * slot.z);
    out.wave = vec4<f32>(w.start, w.first_frame, w.frames_per_beat, edge);
    out.slot = w.slot;
    out.channels = w.channels;
    return out;
}

fn texel(i: u32) -> vec4<f32> {
    return textureLoad(atlas, vec2<i32>(i32(i % ATLAS_WIDE), i32(i / ATLAS_WIDE)), 0);
}

// How much of the pixel row [py - 0.5, py + 0.5] lies in [top, bottom],
// with the band at least one pixel tall.
fn cover(py: f32, top: f32, bottom: f32) -> f32 {
    let mid = 0.5 * (top + bottom);
    let half = max(0.5 * (bottom - top), 0.5);
    return clamp(min(py + 0.5, mid + half) - max(py - 0.5, mid - half), 0.0, 1.0);
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    // The frames under this pixel: columns are counted from the clip's
    // pixel edge, so column k always shows the same frames at this zoom,
    // wherever the view has scrolled to.
    let frames = i.wave.z / l.ppu;
    let column = floor(i.px.x - i.wave.w);
    let frame0 = i.wave.y + column * frames;
    // The finest level whose buckets are no wider than the pixel.
    let level = clamp(i32(floor(log2(max(frames / BASE, 1.0)) * 0.5)), 0, LEVELS - 1);
    let entry = textureLoad(table, vec2<i32>(level, i32(i.slot)), 0);
    let count = entry.y;
    if (count == 0u) {
        discard;
    }
    let size = BASE * exp2(2.0 * f32(level));
    let first = i32(floor(frame0 / size));
    let last = i32(floor((frame0 + frames - 0.001) / size));
    if (last < 0 || first >= i32(count)) {
        discard;
    }
    var lo = vec2<f32>(1.0);
    var hi = vec2<f32>(-1.0);
    // At most five buckets: each is at least a quarter of the pixel.
    for (var b = max(first, 0); b <= min(last, min(first + 5, i32(count) - 1)); b++) {
        let t = texel(entry.x + u32(b));
        lo = min(lo, t.xz);
        hi = max(hi, t.yw);
    }
    let y = i.px.y;
    let top = i.area.y;
    let height = i.area.w - i.area.y;
    var coverage: f32;
    if (i.channels == 2u && height >= l.split_below) {
        // Left above right, each in its own half.
        let half = 0.5 * height;
        let lane = select(1.0, 0.0, y < top + half);
        let centre = top + half * (lane + 0.5);
        let amp = 0.5 * half - 0.5;
        let a = select(vec2<f32>(lo.x, hi.x), vec2<f32>(lo.y, hi.y), lane > 0.5);
        coverage = cover(y, centre - a.y * amp, centre - a.x * amp);
    } else {
        let centre = top + 0.5 * height;
        let amp = 0.5 * height - 0.5;
        coverage = cover(y, centre - max(hi.x, hi.y) * amp, centre - min(lo.x, lo.y) * amp);
    }
    let a = i.color.a * coverage;
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(i.color.rgb * a, a);
}
