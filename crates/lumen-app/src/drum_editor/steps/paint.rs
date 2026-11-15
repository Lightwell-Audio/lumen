use eframe::egui::{CornerRadius, Painter, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use lumen_project::edit::steps::Cell;

use super::geometry::Grid;
use crate::drum_editor::Lane;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Live {
    pub row: usize,
    pub column: usize,
    pub velocity: u8,
}

pub(crate) struct Scene<'a> {
    pub grid: Grid<'a>,
    pub lanes: &'a [Lane],

    pub cells: &'a [Cell],

    pub accent: u8,

    pub playing: Option<usize>,

    pub hover: Option<(usize, usize)>,

    pub cursor: Option<(usize, usize)>,

    pub picked: &'a [(usize, usize)],

    pub live: Option<Live>,
}

pub(crate) fn shown(scene: &Scene<'_>, row: usize, column: usize) -> Cell {
    let count = scene.grid.layout.count;
    let mut cell = scene
        .cells
        .get(row * count + column)
        .copied()
        .unwrap_or_default();
    if let Some(live) = scene.live
        && live.row == row
        && live.column == column
    {
        cell.on = true;
        cell.velocity = live.velocity;
    }
    cell
}

fn rounding(radius: u8) -> CornerRadius {
    CornerRadius::same(radius)
}

pub(crate) fn grid(painter: &Painter, scene: &Scene<'_>) {
    let g = &scene.grid;
    let painter = painter.with_clip_rect(g.body.intersect(painter.clip_rect()));
    let p = theme::p();
    let wells = if theme::is_light() {
        theme::alpha::STEP_WELL_LIGHT
    } else {
        theme::alpha::STEP_WELL_DARK
    };
    let (first_row, last_row) = g.rows_in_view();

    if let Some(column) = scene.playing {
        let x = g.body.left() + column as f32 * g.step_width();
        painter.rect_filled(
            Rect::from_min_max(
                pos2(x, g.body.top()),
                pos2(x + g.step_width(), g.body.bottom()),
            ),
            0.0,
            p.accent.gamma_multiply(theme::alpha::WASH),
        );
    }
    for row in first_row..last_row {
        let Some(lane) = scene.lanes.get(row) else {
            continue;
        };
        for column in 0..g.layout.count {
            let index = g.index_of(column);
            let rect = g.cell_rect(row, column);
            if rect.right() < g.body.left() || rect.left() > g.body.right() {
                continue;
            }
            let beat = (index.max(0) as usize / g.layout.group.max(1)) % 2;
            let past = g.past_end(index);
            let well = p.well.gamma_multiply(if past {
                theme::alpha::STEP_PAST_END
            } else {
                wells[beat]
            });
            painter.rect_filled(rect, rounding(theme::radius::SMALL), well);
            if past {
                continue;
            }
            let cell = shown(scene, row, column);
            cell_hit(&painter, rect, lane, &cell, scene.accent);
        }
    }

    if let Some(column) = scene.playing {
        let x = g.body.left() + column as f32 * g.step_width();
        let bar = Rect::from_min_size(
            pos2(x + 2.0, g.body.bottom() - theme::size::STEP_LIGHT),
            vec2((g.step_width() - 4.0).max(1.0), theme::size::STEP_LIGHT),
        );
        painter.rect_filled(
            bar,
            rounding((theme::size::STEP_LIGHT / 2.0).round() as u8),
            p.accent,
        );
    }
}

fn cell_hit(painter: &Painter, rect: Rect, lane: &Lane, cell: &Cell, accent: u8) {
    let p = theme::p();
    if !cell.on {
        if cell.held {
            painter.rect_stroke(
                rect,
                rounding(theme::radius::SMALL),
                Stroke::new(
                    theme::stroke::HAIRLINE,
                    lane.color.gamma_multiply(theme::alpha::STEP_EDGE_SILENT),
                ),
                StrokeKind::Inside,
            );
        }
        return;
    }
    let silent = lane.silent;
    let fill = if silent {
        lane.color.gamma_multiply(theme::alpha::STEP_FILL_SILENT)
    } else {
        lane.color
    };
    let edge = lane.color.gamma_multiply(if silent {
        theme::alpha::STEP_EDGE_SILENT
    } else {
        theme::alpha::STEP_EDGE_ON
    });
    let t = f32::from(cell.velocity.min(127)) / 127.0;
    let height = (rect.height() * t)
        .max(theme::size::STEP_FILL_LEAST)
        .min(rect.height());
    let filled = Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max);
    painter.rect_filled(
        filled,
        rounding(theme::radius::SMALL.min((height / 2.0) as u8)),
        fill,
    );
    painter.rect_stroke(
        rect,
        rounding(theme::radius::SMALL),
        Stroke::new(theme::stroke::HAIRLINE, edge),
        StrokeKind::Inside,
    );
    if cell.velocity >= accent {
        painter.rect_stroke(
            rect.expand(theme::size::STEP_RIM),
            rounding(theme::radius::MEDIUM),
            Stroke::new(theme::stroke::EDGE, p.warning),
            StrokeKind::Middle,
        );
    }
    if cell.off_grid {
        mark(painter, rect);
    }
}

