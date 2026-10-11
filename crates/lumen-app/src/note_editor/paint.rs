//! Drawing the editor's overlay (M5 slice 3c). The lanes, grid, notes and
//! key column are slice 3b's GPU roll (`piano_roll::draw::roll`); this file
//! is the egui-drawn part over it, and the **drawing boundary** the input
//! code calls: dimming outside the clip, the hovered note, the originals a
//! move leaves behind (dimmed), the gesture's preview, the marquee, the
//! lit key and the ruler. The preview is separate from the committed notes:
//! the document is never changed to show it.

use eframe::egui::{Align2, Color32, FontId, Painter, Rect, Stroke, StrokeKind, pos2, vec2};
use lumen_core::grid::{Level, Line};
use lumen_core::meter::MeterMap;
use lumen_core::time::Ticks;
use lumen_project::model::Note;

use super::gesture::Preview;
use super::view::{EditorView, pitch_name};
use crate::theme;

/// Everything [`paint_overlay`] needs, as data.
pub(crate) struct Scene<'a> {
    pub notes: &'a [Note],
    /// The gesture's notes, drawn over the clip.
    pub preview: &'a Preview,
    /// Notes a marquee would pick, outlined.
    pub boxed: &'a [usize],
    /// The note colour: the clip's.
    pub colour: Color32,
    /// How long the clip is.
    pub clip_length: Ticks,
    /// The note under the pointer.
    pub hover: Option<usize>,
}

/// What goes over the GPU roll inside `view.body`.
pub(crate) fn paint_overlay(painter: &Painter, view: &EditorView, scene: &Scene<'_>) {
    let palette = theme::p();
    let body = view.body;
    let painter = painter.with_clip_rect(body);

    // Time outside the clip is dimmed.
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
    // Notes whose move is previewed fade where they were.
    for &i in &scene.preview.hidden {
        if let Some(note) = scene.notes.get(i)
            && view.shows(*note)
        {
            painter.rect_filled(
                view.note_rect(*note),
                2.0,
                palette.canvas.gamma_multiply(0.7),
            );
        }
    }
    if let Some(note) = scene.hover.and_then(|i| scene.notes.get(i)) {
        painter.rect_filled(
            view.note_rect(*note),
            2.0,
            Color32::WHITE.gamma_multiply(0.18),
        );
    }
    for &i in scene.boxed {
        if let Some(note) = scene.notes.get(i)
            && view.shows(*note)
        {
            painter.rect_stroke(
                view.note_rect(*note),
                2.0,
                Stroke::new(1.5, palette.accent),
                StrokeKind::Inside,
            );
        }
    }
    for ghost in &scene.preview.ghosts {
        let rect = view.note_rect(*ghost).shrink2(vec2(0.0, 0.5));
        let fill = scene
            .colour
            .gamma_multiply(0.55 + 0.45 * f32::from(ghost.velocity()) / 127.0)
            .lerp_to_gamma(Color32::WHITE, 0.25)
            .gamma_multiply(0.85);
        painter.rect(
            rect,
            2.0,
            fill,
            Stroke::new(1.5, palette.accent),
            StrokeKind::Inside,
        );
        if rect.width() >= 30.0 && view.row_height >= 12.0 {
            painter.text(
                rect.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                pitch_name(i32::from(ghost.pitch())),
                FontId::proportional(theme::size::SCALE),
                theme::text_on(fill),
            );
        }
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

/// Lights the key of `pitch` in the key column `column`.
pub(crate) fn paint_lit_key(painter: &Painter, view: &EditorView, column: Rect, pitch: i32) {
    let top = view.row_top(pitch);
    let row = Rect::from_min_max(
        pos2(column.left(), top),
        pos2(column.right(), top + view.row_height),
    );
    painter
        .with_clip_rect(column)
        .rect_filled(row, 0.0, theme::p().accent.gamma_multiply(0.45));
}

/// The time ruler over the notes: bar numbers and beat ticks, the clip's
/// span underlined.
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
