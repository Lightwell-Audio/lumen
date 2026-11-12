//! The fifteen drum sounds as glyphs (M8 D1's "an icon per drum sound",
//! round 3 of the specimens, `docs/specimens/m8/kit.html`). Each is geometry
//! on the same 16-unit grid as `Icon`, scaled to the target rectangle when
//! painted, so it is sharp at the 12 to 28 pt it is drawn at (a lane, a
//! popover's grid, a track header).
//!
//! The lines are the specimens' shapes with the design system's weights:
//! `ICON` for the ordinary line, `ICON_FINE` for a small detail and
//! `ICON_HEAVY` for a cymbal's edge and a rim's strike. Unlike `Icon`'s, the
//! weight stays the same number of points at every size, as round 3 drew
//! them: a glyph is shown from 12 to 28 pt, and scaling the line with the
//! box made the small ones thin and the big ones heavy (see the sheet in
//! `shots::drum_icons` and the specimens it was compared with).

use eframe::egui::epaint::{CubicBezierShape, QuadraticBezierShape};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use lumen_project::model::DrumIcon;

use crate::theme;

use super::geometry::Grid;

/// A drum sound's glyph. The order is the model's `DrumIcon` order (D0), so
/// mapping one onto the other is a match and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrumGlyph {
    Kick,
    Snare,
    Rim,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Tom,
    FloorTom,
    Crash,
    Ride,
    Cowbell,
    Shaker,
    Percussion,
    Effect,
}

impl DrumGlyph {
    /// Every glyph, in the order the map editor's 5 x 3 grid shows them.
    pub const ALL: [DrumGlyph; 15] = [
        DrumGlyph::Kick,
        DrumGlyph::Snare,
        DrumGlyph::Rim,
        DrumGlyph::Clap,
        DrumGlyph::ClosedHat,
        DrumGlyph::PedalHat,
        DrumGlyph::OpenHat,
        DrumGlyph::Tom,
        DrumGlyph::FloorTom,
        DrumGlyph::Crash,
        DrumGlyph::Ride,
        DrumGlyph::Cowbell,
        DrumGlyph::Shaker,
        DrumGlyph::Percussion,
        DrumGlyph::Effect,
    ];

    /// The glyph's accessible name, as a lane, a choice in the icon popover
    /// and a track header say it.
    pub fn name(self) -> &'static str {
        match self {
            DrumGlyph::Kick => "Kick",
            DrumGlyph::Snare => "Snare",
            DrumGlyph::Rim => "Rim",
            DrumGlyph::Clap => "Clap",
            DrumGlyph::ClosedHat => "Closed hat",
            DrumGlyph::PedalHat => "Pedal hat",
            DrumGlyph::OpenHat => "Open hat",
            DrumGlyph::Tom => "Tom",
            DrumGlyph::FloorTom => "Floor tom",
            DrumGlyph::Crash => "Crash",
            DrumGlyph::Ride => "Ride",
            DrumGlyph::Cowbell => "Cowbell",
            DrumGlyph::Shaker => "Shaker",
            DrumGlyph::Percussion => "Percussion",
            DrumGlyph::Effect => "Effect",
        }
    }
}

/// Paints `glyph` centred in `rect` (a square of the glyph's size) in
/// `color`. The caller snaps `rect` to whole pixels, as the icon buttons do.
pub fn paint_drum_glyph(painter: &Painter, rect: Rect, glyph: DrumGlyph, color: Color32) {
    for shape in shapes(glyph, rect, color) {
        painter.add(shape);
    }
}

/// The glyph a sound's model icon is drawn with (D0's `DrumIcon`), so the
/// map, the lanes and the piano roll's keys share one drawing.
impl From<DrumIcon> for DrumGlyph {
    fn from(icon: DrumIcon) -> Self {
        match icon {
            DrumIcon::Kick => DrumGlyph::Kick,
            DrumIcon::Snare => DrumGlyph::Snare,
            DrumIcon::Rim => DrumGlyph::Rim,
            DrumIcon::Clap => DrumGlyph::Clap,
            DrumIcon::ClosedHat => DrumGlyph::ClosedHat,
            DrumIcon::PedalHat => DrumGlyph::PedalHat,
            DrumIcon::OpenHat => DrumGlyph::OpenHat,
            DrumIcon::Tom => DrumGlyph::Tom,
            DrumIcon::FloorTom => DrumGlyph::FloorTom,
            DrumIcon::Crash => DrumGlyph::Crash,
            DrumIcon::Ride => DrumGlyph::Ride,
            DrumIcon::Cowbell => DrumGlyph::Cowbell,
            DrumIcon::Shaker => DrumGlyph::Shaker,
            DrumIcon::Percussion => DrumGlyph::Percussion,
            DrumIcon::Effect => DrumGlyph::Effect,
        }
    }
}

