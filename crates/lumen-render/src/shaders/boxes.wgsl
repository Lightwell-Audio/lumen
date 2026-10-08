struct Layer {
    canvas: vec2<f32>, origin: vec2<f32>, size: vec2<f32>,
    left: f32, ppu: f32, top: f32,
    inset: f32, radius: f32, title: f32, border: f32, min_width: f32,
    left_frac: f32, pad: f32,
};
@group(0) @binding(0) var<uniform> l: Layer;
@group(0) @binding(1) var rows: texture_2d<f32>;

// Beats arrive split into whole and fractional parts, and so does the view's
// left edge: subtracting the parts separately stays exact deep into a long
// song at sample-level zoom, where one f32 subtraction would be pixels off.
fn from_left(b: vec2<f32>) -> f32 {
    return (b.x - l.left) + (b.y - l.left_frac);
}

struct Box {
    @location(0) start: vec2<f32>, @location(1) end: vec2<f32>, @location(2) row: u32,
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
    let x0 = l.origin.x + from_left(b.start) * l.ppu;
    let x1 = max(l.origin.x + from_left(b.end) * l.ppu, x0 + l.min_width);
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
