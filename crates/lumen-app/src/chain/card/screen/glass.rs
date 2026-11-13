//! Gradients on a rounded panel: the screen's glow and the header's tint.

use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, pos2};

use crate::theme::personality::card as c;

/// The most rows of the grid: four in each corner and a band of the middle
/// every [`BAND`] points (a 176 pt screen needs under 30).
const ROWS: usize = 40;
const COLUMNS: usize = 16;
const CORNER_ROWS: usize = 4;
/// The middle's rows are about this far apart.
const BAND: f32 = 14.0;

/// A rounded panel filled by `colour` at each vertex of a grid (finer in the
/// rounded corners), so a radial or vertical gradient is drawn as it is, not as
/// a fan from the middle. `top` and `bottom` are the corner radii of its top and
/// bottom edges. One mesh, and no allocation but its own.
pub(super) fn glass(
    painter: &Painter,
    rect: Rect,
    top: f32,
    bottom: f32,
    colour: impl Fn(Pos2) -> Color32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let (top, bottom) = (
        top.min(rect.height() / 2.0),
        bottom.min(rect.height() / 2.0),
    );
    let mut ys = [0.0_f32; ROWS];
    let mut rows = 0;
    let mut push = |dy: f32| {
        if rows < ROWS && (rows == 0 || (dy - ys[rows - 1]).abs() > 1e-3) {
            ys[rows] = dy;
            rows += 1;
        }
    };
    for i in 0..=CORNER_ROWS {
        push(top * i as f32 / CORNER_ROWS as f32);
    }
    let middle = rect.height() - top - bottom;
    let bands = ((middle / BAND).ceil().max(1.0) as usize).min(ROWS - 2 * (CORNER_ROWS + 1));
    for i in 1..bands {
        push(top + middle * i as f32 / bands as f32);
    }
    for i in 0..=CORNER_ROWS {
        push(rect.height() - bottom + bottom * i as f32 / CORNER_ROWS as f32);
    }
    // How far in from the sides the rounded edge is at `dy` from the top.
    let inset = |dy: f32| {
        let side = |r: f32, d: f32| {
            if r <= 0.0 || d >= r {
                0.0
            } else {
                r - (r * r - (r - d) * (r - d)).max(0.0).sqrt()
            }
        };
        side(top, dy).max(side(bottom, rect.height() - dy))
    };
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(rows * (COLUMNS + 1));
    mesh.indices.reserve((rows - 1) * COLUMNS * 6);
    for dy in &ys[..rows] {
        let (left, right) = (rect.left() + inset(*dy), rect.right() - inset(*dy));
        for column in 0..=COLUMNS {
            let at = pos2(
                left + (right - left) * column as f32 / COLUMNS as f32,
                rect.top() + dy,
            );
            mesh.colored_vertex(at, colour(at));
        }
    }
    let stride = (COLUMNS + 1) as u32;
    for row in 0..rows as u32 - 1 {
        for column in 0..COLUMNS as u32 {
            let a = row * stride + column;
            mesh.add_triangle(a, a + 1, a + stride);
            mesh.add_triangle(a + 1, a + stride + 1, a + stride);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// The header's tint: the hue at its strongest at the top, fading to nothing at
/// the foot.
pub(super) fn tint(painter: &Painter, header: Rect, hue: Color32) {
    let top = hue.gamma_multiply(c::HEADER_TINT);
    glass(painter, header, c::RADIUS, 0.0, |at| {
        let t = ((at.y - header.top()) / header.height()).clamp(0.0, 1.0);
        top.lerp_to_gamma(Color32::TRANSPARENT, t)
    });
}

/// The screen's ground: the hue at a share of its strength at the top middle,
/// over the card's own colour, fading to `deep` as it reaches out, the same in
/// both themes. `power` (0 to 1) is how lit it is: off, it is the deep colour
/// alone, a cool dark.
pub(super) fn screen(
    painter: &Painter,
    rect: Rect,
    hue: Color32,
    deep: Color32,
    card: Color32,
    power: f32,
) {
    let glow = card.lerp_to_gamma(hue, c::SCREEN_GLOW * power);
    let reach = c::SCREEN_REACH;
    let (cx, cy) = (rect.center().x, rect.top());
    glass(painter, rect, c::SCREEN_RADIUS, c::SCREEN_RADIUS, |at| {
        let d = (((at.x - cx) / (reach * rect.width())).powi(2)
            + ((at.y - cy) / (reach * rect.height())).powi(2))
        .sqrt()
        .clamp(0.0, 1.0);
        glow.lerp_to_gamma(deep, d)
    });
}
