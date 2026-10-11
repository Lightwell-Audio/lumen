//! Drawing the editor (M5 slice 3c): the studio-keys look (owner's A). This
//! file is the **drawing boundary**: the interaction code calls only these
//! functions, with a [`Scene`] of plain data, so the GPU note renderer of
//! slice 3b can replace [`paint_body`]'s notes and lanes without touching
//! the input code. The preview is separate from the committed notes: it is
//! drawn over them, and the document is never changed to show it.

use eframe::egui::{Align2, Color32, FontId, Painter, Rect, Stroke, StrokeKind, pos2, vec2};
use lumen_core::grid::{Level, Line};
use lumen_core::meter::MeterMap;
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use super::gesture::Preview;
use super::view::{EditorView, is_black, pitch_name};
use crate::theme;

/// The most loop repeats drawn beside the first.
const MAX_REPEATS: u32 = 16;

/// Everything [`paint_body`] needs, as data.
pub(crate) struct Scene<'a> {
    pub notes: &'a [Note],
    /// Indices of the picked notes.
    pub selected: &'a [usize],
    /// The gesture's notes, drawn over the clip.
    pub preview: &'a Preview,
    /// The note colour: the clip's.
    pub colour: Color32,
    /// How long the clip is, and how long its content is (they differ when
    /// it loops).
    pub clip_length: Ticks,
    pub content_length: Ticks,
    /// The grid lines in view, arrangement times.
    pub lines: &'a [Line],
    /// The note under the pointer.
    pub hover: Option<usize>,
}

/// The lanes, grid, clip bounds, notes and preview, inside `view.body`.
pub(crate) fn paint_body(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let palette = theme::p();
    let body = view.body;
    let painter = painter.with_clip_rect(body);
    painter.rect_filled(body, 0.0, palette.canvas);

    // One lane per pitch: black-key lanes a step darker, a stronger line
    // above each C.
    let (high, low) = view.visible_pitches();
    for pitch in (low..=high).rev() {
        let top = view.row_top(pitch);
        let row = Rect::from_min_max(
            pos2(body.left(), top),
            pos2(body.right(), top + view.row_height),
        );
        if is_black(pitch) {
            painter.rect_filled(row, 0.0, palette.well);
        }
        let line = if pitch % 12 == 0 {
            palette.border_strong
        } else {
            palette.border
        };
        painter.hline(
            body.x_range(),
            top + view.row_height,
            Stroke::new(1.0, line),
        );
    }

    // Time outside the clip is dimmed; inside it, the grid.
    let clip_left = view.x_of(Ticks::ZERO);
    let clip_right = view.x_of(scene.clip_length);
    let dim = palette.app.gamma_multiply(0.55);
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
    for line in scene.lines {
        let x = view.time.x(body.left(), line.at.as_quarters());
        let colour = match line.level {
            Level::Bar => palette.grid_bar,
            Level::Beat => palette.grid_beat,
            Level::Sub => palette.grid_sub,
        };
        painter.vline(x, body.y_range(), Stroke::new(1.0, colour));
    }

    // Loop repeats, faint, then the notes of the clip itself.
    let repeats = if scene.content_length.0 > 0 {
        let n = (scene.clip_length.0 + scene.content_length.0 - 1) / scene.content_length.0;
        u32::try_from(n).unwrap_or(1).clamp(1, MAX_REPEATS)
    } else {
        1
    };
    for r in 1..repeats {
        let offset = Ticks(scene.content_length.0 * i64::from(r));
        let mut shifted = *view;
        shifted.clip_start = view.clip_start + offset;
        let limit = scene.clip_length - offset;
        for note in scene.notes {
            if note.start() < limit && shifted.shows(*note) {
                paint_note(&painter, &shifted, *note, scene.colour, Look::Repeat);
            }
        }
    }
    let hidden = &scene.preview.hidden;
    for (i, note) in scene.notes.iter().enumerate() {
        if !view.shows(*note) {
            continue;
        }
        let look = if hidden.binary_search(&i).is_ok() {
            Look::Moved
        } else if scene.selected.binary_search(&i).is_ok() {
            Look::Selected
        } else if scene.hover == Some(i) {
            Look::Hover
        } else {
            Look::Plain
        };
        paint_note(&painter, view, *note, scene.colour, look);
    }

    // The preview, over everything.
    for ghost in &scene.preview.ghosts {
        paint_note(&painter, view, *ghost, scene.colour, Look::Ghost);
    }
    if let Some(rect) = scene.preview.marquee {
        painter.rect(
            rect,
            theme::radius::SMALL,
            palette.accent.gamma_multiply(0.12),
            Stroke::new(1.0, palette.accent),
            StrokeKind::Inside,
        );
    }
}

/// How a note is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Plain,
    Hover,
    Selected,
    /// Where a note was, while its move is previewed.
    Moved,
    /// The preview of a gesture.
    Ghost,
    /// A note of a loop repeat.
    Repeat,
}

