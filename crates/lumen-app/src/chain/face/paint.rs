//! Drawing helpers the faces share: a feathered line and a soft disc, each
//! added to one [`Mesh`] so a picture of many lines is one shape and one
//! allocation, never a `Vec` of points a line.

use eframe::egui::{Color32, Mesh, Painter, Pos2, Shape, Stroke, pos2, vec2};

/// How far the soft edge of a line reaches past its width, in points: what
/// anti-aliases a line that is a strip of triangles.
const FEATHER: f32 = 0.6;
/// The sides of a disc, and the rings a soft one has between its middle and
/// its edge.
const SIDES: usize = 28;

/// A mesh with room for `lines` lines of `points` points each.
pub(super) fn mesh_for(lines: usize, points: usize) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(lines * points * 4);
    mesh.indices.reserve(lines * points.saturating_sub(1) * 18);
    mesh
}

/// A line of `n` points, `at(i)` the `i`th, `width` wide in `colour`, with a
/// soft edge. The normals are taken from each point's neighbours, so a smooth
/// curve stays smooth.
pub(super) fn line(
    mesh: &mut Mesh,
    n: usize,
    at: impl Fn(usize) -> Pos2,
    width: f32,
    colour: Color32,
) {
    if n < 2 {
        return;
    }
    let base = mesh.vertices.len() as u32;
    let core = width / 2.0;
    let edge = core + FEATHER;
    for i in 0..n {
        let (before, here, after) = (at(i.saturating_sub(1)), at(i), at((i + 1).min(n - 1)));
        let along = after - before;
        let length = along.length();
        let along = if length > 1e-6 {
            along / length
        } else {
            vec2(1.0, 0.0)
        };
        let across = vec2(-along.y, along.x);
        mesh.colored_vertex(here - across * edge, Color32::TRANSPARENT);
        mesh.colored_vertex(here - across * core, colour);
        mesh.colored_vertex(here + across * core, colour);
        mesh.colored_vertex(here + across * edge, Color32::TRANSPARENT);
    }
    for i in 0..n as u32 - 1 {
        for part in 0..3 {
            let a = base + 4 * i + part;
            let b = a + 4;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }
}

/// A disc of `radius` round `centre` whose colour at each point is
/// `colour(point)`: a soft glow (the colour fading to nothing at the edge) or a
/// lit ball (a highlight off its middle). Added to `mesh` as a fan of rings.
pub(super) fn disc(
    mesh: &mut Mesh,
    centre: Pos2,
    radius: f32,
    rings: usize,
    colour: impl Fn(Pos2) -> Color32,
) {
    if radius <= 0.0 {
        return;
    }
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(centre, colour(centre));
    for ring in 1..=rings {
        let out = radius * ring as f32 / rings as f32;
        for side in 0..SIDES {
            let angle = std::f32::consts::TAU * side as f32 / SIDES as f32;
            let at = pos2(centre.x + out * angle.cos(), centre.y + out * angle.sin());
            mesh.colored_vertex(at, colour(at));
        }
    }
    let ring_at = |ring: usize, side: usize| base + 1 + (ring * SIDES + side % SIDES) as u32;
    for side in 0..SIDES {
        mesh.add_triangle(base, ring_at(0, side), ring_at(0, side + 1));
    }
    for ring in 0..rings.saturating_sub(1) {
        for side in 0..SIDES {
            mesh.add_triangle(
                ring_at(ring, side),
                ring_at(ring + 1, side),
                ring_at(ring + 1, side + 1),
            );
            mesh.add_triangle(
                ring_at(ring, side),
                ring_at(ring + 1, side + 1),
                ring_at(ring, side + 1),
            );
        }
    }
}

/// A soft glow: `colour` at the middle falling to nothing at `radius`, along a
/// smooth bell.
pub(super) fn glow(painter: &Painter, centre: Pos2, radius: f32, colour: Color32) {
    let mut mesh = Mesh::default();
    disc(&mut mesh, centre, radius, 4, |at| {
        let t = ((at - centre).length() / radius).clamp(0.0, 1.0);
        // A bell: 1 at the middle, 0 at the edge, soft all the way.
        let fall = (1.0 - t * t).powi(2);
        colour.gamma_multiply(fall)
    });
    painter.add(Shape::mesh(mesh));
}

/// An ellipse's dashes: `dash` long and `gap` apart along its perimeter, drawn
/// as chords, so the dashes are even all the way round rather than long where
/// the curve is flat. No allocation: the walk keeps its table on the stack.
pub(super) fn dashed_ellipse(
    painter: &Painter,
    centre: Pos2,
    radii: eframe::egui::Vec2,
    stroke: Stroke,
    (dash, gap): (f32, f32),
) {
    const STEPS: usize = 96;
    let point = |theta: f32| {
        pos2(
            centre.x + radii.x * theta.cos(),
            centre.y + radii.y * theta.sin(),
        )
    };
    let mut along = [0.0_f32; STEPS + 1];
    let mut last = point(0.0);
    for (step, slot) in along.iter_mut().enumerate().skip(1) {
        let here = point(std::f32::consts::TAU * step as f32 / STEPS as f32);
        *slot = (here - last).length();
        last = here;
    }
    for step in 1..=STEPS {
        along[step] += along[step - 1];
    }
    let perimeter = along[STEPS];
    if perimeter < dash {
        return;
    }
    // The angle a distance `s` along the perimeter stands at.
    let theta_at = |s: f32| {
        let s = s.clamp(0.0, perimeter);
        let at = along.partition_point(|&reach| reach < s).clamp(1, STEPS);
        let (from, to) = (along[at - 1], along[at]);
        let part = if to > from {
            (s - from) / (to - from)
        } else {
            0.0
        };
        std::f32::consts::TAU * (at as f32 - 1.0 + part) / STEPS as f32
    };
    // Whole dashes round the ring, spread evenly.
    let count = ((perimeter / (dash + gap)).round() as usize).max(1);
    let pitch = perimeter / count as f32;
    for k in 0..count {
        let s = k as f32 * pitch;
        let (a, b) = (point(theta_at(s)), point(theta_at(s + dash)));
        painter.line_segment([a, b], stroke);
    }
}
