struct Layer { canvas: vec2<f32>, origin: vec2<f32>, size: vec2<f32>, left: f32, ppu: f32, top: f32, row_h: f32, left_frac: f32, pad: f32 };
@group(0) @binding(0) var<uniform> l: Layer;
struct Rect { @location(0) start: vec2<f32>, @location(1) end: vec2<f32>, @location(2) row: f32, @location(3) color: u32 };

// Beats arrive split into whole and fractional parts, and so does the view's
// left edge: subtracting the parts separately stays exact deep into a long
// song at sample-level zoom, where one f32 subtraction would be pixels off.
fn from_left(b: vec2<f32>) -> f32 {
    return (b.x - l.left) + (b.y - l.left_frac);
}

struct Out { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };

@vertex
fn vs_main(@builtin(vertex_index) v: u32, r: Rect) -> Out {
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    let x0 = l.origin.x + from_left(r.start) * l.ppu;
    let x1 = max(l.origin.x + from_left(r.end) * l.ppu, x0 + 1.0);
    let y0 = l.origin.y + r.row * l.row_h - l.top + 1.0;
    let y1 = y0 + max(l.row_h - 2.0, 1.0);
    // Clamping to the area replaces scissoring, so hosts need not set one.
    let lo = l.origin;
    let hi = l.origin + l.size;
    let p = clamp(vec2<f32>(mix(x0, x1, corner.x), mix(y0, y1, corner.y)), lo, hi);
    var out: Out;
    out.pos = vec4<f32>(p.x / l.canvas.x * 2.0 - 1.0, 1.0 - p.y / l.canvas.y * 2.0, 0.0, 1.0);
    out.color = unpack4x8unorm(r.color);
    return out;
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color.rgb * i.color.a, i.color.a);
}