fn paint_note(painter: &Painter, view: &EditorView, note: Note, colour: Color32, look: Look) {
    let palette = theme::p();
    let rect = view.note_rect(note).shrink2(vec2(0.0, 0.5));
    let loudness = 0.55 + 0.45 * f32::from(note.velocity()) / 127.0;
    let mut fill = if note.muted() {
        theme::CLIP_MUTED
    } else {
        colour.gamma_multiply(loudness)
    };
    let mut stroke = Stroke::new(1.0, colour.gamma_multiply(0.45));
    match look {
        Look::Plain => {}
        Look::Hover => fill = fill.lerp_to_gamma(Color32::WHITE, 0.15),
        Look::Selected => {
            fill = fill.lerp_to_gamma(Color32::WHITE, 0.35);
            stroke = Stroke::new(1.5, palette.accent);
        }
        Look::Moved => {
            fill = fill.gamma_multiply(0.25);
            stroke = Stroke::new(1.0, palette.border_strong);
        }
        Look::Ghost => {
            fill = fill.lerp_to_gamma(Color32::WHITE, 0.25).gamma_multiply(0.8);
            stroke = Stroke::new(1.5, palette.accent);
        }
        Look::Repeat => {
            fill = fill.gamma_multiply(0.3);
            stroke = Stroke::NONE;
        }
    }
    painter.rect(rect, 2.0, fill, stroke, StrokeKind::Inside);
    if matches!(
        look,
        Look::Plain | Look::Hover | Look::Selected | Look::Ghost
    ) && rect.width() >= 30.0
        && view.row_height >= 12.0
    {
        painter.text(
            rect.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            pitch_name(i32::from(note.pitch())),
            FontId::proportional(theme::size::SCALE),
            theme::text_on(fill),
        );
    }
}

/// The key column: white keys, black keys shorter, the C's named. `lit` is
/// the pitch under the pointer.
pub(crate) fn paint_keys(painter: &Painter, view: &EditorView, column: Rect, lit: Option<i32>) {
    let palette = theme::p();
    let painter = painter.with_clip_rect(column);
    painter.rect_filled(column, 0.0, palette.raised);
    let (high, low) = view.visible_pitches();
    let black_width = column.width() * 0.62;
    for pitch in (low..=high).rev() {
        let top = view.row_top(pitch);
        let row = Rect::from_min_max(
            pos2(column.left(), top),
            pos2(column.right(), top + view.row_height),
        );
        let white = theme::CLIP_TEXT.gamma_multiply(0.92);
        painter.rect_filled(row.shrink2(vec2(0.0, 0.25)), 0.0, white);
        if is_black(pitch) {
            let key = Rect::from_min_size(row.min, vec2(black_width, row.height()));
            painter.rect_filled(key.shrink2(vec2(0.0, 0.5)), 2.0, theme::TEXT_ON_LIGHT);
        } else {
            painter.hline(
                column.x_range(),
                top + view.row_height,
                Stroke::new(1.0, palette.border_strong),
            );
        }
        if lit == Some(pitch) {
            painter.rect_filled(row, 0.0, palette.accent.gamma_multiply(0.45));
        }
        if pitch % 12 == 0 && view.row_height >= 9.0 {
            painter.text(
                pos2(column.right() - 4.0, row.center().y),
                Align2::RIGHT_CENTER,
                pitch_name(pitch),
                FontId::proportional(theme::size::SCALE),
                theme::TEXT_ON_LIGHT,
            );
        }
    }
    painter.vline(
        column.right(),
        column.y_range(),
        Stroke::new(1.0, palette.border_strong),
    );
}

/// The time ruler over the notes: bar numbers and beat ticks, the clip's
/// span underlined, the playhead.
pub(crate) fn paint_ruler(
    painter: &Painter,
    view: &EditorView,
    strip: Rect,
    lines: &[Line],
    meter: &MeterMap,
    clip_length: Ticks,
) {
    let palette = theme::p();
    let painter = painter.with_clip_rect(strip);
    painter.rect_filled(strip, 0.0, palette.raised);
    let (left, right) = (view.x_of(Ticks::ZERO), view.x_of(clip_length));
    painter.rect_filled(
        Rect::from_min_max(
            pos2(left.max(strip.left()), strip.bottom() - 3.0),
            pos2(right.min(strip.right()).max(left), strip.bottom()),
        ),
        0.0,
        palette.accent_muted,
    );
    let mut last_label = f32::NEG_INFINITY;
    for line in lines {
        let x = view.time.x(strip.left(), line.at.as_quarters());
        match line.level {
            Level::Bar => {
                painter.vline(
                    x,
                    strip.top() + 4.0..=strip.bottom(),
                    Stroke::new(1.0, palette.grid_bar),
                );
                if x - last_label > 28.0 {
                    last_label = x;
                    painter.text(
                        pos2(x + 3.0, strip.top() + 2.0),
                        Align2::LEFT_TOP,
                        format!("{}", meter.bbt_at(line.at).bar + 1),
                        FontId::proportional(theme::size::SCALE),
                        palette.text_secondary,
                    );
                }
            }
            Level::Beat => {
                painter.vline(
                    x,
                    strip.bottom() - 6.0..=strip.bottom(),
                    Stroke::new(1.0, palette.grid_beat),
                );
            }
            Level::Sub => {}
        }
    }
    painter.hline(
        strip.x_range(),
        strip.bottom(),
        Stroke::new(1.0, palette.border_strong),
    );
}
