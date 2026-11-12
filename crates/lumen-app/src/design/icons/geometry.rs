//! Each icon's geometry on the 16 x 16 grid: the mapped grid `shapes` draws
//! one icon in, and the shape of every icon in `Icon`.

use std::f32::consts::{PI, TAU};

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};

use crate::theme;

use super::Icon;

/// How strongly an icon's secondary line is drawn (the tempo lanes' meter
/// line).
const FAINT: f32 = 0.45;

/// Maps the 16-unit icon grid onto a square centred in `rect`.
struct Grid {
    origin: Pos2,
    unit: f32,
}

impl Grid {
    fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height());
        Self {
            origin: rect.center() - Vec2::splat(side / 2.0),
            unit: side / 16.0,
        }
    }

    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.unit
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
}

/// The shapes for `icon` in `rect`.
pub fn shapes(icon: Icon, rect: Rect, color: Color32) -> Vec<Shape> {
    let g = Grid::new(rect);
    let stroke = Stroke::new(theme::stroke::ICON * g.unit, color);
    let none = Stroke::NONE;
    match icon {
        Icon::Pointer => vec![Shape::closed_line(
            vec![
                g.p(4.0, 2.5),
                g.p(4.0, 12.5),
                g.p(6.8, 9.9),
                g.p(8.7, 13.9),
                g.p(10.3, 13.1),
                g.p(8.4, 9.2),
                g.p(12.0, 9.2),
            ],
            Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
        )],
        Icon::Pencil => vec![
            Shape::closed_line(
                vec![
                    g.p(3.0, 13.0),
                    g.p(4.0, 9.5),
                    g.p(10.5, 3.0),
                    g.p(13.0, 5.5),
                    g.p(6.5, 12.0),
                ],
                Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
            ),
            Shape::line_segment(
                [g.p(9.0, 4.5), g.p(11.5, 7.0)],
                Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
            ),
        ],
        Icon::Eraser => vec![
            Shape::closed_line(
                vec![
                    g.p(3.5, 9.5),
                    g.p(8.5, 4.5),
                    g.p(12.5, 8.5),
                    g.p(8.0, 13.0),
                    g.p(6.0, 13.0),
                ],
                Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
            ),
            Shape::line_segment(
                [g.p(8.0, 13.0), g.p(13.0, 13.0)],
                Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
            ),
        ],
        Icon::Split => vec![
            Shape::line_segment([g.p(8.0, 2.0), g.p(8.0, 14.0)], stroke),
            Shape::line_segment([g.p(3.0, 5.0), g.p(3.0, 11.0)], stroke),
            Shape::line_segment([g.p(13.0, 5.0), g.p(13.0, 11.0)], stroke),
        ],
        Icon::MuteCircle => vec![
            Shape::circle_stroke(g.p(8.0, 8.0), 5.5 * g.unit, stroke),
            Shape::line_segment([g.p(4.2, 11.8), g.p(11.8, 4.2)], stroke),
        ],
        Icon::ZoomIn => vec![
            Shape::circle_stroke(g.p(7.0, 7.0), 4.5 * g.unit, stroke),
            Shape::line_segment([g.p(10.5, 10.5), g.p(14.0, 14.0)], stroke),
            Shape::line_segment([g.p(5.0, 7.0), g.p(9.0, 7.0)], stroke),
            Shape::line_segment([g.p(7.0, 5.0), g.p(7.0, 9.0)], stroke),
        ],
        Icon::TempoLanes => vec![
            Shape::line_segment(
                [g.p(1.5, 4.0), g.p(14.5, 4.0)],
                Stroke::new(stroke.width, color.gamma_multiply(FAINT)),
            ),
            Shape::line(
                vec![
                    g.p(1.5, 12.5),
                    g.p(5.5, 12.5),
                    g.p(5.5, 7.5),
                    g.p(9.5, 7.5),
                    g.p(9.5, 10.5),
                    g.p(14.5, 10.5),
                ],
                stroke,
            ),
        ],
        Icon::Play => vec![Shape::convex_polygon(
            vec![g.p(4.0, 2.5), g.p(13.5, 8.0), g.p(4.0, 13.5)],
            color,
            none,
        )],
        Icon::Pause => vec![
            Shape::rect_filled(g.rect(3.5, 3.0, 6.5, 13.0), 0.5 * g.unit, color),
            Shape::rect_filled(g.rect(9.5, 3.0, 12.5, 13.0), 0.5 * g.unit, color),
        ],
        Icon::Stop => vec![Shape::rect_filled(
            g.rect(3.5, 3.5, 12.5, 12.5),
            0.75 * g.unit,
            color,
        )],
        Icon::Record => vec![Shape::circle_filled(g.p(8.0, 8.0), 4.75 * g.unit, color)],
        // The ring's outer edge is the filled dot's radius: the stroke is
        // centred 0.8 units inside it.
        Icon::RecordRing => vec![Shape::circle_stroke(g.p(8.0, 8.0), 3.95 * g.unit, stroke)],
        Icon::Layers => vec![
            Shape::closed_line(
                vec![g.p(8.0, 2.0), g.p(14.0, 5.0), g.p(8.0, 8.0), g.p(2.0, 5.0)],
                stroke,
            ),
            Shape::line(vec![g.p(2.0, 8.5), g.p(8.0, 11.5), g.p(14.0, 8.5)], stroke),
            Shape::line(
                vec![g.p(2.0, 11.5), g.p(8.0, 14.5), g.p(14.0, 11.5)],
                stroke,
            ),
        ],
        Icon::Star | Icon::StarFilled => {
            // Ten points, outer and inner in turn, from the top.
            let centre = g.p(8.0, 8.6);
            let points: Vec<Pos2> = (0..10)
                .map(|k| {
                    let radius = if k % 2 == 0 { 6.2 } else { 2.7 };
                    let angle = -PI / 2.0 + PI * k as f32 / 5.0;
                    centre + radius * g.unit * Vec2::angled(angle)
                })
                .collect();
            let mut shapes = Vec::new();
            if icon == Icon::StarFilled {
                // A star is not convex, but every point sees its centre:
                // a fan of triangles fills it.
                let mut mesh = eframe::egui::Mesh::default();
                mesh.colored_vertex(centre, color);
                for point in &points {
                    mesh.colored_vertex(*point, color);
                }
                for k in 0..10u32 {
                    mesh.add_triangle(0, 1 + k, 1 + (k + 1) % 10);
                }
                shapes.push(Shape::mesh(mesh));
            }
            shapes.push(Shape::closed_line(
                points,
                Stroke::new(theme::stroke::ICON_FINE * g.unit, color),
            ));
            shapes
        }
        Icon::Keys => vec![
            Shape::closed_line(
                vec![
                    g.p(2.0, 4.0),
                    g.p(14.0, 4.0),
                    g.p(14.0, 12.0),
                    g.p(2.0, 12.0),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(6.0, 8.0), g.p(6.0, 12.0)], stroke),
            Shape::line_segment([g.p(10.0, 8.0), g.p(10.0, 12.0)], stroke),
        ],
        Icon::Knob => vec![
            Shape::circle_stroke(g.p(8.0, 8.0), 5.2 * g.unit, stroke),
            Shape::line_segment([g.p(8.0, 8.0), g.p(10.6, 5.4)], stroke),
        ],
        Icon::Warning => vec![
            Shape::closed_line(vec![g.p(8.0, 2.4), g.p(14.2, 13.2), g.p(1.8, 13.2)], stroke),
            Shape::line_segment([g.p(8.0, 6.6), g.p(8.0, 9.6)], stroke),
            Shape::circle_filled(g.p(8.0, 11.5), 0.9 * g.unit, color),
        ],
        Icon::Power => {
            // An open ring with a bar through its gap.
            let (centre, radius) = (g.p(8.0, 8.5), 4.8 * g.unit);
            let (start, sweep) = (-PI / 2.0 + 0.7, TAU - 1.4);
            let arc: Vec<Pos2> = (0..=28)
                .map(|i| centre + radius * Vec2::angled(start + sweep * i as f32 / 28.0))
                .collect();
            vec![
                Shape::line(arc, stroke),
                Shape::line_segment([g.p(8.0, 2.2), g.p(8.0, 7.6)], stroke),
            ]
        }
        Icon::Sliders => vec![
            Shape::line_segment([g.p(2.5, 4.0), g.p(13.5, 4.0)], stroke),
            Shape::line_segment([g.p(2.5, 8.0), g.p(13.5, 8.0)], stroke),
            Shape::line_segment([g.p(2.5, 12.0), g.p(13.5, 12.0)], stroke),
            Shape::circle_filled(g.p(10.0, 4.0), 1.7 * g.unit, color),
            Shape::circle_filled(g.p(5.5, 8.0), 1.7 * g.unit, color),
            Shape::circle_filled(g.p(9.5, 12.0), 1.7 * g.unit, color),
        ],
        Icon::PlugOff => vec![
            Shape::line_segment([g.p(6.0, 2.2), g.p(6.0, 5.0)], stroke),
            Shape::line_segment([g.p(10.0, 2.2), g.p(10.0, 5.0)], stroke),
            Shape::closed_line(
                vec![
                    g.p(4.2, 5.0),
                    g.p(11.8, 5.0),
                    g.p(11.8, 8.2),
                    g.p(8.0, 11.0),
                    g.p(4.2, 8.2),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(8.0, 11.0), g.p(8.0, 13.8)], stroke),
            Shape::line_segment([g.p(2.6, 2.6), g.p(13.4, 13.4)], stroke),
        ],
        Icon::Shield => vec![
            Shape::closed_line(
                vec![
                    g.p(8.0, 2.0),
                    g.p(13.2, 4.0),
                    g.p(13.2, 8.4),
                    g.p(11.4, 11.6),
                    g.p(8.0, 14.0),
                    g.p(4.6, 11.6),
                    g.p(2.8, 8.4),
                    g.p(2.8, 4.0),
                ],
                stroke,
            ),
            Shape::line(vec![g.p(5.6, 8.2), g.p(7.4, 10.0), g.p(10.6, 6.2)], stroke),
        ],
        Icon::ToStart => vec![
            Shape::rect_filled(g.rect(3.0, 3.0, 5.0, 13.0), 0.3 * g.unit, color),
            Shape::convex_polygon(
                vec![g.p(13.0, 3.0), g.p(13.0, 13.0), g.p(5.5, 8.0)],
                color,
                none,
            ),
        ],
        Icon::Loop => {
            // An almost-closed circle with an arrowhead at its end.
            let (centre, radius) = (g.p(8.0, 8.0), 5.0 * g.unit);
            let (start, sweep) = (-PI / 2.0 + 0.55, TAU - 1.1);
            let at = |angle: f32| centre + radius * Vec2::angled(angle);
            let arc: Vec<Pos2> = (0..=32)
                .map(|i| at(start + sweep * i as f32 / 32.0))
                .collect();
            let end_angle = start + sweep;
            let tip = at(end_angle);
            let along = Vec2::angled(end_angle + PI / 2.0); // direction of travel
            let across = Vec2::angled(end_angle);
            let head = vec![
                tip + along * 2.6 * g.unit,
                tip + across * 2.2 * g.unit,
                tip - across * 2.2 * g.unit,
            ];
            vec![
                Shape::line(arc, stroke),
                Shape::convex_polygon(head, color, none),
            ]
        }
        Icon::Metronome => vec![
            Shape::closed_line(
                vec![
                    g.p(6.3, 2.5),
                    g.p(9.7, 2.5),
                    g.p(12.0, 13.5),
                    g.p(4.0, 13.5),
                ],
                stroke,
            ),
            Shape::line_segment([g.p(8.0, 11.0), g.p(12.0, 4.0)], stroke),
            Shape::circle_filled(g.p(10.3, 7.0), 1.3 * g.unit, color),
        ],
        Icon::Magnet | Icon::MagnetOff => {
            // A horseshoe: two legs joined by a half circle on top, with
            // pole tips at the bottom.
            let thick = Stroke::new(theme::stroke::ICON_BOLD * g.unit, color);
            let mut arch: Vec<Pos2> = (0..=12)
                .map(|k| {
                    let a = PI + PI * k as f32 / 12.0;
                    g.p(8.0 + 4.0 * a.cos(), 7.5 + 4.0 * a.sin())
                })
                .collect();
            // The legs stop short of the tips, so the notch shows at 100%.
            arch.insert(0, g.p(4.0, 10.0));
            arch.push(g.p(12.0, 10.0));
            let mut shapes = vec![
                Shape::line(arch, thick),
                Shape::rect_filled(g.rect(2.6, 11.6, 5.4, 14.4), 0.3 * g.unit, color),
                Shape::rect_filled(g.rect(10.6, 11.6, 13.4, 14.4), 0.3 * g.unit, color),
            ];
            if icon == Icon::MagnetOff {
                shapes.push(Shape::line_segment(
                    [g.p(2.0, 2.0), g.p(14.0, 14.0)],
                    stroke,
                ));
            }
            shapes
        }
        Icon::Lock => vec![
            // A shackle over a solid body.
            Shape::line(
                std::iter::once(g.p(5.0, 8.0))
                    .chain((0..=12).map(|k| {
                        let a = PI + PI * k as f32 / 12.0;
                        g.p(8.0 + 3.0 * a.cos(), 6.5 + 3.5 * a.sin())
                    }))
                    .chain([g.p(11.0, 8.0)])
                    .collect(),
                stroke,
            ),
            Shape::rect_filled(g.rect(3.0, 7.5, 13.0, 14.5), 1.0 * g.unit, color),
        ],
        // The ring is a fully rounded square on the same half-point grid
        // as the framed icons beside it (the panel toggles, 2.5 to 13.5
        // top to bottom),
        // centred on their middle line: rectangles snap to pixels and
        // circles do not, so drawn this way it stays level with them at
        // every scale.
        Icon::Search => vec![
            Shape::rect_stroke(
                g.rect(2.5, 3.5, 11.5, 12.5),
                4.5 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::line_segment([g.p(10.2, 11.2), g.p(13.25, 14.25)], stroke),
        ],
        Icon::Mouse => vec![
            // A rounded body with the buttons' parting line.
            Shape::rect_stroke(
                g.rect(4.0, 1.5, 12.0, 14.5),
                4.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::line_segment([g.p(8.0, 1.5), g.p(8.0, 6.5)], stroke),
        ],
        Icon::Keycap => vec![
            Shape::rect_stroke(
                g.rect(1.5, 1.5, 14.5, 14.5),
                3.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::rect_stroke(
                g.rect(4.5, 4.0, 11.5, 9.5),
                1.5 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
        ],
        Icon::Din => {
            // The shell, and five pins on its upper arc (180 degrees).
            let pin = 1.1 * g.unit;
            let mut shapes = vec![Shape::circle_stroke(g.p(8.0, 8.0), 6.6 * g.unit, stroke)];
            for (x, y) in [(3.6, 9.6), (4.7, 6.0), (8.0, 4.6), (11.3, 6.0), (12.4, 9.6)] {
                shapes.push(Shape::circle_filled(g.p(x, y), pin, color));
            }
            shapes
        }
        Icon::Speaker | Icon::SpeakerOff => {
            // A cone and box, and one wave; Off adds the slash.
            let mut shapes = vec![
                Shape::convex_polygon(
                    vec![
                        g.p(2.0, 6.0),
                        g.p(5.2, 6.0),
                        g.p(9.0, 2.8),
                        g.p(9.0, 13.2),
                        g.p(5.2, 10.0),
                        g.p(2.0, 10.0),
                    ],
                    color,
                    none,
                ),
                Shape::line(
                    (0..=8)
                        .map(|k| {
                            let a = -0.9 + 1.8 * k as f32 / 8.0;
                            g.p(8.4 + 3.6 * a.cos(), 8.0 + 3.6 * a.sin())
                        })
                        .collect(),
                    stroke,
                ),
            ];
            if icon == Icon::SpeakerOff {
                shapes.push(Shape::line_segment(
                    [g.p(2.0, 14.0), g.p(14.0, 2.0)],
                    Stroke::new(theme::stroke::ICON_STRIKE * g.unit, color),
                ));
            }
            shapes
        }
        Icon::PanelLeft | Icon::PanelBottom => {
            let (from, to) = if icon == Icon::PanelLeft {
                ([6.0, 2.5], [6.0, 13.5])
            } else {
                ([1.5, 9.5], [14.5, 9.5])
            };
            vec![
                Shape::rect_stroke(
                    g.rect(1.5, 2.5, 14.5, 13.5),
                    2.0 * g.unit,
                    stroke,
                    egui::StrokeKind::Middle,
                ),
                Shape::line_segment([g.p(from[0], from[1]), g.p(to[0], to[1])], stroke),
            ]
        }
        Icon::Rack => vec![
            Shape::rect_stroke(
                g.rect(2.0, 3.0, 14.0, 13.0),
                2.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::line_segment([g.p(5.5, 6.0), g.p(5.5, 10.0)], stroke),
            Shape::line_segment([g.p(8.0, 6.0), g.p(8.0, 10.0)], stroke),
            Shape::line_segment([g.p(10.5, 6.0), g.p(10.5, 10.0)], stroke),
        ],
        Icon::Wave => vec![Shape::line(
            vec![
                g.p(1.0, 8.0),
                g.p(3.0, 8.0),
                g.p(4.5, 3.0),
                g.p(7.5, 13.0),
                g.p(10.0, 6.0),
                g.p(11.5, 10.0),
                g.p(15.0, 10.0),
            ],
            stroke,
        )],
        Icon::Document => vec![
            Shape::closed_line(
                vec![
                    g.p(4.0, 2.0),
                    g.p(10.0, 2.0),
                    g.p(13.0, 5.0),
                    g.p(13.0, 14.0),
                    g.p(4.0, 14.0),
                ],
                stroke,
            ),
            Shape::line(vec![g.p(10.0, 2.0), g.p(10.0, 5.0), g.p(13.0, 5.0)], stroke),
        ],
        Icon::Folder => vec![Shape::closed_line(
            vec![
                g.p(2.0, 4.0),
                g.p(6.0, 4.0),
                g.p(7.5, 5.5),
                g.p(14.0, 5.5),
                g.p(14.0, 13.0),
                g.p(2.0, 13.0),
            ],
            stroke,
        )],
        Icon::Disc => vec![
            Shape::circle_stroke(g.p(8.0, 8.0), 5.5 * g.unit, stroke),
            Shape::circle_filled(g.p(8.0, 8.0), 1.5 * g.unit, color),
        ],
        Icon::Reverse => vec![
            // Two short wave bars under an arrow that points back.
            Shape::line_segment([g.p(13.5, 5.0), g.p(3.5, 5.0)], stroke),
            Shape::convex_polygon(
                vec![g.p(2.0, 5.0), g.p(6.0, 2.2), g.p(6.0, 7.8)],
                color,
                none,
            ),
            Shape::line_segment([g.p(5.0, 10.0), g.p(5.0, 13.5)], stroke),
            Shape::line_segment([g.p(8.0, 9.0), g.p(8.0, 14.5)], stroke),
            Shape::line_segment([g.p(11.0, 10.5), g.p(11.0, 13.0)], stroke),
        ],
        Icon::Polarity => vec![
            // The slash runs past the ring, which tells it from the Mute
            // tool's circle.
            Shape::circle_stroke(g.p(8.0, 8.0), 4.5 * g.unit, stroke),
            Shape::line_segment([g.p(3.0, 13.0), g.p(13.0, 3.0)], stroke),
        ],
        Icon::Normalize => vec![
            Shape::line_segment([g.p(2.5, 3.0), g.p(13.5, 3.0)], stroke),
            Shape::line_segment([g.p(8.0, 13.5), g.p(8.0, 6.5)], stroke),
            Shape::convex_polygon(
                vec![g.p(8.0, 4.6), g.p(4.8, 8.4), g.p(11.2, 8.4)],
                color,
                none,
            ),
        ],
        Icon::Window => vec![
            Shape::rect_stroke(
                g.rect(1.5, 2.5, 14.5, 13.5),
                2.0 * g.unit,
                stroke,
                egui::StrokeKind::Middle,
            ),
            Shape::line_segment([g.p(1.5, 6.0), g.p(14.5, 6.0)], stroke),
        ],
        Icon::Follow => vec![
            Shape::line_segment([g.p(4.0, 2.5), g.p(4.0, 13.5)], stroke),
            Shape::line_segment([g.p(6.5, 8.0), g.p(11.5, 8.0)], stroke),
            Shape::convex_polygon(
                vec![g.p(14.0, 8.0), g.p(10.5, 5.0), g.p(10.5, 11.0)],
                color,
                none,
            ),
        ],
    }
}
