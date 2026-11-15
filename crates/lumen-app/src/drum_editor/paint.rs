use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};
use eframe::emath::GuiRounding as _;
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use super::Lane;
use super::geometry::{self, Cell};
use crate::note_editor::gesture::Preview;
use crate::note_editor::view::EditorView;
use crate::widgets;
use crate::{fonts, theme};

const MAX_DRAWN: usize = 4000;

const PICKED: f32 = theme::size::LANE_PICKED_RING;

const LEAST_CELL: f32 = 2.5;

const CELL_FILL: f32 = 0.9;

const SMALL_HIT: f32 = 2.0;

pub(crate) struct Scene<'a> {
    pub lanes: &'a [Lane],

    pub notes: &'a [Note],

    pub cells: &'a [Cell],

    pub hover: Option<usize>,
    pub preview: &'a Preview,

    pub boxed: &'a [usize],
    pub clip_length: Ticks,

    pub step: Ticks,

    pub shows_length: &'a dyn Fn(u8) -> bool,
}

pub(crate) fn background(
    painter: &Painter,
    view: &EditorView,
    lanes: &[Lane],
    picked_rows: &[bool],
    banded: bool,
) {
    let p = theme::p();
    let painter = painter.with_clip_rect(view.body.intersect(painter.clip_rect()));
    painter.rect_filled(view.body, 0.0, p.lane);
    for row in 0..lanes.len() {
        let rect = geometry::row_rect(view, row).intersect(view.body);
        if !rect.is_positive() {
            continue;
        }
        let picked = picked_rows.get(row).copied().unwrap_or(false);
        let wash = if picked {
            p.accent.gamma_multiply(theme::alpha::WASH_FAINT)
        } else if banded && row % 2 == 1 {
            p.lane_black
        } else {
            Color32::TRANSPARENT
        };
        if wash.a() > 0 {
            painter.rect_filled(rect, 0.0, wash);
        }
        painter.hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(theme::stroke::HAIRLINE, p.border),
        );
    }
}

pub(crate) fn hits(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let painter = painter.with_clip_rect(view.body.intersect(painter.clip_rect()));
    let p = theme::p();
    let column = cell_column_width(view) * CELL_FILL;
    for cell in scene.cells {
        let Some(lane) = scene.lanes.get(cell.row) else {
            continue;
        };
        let top = view.body.top() + cell.row as f32 * view.row_height - view.scroll;

        let r = geometry::hit_radius(view, scene.step).min(column.max(LEAST_CELL));
        let centre = pos2(cell.x, top + view.row_height / 2.0);
        let colour = if cell.muted {
            p.text_secondary.gamma_multiply(theme::alpha::EDGE)
        } else {
            lane.color
        };
        let fill = if cell.muted {
            lane.color.gamma_multiply(theme::alpha::INK_TRACE)
        } else {
            lane.color
        };
        if r <= SMALL_HIT {
            let points = diamond_points(centre, r);
            painter.add(Shape::convex_polygon(points.to_vec(), fill, Stroke::NONE));
        } else {
            diamond(
                &painter,
                centre,
                r,
                colour,
                Some((fill, cell.velocity)),
                theme::stroke::RING,
            );
        }
        if cell.picked {
            diamond_outline(&painter, centre, r + PICKED, theme::stroke::LIT, p.accent);
        }
    }

    let mut barred = 0;
    for cell in scene.cells {
        if barred >= MAX_DRAWN {
            break;
        }
        let Some(note) = scene.notes.get(cell.note) else {
            continue;
        };
        if !(scene.shows_length)(note.pitch()) {
            continue;
        }
        let Some((from, to)) = geometry::length_bar(view, *note, scene.step) else {
            continue;
        };
        let Some(lane) = scene.lanes.get(cell.row) else {
            continue;
        };
        let top = view.body.top() + cell.row as f32 * view.row_height - view.scroll;
        let y = top + view.row_height / 2.0;
        barred += 1;
        painter.line_segment(
            [pos2(from, y), pos2(to, y)],
            Stroke::new(
                geometry::LENGTH_LINE,
                lane.color.gamma_multiply(theme::alpha::EDGE),
            ),
        );
    }
}

fn cell_column_width(view: &EditorView) -> f32 {
    geometry::bucket(view)
}