fn mark(painter: &Painter, rect: Rect) {
    let p = theme::p();
    let size = theme::size::STEP_MARK.min(rect.width() / 2.0);
    let corner = rect.right_top();
    painter.add(Shape::convex_polygon(
        vec![
            pos2(corner.x - size, corner.y),
            corner,
            pos2(corner.x, corner.y + size),
        ],
        p.text,
        Stroke::NONE,
    ));
}

pub(crate) fn overlay(painter: &Painter, scene: &Scene<'_>) {
    let g = &scene.grid;
    let painter = painter.with_clip_rect(g.body.intersect(painter.clip_rect()));
    let p = theme::p();
    for &(row, column) in scene.picked {
        painter.rect_stroke(
            g.cell_rect(row, column),
            rounding(theme::radius::SMALL),
            Stroke::new(theme::stroke::LIT, p.accent),
            StrokeKind::Inside,
        );
    }
    if let Some((row, column)) = scene.hover
        && !g.past_end(g.index_of(column))
    {
        painter.rect_stroke(
            g.cell_rect(row, column),
            rounding(theme::radius::SMALL),
            Stroke::new(theme::stroke::HAIRLINE, p.text),
            StrokeKind::Inside,
        );
    }
    if let Some((row, column)) = scene.cursor {
        painter.rect_stroke(
            g.cell_rect(row, column),
            rounding(theme::radius::SMALL),
            Stroke::new(theme::stroke::LIT, p.accent),
            StrokeKind::Inside,
        );
    }
}

pub(crate) fn velocity_chip(velocity: u8, accent: u8) -> String {
    if velocity >= accent {
        format!("Velocity {velocity} (accent)")
    } else {
        format!("Velocity {velocity}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drum_editor::steps::{Layout, geometry::ROW};
    use lumen_core::time::{PPQ, Ticks};

    fn scene_with<'a>(
        pitches: &'a [u8],
        lanes: &'a [Lane],
        cells: &'a [Cell],
        live: Option<Live>,
    ) -> Scene<'a> {
        Scene {
            grid: Grid {
                body: Rect::from_min_max(pos2(0.0, 0.0), pos2(160.0, ROW * 2.0)),
                layout: Layout {
                    step: Ticks(PPQ / 4),
                    count: 8,
                    first: 0,
                    in_clip: 8,
                    pages: 1,
                    group: 4,
                },
                scroll: 0.0,
                pitches,
            },
            lanes,
            cells,
            accent: 127,
            playing: None,
            hover: None,
            cursor: None,
            picked: &[],
            live,
        }
    }

    #[test]
    fn a_drags_velocity_is_shown_over_the_stored_cell_without_touching_it() {
        let pitches = [36_u8, 38];
        let mut cells = vec![Cell::default(); 16];
        cells[3] = Cell {
            on: true,
            velocity: 60,
            ..Cell::default()
        };
        let live = Live {
            row: 0,
            column: 3,
            velocity: 110,
        };
        let scene = scene_with(&pitches, &[], &cells, Some(live));
        assert_eq!(shown(&scene, 0, 3).velocity, 110);
        assert!(shown(&scene, 0, 3).on);
        assert_eq!(cells[3].velocity, 60, "the stored cell is untouched");

        let off = Live {
            row: 1,
            column: 5,
            velocity: 40,
        };
        let scene = scene_with(&pitches, &[], &cells, Some(off));
        assert!(shown(&scene, 1, 5).on);
        assert!(!shown(&scene, 0, 5).on);
    }

    #[test]
    fn a_velocity_at_or_above_the_accent_is_called_an_accent() {
        assert_eq!(velocity_chip(100, 127), "Velocity 100");
        assert_eq!(velocity_chip(127, 127), "Velocity 127 (accent)");
        assert_eq!(velocity_chip(112, 112), "Velocity 112 (accent)");
    }
}
