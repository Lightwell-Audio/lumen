use eframe::egui::{Color32, Painter, Pos2, Stroke};

const GLYPH: f32 = 13.0;
const WEIGHT: f32 = 1.5;

struct Grid {
    origin: Pos2,
    unit: f32,
}

impl Grid {
    fn new(c: Pos2) -> Self {
        let unit = GLYPH / 16.0;
        Self {
            origin: c - eframe::egui::Vec2::splat(GLYPH / 2.0),
            unit,
        }
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + eframe::egui::vec2(x, y) * self.unit
    }

    fn stroke(&self, colour: Color32) -> Stroke {
        Stroke::new(WEIGHT * self.unit * 1.15, colour)
    }

    fn path(&self, painter: &Painter, points: &[(f32, f32)], colour: Color32, close: bool) {
        let mut pts: Vec<Pos2> = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        if close {
            pts.push(pts[0]);
        }
        painter.add(eframe::egui::Shape::line(pts, self.stroke(colour)));
    }
}

const ARROW: [(f32, f32); 4] = [(3.0, 3.0), (12.0, 8.0), (8.0, 9.2), (6.8, 13.0)];

pub(crate) fn pointer(painter: &Painter, c: Pos2, colour: Color32) {
    Grid::new(c).path(painter, &ARROW, colour, true);
}

pub(crate) fn pencil(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(
        painter,
        &[
            (2.5, 13.5),
            (3.5, 10.0),
            (11.0, 2.5),
            (13.5, 5.0),
            (6.0, 12.5),
        ],
        colour,
        true,
    );
}

pub(crate) fn line(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(painter, &[(2.5, 12.5), (13.5, 3.5)], colour, false);
}

pub(crate) fn curve(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    let cubic = |t: f32| {
        let u = 1.0 - t;
        let x = u * u * u * 2.0 + 3.0 * u * u * t * 6.0 + 3.0 * u * t * t * 6.0 + t * t * t * 14.0;
        let y = u * u * u * 13.0 + 3.0 * u * u * t * 13.0 + 3.0 * u * t * t * 3.0 + t * t * t * 3.0;
        (x, y)
    };
    let points: Vec<(f32, f32)> = (0..=12).map(|i| cubic(i as f32 / 12.0)).collect();
    g.path(painter, &points, colour, false);
}

pub(crate) fn lane(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(
        painter,
        &[(2.5, 4.5), (13.5, 4.5), (13.5, 11.5), (2.5, 11.5)],
        colour,
        true,
    );
    g.path(painter, &[(2.5, 9.5), (13.5, 9.5)], colour, false);
    g.path(
        painter,
        &[(5.0, 12.0), (7.0, 10.5), (9.0, 11.5), (11.0, 10.0)],
        colour,
        false,
    );
}

pub(crate) fn pop_out(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(
        painter,
        &[(9.0, 2.5), (13.5, 2.5), (13.5, 7.0)],
        colour,
        false,
    );
    g.path(painter, &[(13.5, 2.5), (8.0, 8.0)], colour, false);
    g.path(
        painter,
        &[
            (12.0, 10.0),
            (12.0, 12.5),
            (11.0, 13.5),
            (3.5, 13.5),
            (2.5, 12.5),
            (2.5, 5.0),
            (3.5, 4.0),
            (6.0, 4.0),
        ],
        colour,
        false,
    );
}

pub(crate) fn keys(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(
        painter,
        &[(2.5, 3.5), (13.5, 3.5), (13.5, 13.0), (2.5, 13.0)],
        colour,
        true,
    );
    for x in [5.5, 8.5, 11.5] {
        g.path(painter, &[(x, 3.5), (x, 8.5)], colour, false);
    }
}

pub(crate) fn dock(painter: &Painter, c: Pos2, colour: Color32) {
    let g = Grid::new(c);
    g.path(
        painter,
        &[(12.0, 7.0), (8.0, 7.0), (8.0, 3.0)],
        colour,
        false,
    );
    g.path(painter, &[(8.0, 7.0), (13.5, 1.5)], colour, false);
    g.path(
        painter,
        &[
            (12.0, 10.0),
            (12.0, 12.5),
            (11.0, 13.5),
            (3.5, 13.5),
            (2.5, 12.5),
            (2.5, 5.0),
            (3.5, 4.0),
            (6.0, 4.0),
        ],
        colour,
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_is_centred_on_the_grid_from_top_to_bottom() {
        let top = ARROW.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let bottom = ARROW.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        assert_eq!((top + bottom) / 2.0, 8.0, "{top} to {bottom}");
    }
}
