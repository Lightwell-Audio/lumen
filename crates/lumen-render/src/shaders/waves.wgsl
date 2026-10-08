struct Layer {
    canvas: vec2<f32>, origin: vec2<f32>, size: vec2<f32>,
    left: f32, ppu: f32, top: f32,
    wave_top: f32, wave_bottom: f32, side: f32, split_below: f32,
    left_frac: f32,
    // 1 while any wave is drawn sample by sample; otherwise the strip
    // table is not read at all.
    strips_on: f32,
    // A float, not a vec, which would change the alignment.
    pad1: f32,
};
@group(0) @binding(0) var<uniform> l: Layer;
@group(0) @binding(1) var rows: texture_2d<f32>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var table: texture_2d<u32>;
// Deep zoom: samples (left, right) and, per wave instance, where its strip is:
// first texel, count, viewport x of the first sample, pixels per sample.
@group(0) @binding(4) var strips: texture_2d<i32>;
@group(0) @binding(5) var strip_table: texture_2d<u32>;

// Beats arrive split into whole and fractional parts, and so does the view's
// left edge: subtracting the parts separately stays exact deep into a long
// song at sample-level zoom, where one f32 subtraction would be pixels off.
fn from_left(b: vec2<f32>) -> f32 {
    return (b.x - l.left) + (b.y - l.left_frac);
}

const BASE: f32 = 16.0;
const LEVELS: i32 = 8;
const ATLAS_WIDE: u32 = 4096u;

struct Wave {
    @location(0) start: vec2<f32>, @location(1) end: vec2<f32>, @location(2) row: u32,
    @location(3) slot: u32, @location(4) first_frame: f32,
    @location(5) frames_per_beat: f32, @location(6) color: u32,
    @location(7) channels: u32,
    // The fade curves: (x1, y1, x2, y2) of a cubic Bézier from (0, 0) to (1, 1).
    @location(8) gain: f32, @location(9) curve_in: vec4<f32>, @location(10) curve_out: vec4<f32>,
    // Fade in and fade out: (from, to) as split beats.
    @location(11) fade_in: vec4<f32>, @location(12) fade_out: vec4<f32>,
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
    @location(6) @interpolate(flat) index: u32,
    // Where the fades run, in viewport pixels: in (from, to), out (from, to).
    @location(7) @interpolate(flat) fades: vec4<f32>,
    @location(8) @interpolate(flat) gain: f32,
    @location(9) @interpolate(flat) curve_in: vec4<f32>,
    @location(10) @interpolate(flat) curve_out: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) v: u32, @builtin(instance_index) index: u32, w: Wave) -> Out {
    var out: Out;
    let slot = textureLoad(rows, vec2<i32>(i32(w.row % 256u), i32(w.row / 256u)), 0);
    // Everything on whole pixels, as the clip box is: while scrolling, the
    // waveform moves by whole pixels with its clip instead of shimmering
    // as the pixel grid slides across its buckets.
    let y0 = round(l.origin.y + slot.x - l.top + l.wave_top);
    let y1 = round(l.origin.y + slot.x - l.top + slot.y - l.wave_bottom);
    let edge = round(l.origin.x + from_left(w.start) * l.ppu);
    let x0 = edge + l.side;
    let x1 = round(l.origin.x + from_left(w.end) * l.ppu) - l.side;
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
    out.wave = vec4<f32>(0.0, w.first_frame, w.frames_per_beat, edge);
    out.slot = w.slot;
    out.channels = w.channels;
    out.index = index;
    out.fades = vec4<f32>(
        x_at(w.fade_in.xy), x_at(w.fade_in.zw), x_at(w.fade_out.xy), x_at(w.fade_out.zw),
    );
    out.gain = w.gain;
    out.curve_in = w.curve_in;
    out.curve_out = w.curve_out;
    return out;
}

// The viewport x of split beats `b`.
fn x_at(b: vec2<f32>) -> f32 {
    return l.origin.x + from_left(b) * l.ppu;
}

// One coordinate of a cubic Bézier from 0 to 1 with inner controls a and b.
fn bezier(a: f32, b: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    return 3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t;
}

