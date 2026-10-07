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
