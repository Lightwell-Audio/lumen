use eframe::egui::{Color32, Mesh, Painter, Pos2, Shape, Stroke, pos2, vec2};

use crate::theme::glow as ladder;

const FEATHER: f32 = 0.6;
const SIDES: usize = 28;

pub(super) fn mesh_for(lines: usize, points: usize) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(lines * points * 4);
    mesh.indices.reserve(lines * points.saturating_sub(1) * 18);
    mesh
}

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

pub(super) fn glow(painter: &Painter, centre: Pos2, radius: f32, colour: Color32) {
    let mut mesh = Mesh::default();
    disc(&mut mesh, centre, radius, 4, |at| {
        let t = ((at - centre).length() / radius).clamp(0.0, 1.0);
        let fall = (1.0 - t * t).powi(2);
        colour.gamma_multiply(fall)
    });
    painter.add(Shape::mesh(mesh));
}

#[allow(dead_code)]
pub(super) fn glow_ladder(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    colour: Color32,
    share: f32,
    additive: bool,
) {
    if radius <= 0.0 || share <= 0.0 {
        return;
    }
    let tint = |alpha: f32| {
        let lit = colour.gamma_multiply(alpha * share.clamp(0.0, 1.0));
        if additive { ladder::additive(lit) } else { lit }
    };
    let sides = ladder::SIDES;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, tint(ladder::ALPHAS[0]));
    for (stop, alpha) in ladder::STOPS.iter().zip(ladder::ALPHAS).skip(1) {
        let out = radius * stop;
        for side in 0..sides {
            let angle = std::f32::consts::TAU * side as f32 / sides as f32;
            mesh.colored_vertex(
                pos2(centre.x + out * angle.cos(), centre.y + out * angle.sin()),
                tint(alpha),
            );
        }
    }
    let ring_at = |ring: usize, side: usize| 1 + (ring * sides + side % sides) as u32;
    for side in 0..sides {
        mesh.add_triangle(0, ring_at(0, side), ring_at(0, side + 1));
    }
    for ring in 0..ladder::STOPS.len() - 2 {
        for side in 0..sides {
            let (a, b) = (ring_at(ring, side), ring_at(ring, side + 1));
            let (c, d) = (ring_at(ring + 1, side), ring_at(ring + 1, side + 1));
            mesh.add_triangle(a, c, d);
            mesh.add_triangle(a, d, b);
        }
    }
    painter.add(Shape::mesh(mesh));
}

#[allow(dead_code)]
pub(super) fn smooth_path(points: &[Pos2], steps: usize) -> Vec<Pos2> {
    let n = points.len();
    if n < 3 || steps == 0 {
        return points.to_vec();
    }
    let mid = |a: Pos2, b: Pos2| pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let mut path = Vec::with_capacity((n - 2) * steps + 2);
    path.push(points[0]);
    for i in 1..n - 1 {
        let (from, control, to) = (
            mid(points[i - 1], points[i]),
            points[i],
            mid(points[i], points[i + 1]),
        );
        path.push(from);
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            let u = 1.0 - t;
            path.push(pos2(
                u * u * from.x + 2.0 * u * t * control.x + t * t * to.x,
                u * u * from.y + 2.0 * u * t * control.y + t * t * to.y,
            ));
        }
    }
    path.push(points[n - 1]);
    path.dedup_by(|a, b| (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4);
    path
}

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
    let count = ((perimeter / (dash + gap)).round() as usize).max(1);
    let pitch = perimeter / count as f32;
    for k in 0..count {
        let s = k as f32 * pitch;
        let (a, b) = (point(theta_at(s)), point(theta_at(s + dash)));
        painter.line_segment([a, b], stroke);
    }
}

#[cfg(test)]
mod tests;
