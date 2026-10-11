//! The piano roll's lanes and key column (the owner's look A, "studio
//! keys"), drawn with egui's painter: only the pitches in view, a few
//! dozen rectangles. The grid and the notes are GPU layers
//! (`canvas::roll_grid_shape`, `canvas::notes_shape`).

use std::sync::Arc;

use eframe::egui::{Align2, Color32, Painter, Rect, Stroke, pos2};
use lumen_render::BoxInstance;

use crate::canvas::{self, GridLook, NoteFrame, Surface};
use crate::theme;
use crate::timeview::TimeView;

use super::PITCHES;

/// Whether `pitch` is a black key.
pub(crate) fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// A pitch's name with its octave, C4 being MIDI note 60.
pub(crate) fn pitch_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i32::from(pitch / 12) - 1
    )
}

/// The height of all the pitch rows, the roll's scrollable content.
pub(crate) fn content_height(row_height: f32) -> f32 {
    PITCHES as f32 * row_height
}

/// The pitches whose rows show in `rect` scrolled `scroll` points down,
/// each with the top of its row.
fn visible(rect: Rect, scroll: f32, row_height: f32, ppp: f32) -> Vec<(u8, f32)> {
    let snap = |y: f32| (y * ppp).round() / ppp;
    let first = (scroll / row_height).floor().max(0.0) as u32;
    let last = (((scroll + rect.height()) / row_height).ceil() as u32).min(PITCHES);
    (first..last)
        .map(|row| {
            let pitch = (PITCHES - 1 - row) as u8;
            (pitch, snap(rect.top() + row as f32 * row_height - scroll))
        })
        .collect()
}

/// The lanes behind the notes: black-key pitches a little darker than
/// white-key ones, and faint lines where the white keys meet (B|C and
/// E|F). The grid's vertical lines go over them.
pub(crate) fn lanes(painter: &Painter, rect: Rect, scroll: f32, row_height: f32) {
    let p = theme::p();
    let ppp = painter.ctx().pixels_per_point();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, p.lane);
    for (pitch, top) in visible(rect, scroll, row_height, ppp) {
        let bottom = top + row_height;
        if is_black(pitch) {
            let row = Rect::from_x_y_ranges(rect.x_range(), top..=bottom);
            painter.rect_filled(row, 0.0, p.lane_black);
        }
        // The line sits on the bottom edge of B and of E: below them.
        if matches!(pitch % 12, 11 | 4) {
            painter.hline(rect.x_range(), bottom, Stroke::new(1.0, p.lane_split));
        }
    }
}

/// The key column: white keys light, black keys dark and shorter, and C
/// labelled ("C4") in the monospace face.
pub(crate) fn keys(painter: &Painter, rect: Rect, scroll: f32, row_height: f32) {
    let p = theme::p();
    let ppp = painter.ctx().pixels_per_point();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, p.key_white);
    let black_width = rect.width() * 0.62;
    for (pitch, top) in visible(rect, scroll, row_height, ppp) {
        let bottom = top + row_height;
        if is_black(pitch) {
            let key = Rect::from_min_max(
                pos2(rect.left(), top),
                pos2(rect.left() + black_width, bottom),
            );
            painter.rect_filled(key, 1.0, p.key_black);
        }
        if matches!(pitch % 12, 11 | 4) {
            painter.hline(
                rect.x_range(),
                bottom,
                Stroke::new(1.0, p.key_text.gamma_multiply(0.5)),
            );
        }
        if pitch % 12 == 0 && row_height >= 8.0 {
            painter.text(
                pos2(rect.right() - 4.0, top + row_height / 2.0),
                Align2::RIGHT_CENTER,
                pitch_name(pitch),
                eframe::egui::FontId::monospace(theme::size::SCALE),
                p.key_text,
            );
        }
    }
    // The edge against the lanes.
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, Color32::from_black_alpha(48)),
    );
}

/// What the piano roll shows this frame. The caller keeps `time` shared
/// with the arrangement; `scroll` and `row_height` are the roll's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RollView {
    pub time: TimeView,
    /// Points scrolled down from the top of pitch 127.
    pub scroll: f32,
    /// Points per pitch row.
    pub row_height: f32,
}

/// Draws the whole roll in `rect`: lanes, the grid's vertical lines, the
/// notes, and the key column (`key_width` wide, on the left).
///
/// Per frame the caller supplies the view and the shared `boxes`; the
/// boxes only change with `generation` (the clip's content or the
/// selection), and are uploaded then, not per frame.
pub(crate) fn roll(
    painter: &Painter,
    surface: Surface,
    rect: Rect,
    key_width: f32,
    view: RollView,
    grid: &GridLook,
    notes: (Arc<Vec<BoxInstance>>, u64),
) {
    let ppp = painter.ctx().pixels_per_point();
    let split = rect.left() + key_width;
    let keys_rect = Rect::from_x_y_ranges(rect.left()..=split, rect.y_range());
    let lanes_rect = Rect::from_x_y_ranges(split..=rect.right(), rect.y_range());
    lanes(painter, lanes_rect, view.scroll, view.row_height);
    painter.add(canvas::roll_grid_shape(
        surface, lanes_rect, ppp, view.time, grid,
    ));
    painter.add(canvas::notes_shape(
        surface,
        lanes_rect,
        ppp,
        NoteFrame {
            boxes: notes.0,
            generation: notes.1,
            time: view.time,
            scroll: view.scroll,
            row_height: view.row_height,
            style: canvas::note_style(ppp),
        },
    ));
    keys(painter, keys_rect, view.scroll, view.row_height);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_keys_and_names() {
        let black: Vec<u8> = (60..72).filter(|&p| is_black(p)).collect();
        assert_eq!(black, [61, 63, 66, 68, 70]);
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(61), "C#4");
        assert_eq!(pitch_name(0), "C-1");
        assert_eq!(pitch_name(127), "G9");
    }

    #[test]
    fn rows_run_from_the_highest_pitch_down() {
        let rect = Rect::from_min_max(pos2(0.0, 10.0), pos2(100.0, 50.0));
        let rows = visible(rect, 0.0, 10.0, 1.0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (127, 10.0));
        assert_eq!(rows[3], (124, 40.0));
        // Scrolled a row and a half: the first row is partly above.
        let rows = visible(rect, 15.0, 10.0, 1.0);
        assert_eq!(rows[0], (126, 5.0));
        assert_eq!(visible(rect, content_height(10.0), 10.0, 1.0).len(), 0);
    }
}