/// The model icon a drawing stands for (the icon popover picks a drawing and
/// the map stores its icon).
impl From<DrumGlyph> for DrumIcon {
    fn from(glyph: DrumGlyph) -> Self {
        match glyph {
            DrumGlyph::Kick => DrumIcon::Kick,
            DrumGlyph::Snare => DrumIcon::Snare,
            DrumGlyph::Rim => DrumIcon::Rim,
            DrumGlyph::Clap => DrumIcon::Clap,
            DrumGlyph::ClosedHat => DrumIcon::ClosedHat,
            DrumGlyph::PedalHat => DrumIcon::PedalHat,
            DrumGlyph::OpenHat => DrumIcon::OpenHat,
            DrumGlyph::Tom => DrumIcon::Tom,
            DrumGlyph::FloorTom => DrumIcon::FloorTom,
            DrumGlyph::Crash => DrumIcon::Crash,
            DrumGlyph::Ride => DrumIcon::Ride,
            DrumGlyph::Cowbell => DrumIcon::Cowbell,
            DrumGlyph::Shaker => DrumIcon::Shaker,
            DrumGlyph::Percussion => DrumIcon::Percussion,
            DrumGlyph::Effect => DrumIcon::Effect,
        }
    }
}

/// The shapes for `glyph` in `rect`.
fn shapes(glyph: DrumGlyph, rect: Rect, color: Color32) -> Vec<Shape> {
    let g = Grid::new(rect);
    let stroke = Stroke::new(theme::stroke::ICON, color);
    let fine = Stroke::new(theme::stroke::ICON_FINE, color);
    let heavy = Stroke::new(theme::stroke::ICON_HEAVY, color);
    let none = Color32::TRANSPARENT;
    let quad = |from: (f32, f32), control: (f32, f32), to: (f32, f32), stroke: Stroke| {
        QuadraticBezierShape::from_points_stroke(
            [
                g.p(from.0, from.1),
                g.p(control.0, control.1),
                g.p(to.0, to.1),
            ],
            false,
            none,
            stroke,
        )
    };
    let cubic = |points: [(f32, f32); 4], stroke: Stroke| {
        CubicBezierShape::from_points_stroke(points.map(|(x, y)| g.p(x, y)), false, none, stroke)
    };
    match glyph {
        // A head seen face on, its eye, and two spurs on the floor.
        DrumGlyph::Kick => vec![
            Shape::circle_stroke(g.p(8.0, 8.5), 6.0 * g.unit, stroke),
            Shape::circle_filled(g.p(8.0, 8.5), 2.4 * g.unit, color),
            Shape::line_segment([g.p(3.0, 14.3), g.p(2.0, 15.4)], stroke),
            Shape::line_segment([g.p(13.0, 14.3), g.p(14.0, 15.4)], stroke),
        ],
        // A shell with the snares under its head.
        DrumGlyph::Snare => {
            let mut shell = vec![g.p(2.0, 6.0)];
            shell.extend(shell_bottom(&g, 8.0, 11.0, 6.0, 2.0));
            shell.push(g.p(14.0, 6.0));
            vec![
                Shape::ellipse_stroke(g.p(8.0, 6.0), Vec2::new(6.0, 2.0) * g.unit, stroke),
                Shape::line(shell, stroke),
                Shape::line_segment([g.p(4.0, 8.5), g.p(6.0, 11.5)], fine),
                Shape::line_segment([g.p(8.0, 8.8), g.p(8.0, 12.2)], fine),
                Shape::line_segment([g.p(12.0, 8.5), g.p(10.0, 11.5)], fine),
            ]
        }
        // The hoop, and a stick struck across its edge.
        DrumGlyph::Rim => vec![
            Shape::ellipse_stroke(g.p(8.0, 9.0), Vec2::new(6.0, 2.0) * g.unit, stroke),
            Shape::line_segment([g.p(3.0, 2.0), g.p(12.0, 9.0)], heavy),
        ],
        // Two hands and three sparks.
        DrumGlyph::Clap => vec![
            Shape::CubicBezier(cubic(
                [(5.0, 14.0), (3.0, 11.0), (3.0, 7.0), (5.0, 4.0)],
                stroke,
            )),
            Shape::CubicBezier(cubic(
                [(8.0, 14.0), (6.5, 11.0), (6.5, 7.0), (8.0, 4.0)],
                stroke,
            )),
            Shape::line_segment([g.p(11.0, 3.0), g.p(12.5, 1.5)], stroke),
            Shape::line_segment([g.p(12.5, 6.0), g.p(14.5, 6.0)], stroke),
            Shape::line_segment([g.p(11.5, 9.0), g.p(13.1, 10.2)], stroke),
        ],
        // Two cymbals closed together, on their stand.
        DrumGlyph::ClosedHat => vec![
            Shape::QuadraticBezier(quad((1.5, 6.5), (8.0, 3.5), (14.5, 6.5), stroke)),
            Shape::QuadraticBezier(quad((1.5, 7.8), (8.0, 10.8), (14.5, 7.8), stroke)),
            Shape::line_segment([g.p(8.0, 4.0), g.p(8.0, 1.5)], stroke),
            Shape::line_segment([g.p(8.0, 9.5), g.p(8.0, 15.0)], stroke),
        ],
        // The same, closed by the foot pedal under them.
        DrumGlyph::PedalHat => vec![
            Shape::QuadraticBezier(quad((1.5, 4.5), (8.0, 1.5), (14.5, 4.5), stroke)),
            Shape::QuadraticBezier(quad((1.5, 5.8), (8.0, 8.8), (14.5, 5.8), stroke)),
            Shape::line_segment([g.p(8.0, 7.5), g.p(8.0, 12.5)], stroke),
            Shape::line(
                vec![g.p(4.0, 15.0), g.p(8.0, 12.5), g.p(12.0, 15.0)],
                stroke,
            ),
        ],
        // Two cymbals apart, their air between.
        DrumGlyph::OpenHat => vec![
            Shape::QuadraticBezier(quad((1.5, 5.0), (8.0, 2.0), (14.5, 5.0), stroke)),
            Shape::QuadraticBezier(quad((1.5, 9.5), (8.0, 12.5), (14.5, 9.5), stroke)),
            Shape::line_segment([g.p(8.0, 2.5), g.p(8.0, 1.0)], stroke),
            Shape::line_segment([g.p(8.0, 11.5), g.p(8.0, 15.0)], stroke),
            Shape::line_segment([g.p(4.0, 7.2), g.p(5.0, 7.2)], stroke),
            Shape::line_segment([g.p(7.5, 7.2), g.p(8.5, 7.2)], stroke),
            Shape::line_segment([g.p(11.0, 7.2), g.p(12.0, 7.2)], stroke),
        ],
        // A shell with its lugs; the floor tom is taller and stands on legs.
        DrumGlyph::Tom => {
            let mut shell = vec![g.p(3.0, 5.0)];
            shell.extend(shell_bottom(&g, 8.0, 11.0, 5.0, 1.8));
            shell.push(g.p(13.0, 5.0));
            vec![
                Shape::ellipse_stroke(g.p(8.0, 5.0), Vec2::new(5.0, 1.8) * g.unit, stroke),
                Shape::line(shell, stroke),
                Shape::line_segment([g.p(5.5, 7.0), g.p(5.5, 12.5)], fine),
                Shape::line_segment([g.p(10.5, 7.0), g.p(10.5, 12.5)], fine),
            ]
        }
        DrumGlyph::FloorTom => {
            let mut shell = vec![g.p(2.5, 3.8)];
            shell.extend(shell_bottom(&g, 8.0, 11.8, 5.5, 1.8));
            shell.push(g.p(13.5, 3.8));
            vec![
                Shape::ellipse_stroke(g.p(8.0, 3.8), Vec2::new(5.5, 1.8) * g.unit, stroke),
                Shape::line(shell, stroke),
                Shape::line_segment([g.p(3.5, 13.5), g.p(2.5, 15.3)], stroke),
                Shape::line_segment([g.p(12.5, 13.5), g.p(13.5, 15.3)], stroke),
            ]
        }
        // A cymbal seen on edge, its bell's strike above, on its stand.
        DrumGlyph::Crash => vec![
            Shape::line_segment([g.p(1.0, 9.0), g.p(14.5, 4.5)], heavy),
            Shape::line_segment([g.p(8.0, 7.0), g.p(8.0, 15.0)], stroke),
            Shape::line_segment([g.p(12.0, 1.5), g.p(13.0, 0.5)], fine),
            Shape::line_segment([g.p(14.3, 2.5), g.p(15.2, 2.05)], fine),
        ],
        // A cymbal with its bell, seen slightly from above.
        DrumGlyph::Ride => vec![
            Shape::QuadraticBezier(quad((1.0, 7.5), (8.0, 4.9), (15.0, 7.5), stroke)),
            Shape::QuadraticBezier(quad((1.0, 7.5), (8.0, 10.1), (15.0, 7.5), stroke)),
            Shape::circle_filled(g.p(8.0, 7.5), 1.4 * g.unit, color),
            Shape::line_segment([g.p(8.0, 9.5), g.p(8.0, 15.0)], stroke),
        ],
        DrumGlyph::Cowbell => vec![
            Shape::closed_line(
                vec![
                    g.p(5.0, 3.0),
                    g.p(11.0, 3.0),
                    g.p(13.5, 13.0),
                    g.p(2.5, 13.0),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(8.0, 3.0), g.p(8.0, 1.0)], stroke),
        ],
        // A hand shaker, with the beads loose inside it.
        DrumGlyph::Shaker => vec![
            Shape::rect_stroke(
                g.rect(4.5, 2.0, 11.5, 14.0),
                3.5 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::circle_filled(g.p(7.0, 6.0), 0.9 * g.unit, color),
            Shape::circle_filled(g.p(9.0, 8.0), 0.9 * g.unit, color),
            Shape::circle_filled(g.p(7.0, 10.0), 0.9 * g.unit, color),
        ],
        // A conga: a tapered drum with its skin.
        DrumGlyph::Percussion => vec![
            Shape::closed_line(
                vec![
                    g.p(3.0, 4.0),
                    g.p(13.0, 4.0),
                    g.p(11.5, 14.0),
                    g.p(4.5, 14.0),
                ],
                stroke,
            ),
            Shape::ellipse_stroke(g.p(8.0, 4.0), Vec2::new(5.0, 1.4) * g.unit, stroke),
        ],
        // A struck star: a sound that is not a drum.
        DrumGlyph::Effect => vec![Shape::closed_line(
            vec![
                g.p(8.0, 1.5),
                g.p(9.6, 5.7),
                g.p(14.0, 6.1),
                g.p(10.6, 8.9),
                g.p(11.7, 13.2),
                g.p(8.0, 10.8),
                g.p(4.3, 13.2),
                g.p(5.4, 8.9),
                g.p(2.0, 6.1),
                g.p(6.4, 5.7),
            ],
            stroke,
        )],
    }
}

/// The half ellipse a drum shell's bottom makes, left edge to right edge, on
/// the 16-unit grid.
fn shell_bottom(g: &Grid, centre_x: f32, centre_y: f32, rx: f32, ry: f32) -> Vec<Pos2> {
    const STEPS: usize = 16;
    (0..=STEPS)
        .map(|i| {
            let angle = std::f32::consts::PI * (1.0 - i as f32 / STEPS as f32);
            g.p(centre_x + rx * angle.cos(), centre_y + ry * angle.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Vec2, pos2};

    use super::*;

    #[test]
    fn every_glyph_stays_inside_its_box_at_every_size() {
        for size in [12.0, 16.0, 20.0, 28.0] {
            let rect = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(size));
            for glyph in DrumGlyph::ALL {
                let shapes = shapes(glyph, rect, Color32::WHITE);
                assert!(!shapes.is_empty(), "{glyph:?} at {size}");
                for shape in shapes {
                    let bounds = shape.visual_bounding_rect();
                    assert!(
                        rect.expand(0.5).contains_rect(bounds),
                        "{glyph:?} at {size}: {bounds:?} outside {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_glyph_has_a_name_of_its_own() {
        let mut names: Vec<&str> = DrumGlyph::ALL.iter().map(|g| g.name()).collect();
        assert_eq!(names.len(), 15);
        assert!(names.iter().all(|name| !name.is_empty()));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 15, "a name is used twice");
    }

    /// The model's fifteen icons and the fifteen drawings line up, in name
    /// as well as in order (M8 D1): a new icon on either side fails here.
    #[test]
    fn the_models_icons_and_the_drawings_line_up_by_name() {
        let spoken = |name: &str| name.replace('_', " ").to_lowercase();
        assert_eq!(DrumIcon::ALL.len(), DrumGlyph::ALL.len());
        for (icon, glyph) in DrumIcon::ALL.into_iter().zip(DrumGlyph::ALL) {
            assert_eq!(
                spoken(icon.name()),
                glyph.name().to_lowercase(),
                "{icon:?} is drawn as {glyph:?}"
            );
            assert_eq!(DrumGlyph::from(icon), glyph);
            assert_eq!(DrumIcon::from(glyph), icon);
        }
    }

    #[test]
    fn glyphs_grow_with_their_box() {
        let small = Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
        let large = Rect::from_min_size(Pos2::ZERO, Vec2::splat(28.0));
        for glyph in DrumGlyph::ALL {
            let width = |r| {
                shapes(glyph, r, Color32::WHITE)
                    .iter()
                    .map(Shape::visual_bounding_rect)
                    .fold(Rect::NOTHING, |a, b| a.union(b))
                    .width()
            };
            // The line keeps its weight, so the picture grows by the box's
            // growth less the line's share.
            let ratio = width(large) / width(small);
            assert!((1.5..2.05).contains(&ratio), "{glyph:?}: {ratio}");
        }
    }
}
