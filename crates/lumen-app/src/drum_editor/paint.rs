//! How the lanes look (M8 slice D2): each lane a row in the map's order, its
//! hits the specimen's diamonds (DE A) whose outline is the sound's colour and
//! whose fill runs from the foot up to the velocity, and the overlay the
//! pointer makes — the hovered hit, the picked ones, a box, a gesture's ghosts
//! and the time outside the clip.
//!
//! The drawing takes what the app computed (the lanes, the gathered cells,
//! the preview) and paints it; it never reads the document.

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

/// The most hits a gesture's ghosts or a lane's length bars draw in a frame.
const MAX_DRAWN: usize = 4000;
/// The ring around a picked hit, beyond its diamond.
const PICKED: f32 = theme::size::LANE_PICKED_RING;
/// How much of the picture column's width a hit at least fills, so a hit
/// stays a visible diamond at any zoom.
const LEAST_CELL: f32 = 2.5;
/// How much of a column a gathered cell fills.
const CELL_FILL: f32 = 0.9;
/// A hit this small is drawn as one filled shape (a packed picture), not a
/// fill inside an outline.
const SMALL_HIT: f32 = 2.0;

/// What the lanes draw this frame.
pub(crate) struct Scene<'a> {
    pub lanes: &'a [Lane],
    /// The clip's notes (the whole content: the editor shows what is there).
    pub notes: &'a [Note],
    /// The hits in view, one per picture column and lane.
    pub cells: &'a [Cell],
    /// The hit under the pointer.
    pub hover: Option<usize>,
    pub preview: &'a Preview,
    /// The notes a marquee would pick.
    pub boxed: &'a [usize],
    pub clip_length: Ticks,
    /// The grid step now, the width a hit's diamond is bounded by.
    pub step: Ticks,
    /// Whether a lane shows its sound's length.
    pub shows_length: &'a dyn Fn(u8) -> bool,
}

/// The lanes' rows behind the grid: the header's face, alternate rows a shade
/// darker, and the lanes of picked hits washed with the accent (the
/// specimen's picked row). Also the row separators.
pub(crate) fn background(
    painter: &Painter,
    view: &EditorView,
    lanes: &[Lane],
    picked_rows: &[bool],
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
        } else if row % 2 == 1 {
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

/// The hits: each a diamond with the sound's colour for its outline and its
/// velocity for the fill, gathered per picture column (so only what is in view
/// is drawn, however many notes the clip holds). A lane set to show length
/// draws a bar from the hit to the note's end.
pub(crate) fn hits(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let painter = painter.with_clip_rect(view.body.intersect(painter.clip_rect()));
    let p = theme::p();
    let column = cell_column_width(view) * CELL_FILL;
    for cell in scene.cells {
        let Some(lane) = scene.lanes.get(cell.row) else {
            continue;
        };
        let top = view.body.top() + cell.row as f32 * view.row_height - view.scroll;
        // The picture column decides the diamond's width when the zoom packs
        // hits: no wider than the column they share.
        let r = geometry::hit_radius(view, scene.step).min(column.max(LEAST_CELL));
        let centre = pos2(cell.x + r, top + view.row_height / 2.0);
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
            // A packed picture: one filled diamond, a shape not two.
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
    // A lane set to show length draws a bar from each gathered hit to its
    // note's end (never a pass over the clip's notes, H2).
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

/// The width of one picture column in `view`.
fn cell_column_width(view: &EditorView) -> f32 {
    geometry::bucket(view)
}

/// The overlay over the hits: the time outside the clip dimmed, the hovered
/// hit lit, the picked ones ringed, a box drawn, a gesture's ghosts and the
/// box itself.
pub(crate) fn overlay(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let p = theme::p();
    let body = view.body;
    let painter = painter.with_clip_rect(body);

    // Time outside the clip is dimmed, as in the roll.
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
    // The hits a gesture moves: where they were, faint.
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
    // A gesture's notes: a wash of accent inside a dashed outline, as a
    // ghost note is in the roll.
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

/// A hit's diamond at `centre`: `radius` from the middle to each tip, its
/// outline in `outline`, and filled from the foot up to `velocity` (0 to 127)
/// in the fill's colour.
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

/// The diamond's outline at `radius`, for a hover, a pick or a ghost.
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

/// The four tips of a diamond, clockwise from the top.
fn diamond_points(centre: Pos2, radius: f32) -> [Pos2; 4] {
    [
        pos2(centre.x, centre.y - radius),
        pos2(centre.x + radius, centre.y),
        pos2(centre.x, centre.y + radius),
        pos2(centre.x - radius, centre.y),
    ]
}

/// A convex polygon cut to the part at or below `y` (Sutherland–Hodgman
/// against one half-plane). Points that land on the line are one point, so a
/// cut at a vertex does not repeat it.
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

/// A lane's name at the left of `rect`: its colour bar, its sound's drawing,
/// its name, and its mute and solo toggles at the right. `picked` washes the
/// row as a selected row; `hovered` gives it the pointer's light; `silent`
/// quiets the name (muted, or unsoloed while another sound is soloed).
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

/// The lanes' empty states (M8 D2 review M6), as the Clip tab says them: a
/// map with no sounds, or Hide unused with nothing used (which offers the way
/// back). Returns the state and whether it offers to show every sound.
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
        // Review M6: a map with no sounds, or Hide unused hiding them all.
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
        // Nothing: the fill is above the diamond entirely.
        assert!(clip_below(&points, 111.0).is_empty());
        // Full: the whole diamond.
        assert_eq!(clip_below(&points, 89.0).len(), 4);
        // Halfway: a triangle from the waist to the bottom tip.
        let half = clip_below(&points, 100.0);
        assert_eq!(half.len(), 3);
        assert!(half.iter().all(|p| p.y >= 100.0));
        // A quarter: a smaller triangle, narrower at the waist.
        let quarter = clip_below(&points, 105.0);
        assert_eq!(quarter.len(), 3);
        let waist = quarter.iter().filter(|p| p.y == 105.0).count();
        assert_eq!(waist, 2);
    }
}