pub(crate) fn overlay(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let p = theme::p();
    let body = view.body;
    let painter = painter.with_clip_rect(body);

    let clip_left = view.x_of(Ticks::ZERO);
    let clip_right = view.x_of(scene.clip_length);
    let dim = p.app.gamma_multiply(theme::alpha::INK_QUIET);
    if clip_left > body.left() {
        painter.rect_filled(
            Rect::from_min_max(body.min, pos2(clip_left.min(body.right()), body.bottom())),
            0.0,
            dim,
        );
    }
    if clip_right < body.right() {
        painter.rect_filled(
            Rect::from_min_max(pos2(clip_right.max(body.left()), body.top()), body.max),
            0.0,
            dim,
        );
    }

    for &i in &scene.preview.hidden {
        if let Some(note) = scene.notes.get(i)
            && view.shows(*note)
        {
            diamond_outline(
                &painter,
                geometry::hit_centre(geometry::hit_rect(view, *note, scene.step)),
                geometry::hit_radius(view, scene.step) + PICKED,
                theme::stroke::HAIRLINE,
                p.text_hint,
            );
        }
    }
    if let Some(note) = scene.hover.and_then(|i| scene.notes.get(i)) {
        let rect = geometry::hit_rect(view, *note, scene.step);
        let centre = geometry::hit_centre(rect);
        let r = geometry::hit_radius(view, scene.step);
        diamond(
            &painter,
            centre,
            r + 1.0,
            p.text,
            Some((p.sheen, 127)),
            theme::stroke::LIT,
        );
    }
    for &i in scene.boxed {
        if let Some(note) = scene.notes.get(i)
            && view.shows(*note)
        {
            diamond_outline(
                &painter,
                geometry::hit_centre(geometry::hit_rect(view, *note, scene.step)),
                geometry::hit_radius(view, scene.step) + 1.0,
                theme::stroke::RING,
                p.accent,
            );
        }
    }

    for (n, ghost) in scene.preview.ghosts.iter().enumerate() {
        if n >= MAX_DRAWN {
            break;
        }
        let rect = geometry::hit_rect(view, *ghost, scene.step);
        let centre = geometry::hit_centre(rect);
        let r = geometry::hit_radius(view, scene.step);
        diamond(
            &painter,
            centre,
            r,
            p.accent,
            Some((
                p.accent.gamma_multiply(theme::alpha::WASH_SELECTED),
                ghost.velocity(),
            )),
            theme::stroke::LIT,
        );
    }
    if let Some(rect) = scene.preview.marquee {
        painter.rect(
            rect,
            theme::radius::SMALL,
            p.accent.gamma_multiply(theme::alpha::WASH_SELECTED),
            Stroke::new(theme::stroke::HAIRLINE, p.accent),
            eframe::egui::StrokeKind::Inside,
        );
    }
}

pub(crate) fn diamond(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    outline: Color32,
    fill: Option<(Color32, u8)>,
    width: f32,
) {
    let points = diamond_points(centre, radius);
    let backing = fill.map_or(Color32::TRANSPARENT, |(colour, _)| {
        colour.gamma_multiply(theme::alpha::WASH_OUTLINE)
    });
    painter.add(Shape::convex_polygon(
        points.to_vec(),
        backing,
        Stroke::new(width, outline),
    ));
    let Some((colour, velocity)) = fill else {
        return;
    };
    let t = f32::from(velocity.min(127)) / 127.0;
    let top = centre.y + radius - 2.0 * radius * t;
    let filled = clip_below(&points, top);
    if filled.len() >= 3 {
        painter.add(Shape::convex_polygon(filled, colour, Stroke::NONE));
    }
}

pub(crate) fn diamond_outline(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    width: f32,
    colour: Color32,
) {
    let points = diamond_points(centre, radius);
    painter.add(Shape::closed_line(
        points.to_vec(),
        Stroke::new(width, colour),
    ));
}

fn diamond_points(centre: Pos2, radius: f32) -> [Pos2; 4] {
    [
        pos2(centre.x, centre.y - radius),
        pos2(centre.x + radius, centre.y),
        pos2(centre.x, centre.y + radius),
        pos2(centre.x - radius, centre.y),
    ]
}