// A fade's level `x` of the way from silence to full, as `Curve::at` gives
// it: find where the curve crosses x (it rises with t), then its height.
fn fade_level(c: vec4<f32>, x: f32) -> f32 {
    let want = clamp(x, 0.0, 1.0);
    var lo = 0.0;
    var hi = 1.0;
    // 14 halvings: far finer than a pixel.
    for (var i = 0; i < 14; i++) {
        let mid = 0.5 * (lo + hi);
        if (bezier(c.x, c.z, mid) < want) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    return clamp(bezier(c.y, c.w, 0.5 * (lo + hi)), 0.0, 1.0);
}

// The level the wave is drawn at under pixel x: the clip's gain through its
// fades, so the waveform looks as it sounds.
fn envelope(i: Out) -> f32 {
    var level = i.gain;
    let f = i.fades;
    if (f.y > f.x && i.px.x < f.y) {
        level *= fade_level(i.curve_in, (i.px.x - f.x) / (f.y - f.x));
    }
    if (f.w > f.z && i.px.x > f.z) {
        level *= fade_level(i.curve_out, (f.w - i.px.x) / (f.w - f.z));
    }
    return level;
}

fn texel(i: u32) -> vec4<f32> {
    return textureLoad(atlas, vec2<i32>(i32(i % ATLAS_WIDE), i32(i / ATLAS_WIDE)), 0);
}

fn sample(first: u32, k: u32) -> vec2<f32> {
    let i = first + k;
    let v = textureLoad(strips, vec2<i32>(i32(i % ATLAS_WIDE), i32(i / ATLAS_WIDE)), 0).xy;
    return vec2<f32>(v) / 32767.0;
}

// The line through the samples at fractional sample `s` (clamped to the strip).
fn line_at(first: u32, count: u32, s: f32) -> vec2<f32> {
    let at = clamp(s, 0.0, f32(count - 1u));
    let k = u32(floor(at));
    let next = min(k + 1u, count - 1u);
    return mix(sample(first, k), sample(first, next), at - f32(k));
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
    var lo = vec2<f32>(1.0);
    var hi = vec2<f32>(-1.0);
    var strip = vec4<u32>(0u);
    if (l.strips_on > 0.5) {
        strip = textureLoad(strip_table, vec2<i32>(i32(i.index % 256u), i32(i.index / 256u)), 0);
    }
    if (strip.y > 1u) {
        // Deep zoom: the line through the samples under this pixel (its
        // ends, and every sample between). Positions come from the app in
        // viewport pixels, so they stay exact deep into a long file.
        let first_x = bitcast<f32>(strip.z);
        let per = bitcast<f32>(strip.w);
        let s0 = (i.px.x - 0.5 - first_x) / per;
        let s1 = (i.px.x + 0.5 - first_x) / per;
        if (s1 < 0.0 || s0 > f32(strip.y - 1u)) {
            discard;
        }
        let a = line_at(strip.x, strip.y, s0);
        let b = line_at(strip.x, strip.y, s1);
        lo = min(a, b);
        hi = max(a, b);
        let k0 = u32(max(ceil(s0), 0.0));
        let k1 = u32(clamp(floor(s1), 0.0, f32(strip.y - 1u)));
        // Up to 16 samples under a pixel (sample level starts there).
        for (var k = k0; k <= min(k1, k0 + 16u); k++) {
            let v = sample(strip.x, k);
            lo = min(lo, v);
            hi = max(hi, v);
        }
        let env = envelope(i);
        return shade(i, lo * env, hi * env);
    }
    // The frames under this pixel: columns are counted from the clip's
    // pixel edge, so column k always shows the same frames at this zoom,
    // wherever the view has scrolled to.
    let frames = i.wave.z / l.ppu;
    let column = floor(i.px.x - i.wave.w);
    let frame0 = i.wave.y + column * frames;
    // A level with at least four buckets under the pixel: the buckets that
    // straddle its edges then decide little, so the waveform stays steady
    // while zooming instead of shimmering as they come and go.
    let level = clamp(i32(floor(log2(max(frames / (BASE * 4.0), 1.0)) * 0.5)), 0, LEVELS - 1);
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
    // At most 17 buckets: 4 to 16 under the pixel, and one it straddles.
    for (var b = max(first, 0); b <= min(last, min(first + 17, i32(count) - 1)); b++) {
        let t = texel(entry.x + u32(b));
        lo = min(lo, t.xz);
        hi = max(hi, t.yw);
    }
    let env = envelope(i);
    return shade(i, lo * env, hi * env);
}

// The pixel's colour for a waveform spanning `lo` to `hi` (left, right).
fn shade(i: Out, lo: vec2<f32>, hi: vec2<f32>) -> vec4<f32> {
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