fn clip_below(points: &[Pos2], y: f32) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len() + 2);
    let mut push = |p: Pos2| {
        if out.last().is_none_or(|last| (*last - p).length() > 1e-3) {
            out.push(p);
        }
    };
    for (i, &current) in points.iter().enumerate() {
        let previous = points[(i + points.len() - 1) % points.len()];
        let in_current = current.y >= y;
        let in_previous = previous.y >= y;
        if in_current != in_previous {
            let t = (y - previous.y) / (current.y - previous.y);
            push(pos2(previous.x + (current.x - previous.x) * t, y));
        }
        if in_current {
            push(current);
        }
    }
    if out.len() > 1
        && (*out.first().expect("non-empty") - *out.last().expect("non-empty")).length() < 1e-3
    {
        out.pop();
    }
    out
}

pub(crate) fn lane_name(
    painter: &Painter,
    rect: Rect,
    lane: &Lane,
    picked: bool,
    hovered: bool,
    silent: bool,
    dragging: bool,
) {
    let p = theme::p();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let ppp = painter.ctx().pixels_per_point();
    painter.rect_filled(rect, 0.0, if picked { p.selected_row } else { p.header });
    if hovered && !picked {
        painter.rect_filled(rect, 0.0, p.ember);
    }
    painter.rect_filled(
        Rect::from_min_max(rect.min, pos2(rect.left() + geometry::BAR, rect.bottom())),
        0.0,
        lane.color,
    );
    let ink = if silent { p.text_hint } else { p.text };
    let icon = geometry::icon_rect(rect).round_to_pixels(ppp);
    if icon.height() >= 8.0 {
        crate::design::icons::paint_drum_glyph(
            &painter,
            icon,
            lane.icon,
            if lane.used {
                lane.color
            } else {
                p.text_secondary
            },
        );
    }
    let (mute, _solo) = geometry::buttons(rect);
    let text_left = rect.left() + geometry::NAME_X;
    let text_right = mute.left() - theme::space::XS;
    if text_right > text_left {
        let font = fonts::medium(theme::size::CAPTION);
        let text = widgets::fit(&painter, &lane.name, font.clone(), text_right - text_left);
        painter.text(
            pos2(text_left, rect.center().y),
            eframe::egui::Align2::LEFT_CENTER,
            text,
            font,
            ink,
        );
    }
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(theme::stroke::HAIRLINE, p.border),
    );
    if dragging {
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(theme::stroke::LIT, p.accent),
            eframe::egui::StrokeKind::Inside,
        );
    }
}

pub(crate) fn empty_state(sounds: usize) -> (widgets::EmptyState<'static>, bool) {
    if sounds == 0 {
        (
            widgets::EmptyState {
                icon: None,
                icon_size: theme::space::L,
                title: "No sounds in this map",
                hint: super::NO_SOUNDS,
                action: None,
            },
            false,
        )
    } else {
        (
            widgets::EmptyState {
                icon: None,
                icon_size: theme::space::L,
                title: "No lanes to show",
                hint: super::UNUSED,
                action: Some(widgets::EmptyAction {
                    label: "Show every sound",
                    keys: None,
                    tip: "Turn Hide unused off",
                }),
            },
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_empty_states_say_which_one_it_is_and_offer_the_way_back() {
        let (none, can_show) = empty_state(0);
        assert!(!can_show);
        assert_eq!(none.hint, super::super::NO_SOUNDS);
        assert!(none.action.is_none());
        let (unused, can_show) = empty_state(7);
        assert!(can_show, "Hide unused can be turned off");
        assert_eq!(unused.hint, super::super::UNUSED);
        assert_eq!(unused.action.expect("a way back").label, "Show every sound");
    }

    #[test]
    fn a_diamonds_fill_starts_at_the_foot_and_grows_to_the_velocity() {
        let centre = pos2(100.0, 100.0);
        let points = diamond_points(centre, 10.0);
        assert_eq!(points[0], pos2(100.0, 90.0));
        assert_eq!(points[1], pos2(110.0, 100.0));
        assert_eq!(points[2], pos2(100.0, 110.0));
        assert_eq!(points[3], pos2(90.0, 100.0));

        assert!(clip_below(&points, 111.0).is_empty());

        assert_eq!(clip_below(&points, 89.0).len(), 4);

        let half = clip_below(&points, 100.0);
        assert_eq!(half.len(), 3);
        assert!(half.iter().all(|p| p.y >= 100.0));

        let quarter = clip_below(&points, 105.0);
        assert_eq!(quarter.len(), 3);
        let waist = quarter.iter().filter(|p| p.y == 105.0).count();
        assert_eq!(waist, 2);
    }
}
