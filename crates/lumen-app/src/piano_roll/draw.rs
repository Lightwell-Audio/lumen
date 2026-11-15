use std::sync::Arc;

use eframe::egui::{Align2, Color32, Painter, Rect, Stroke, pos2, vec2};
use lumen_render::BoxInstance;

use eframe::emath::GuiRounding as _;

use crate::canvas::{self, GridLook, NoteFrame, Surface};
use crate::timeview::TimeView;
use crate::{theme, widgets};

use super::PITCHES;

pub(crate) fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeySound {
    pub input: u8,
    pub name: String,
    pub icon: crate::design::icons::DrumGlyph,
    pub color: Color32,
    pub silent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rows {
    mapped: bool,
    listed: bool,
    pitches: [u8; PITCHES as usize],
    len: u8,
}

impl Default for Rows {
    fn default() -> Self {
        Self::all()
    }
}

impl Rows {
    pub fn all() -> Self {
        Self {
            mapped: false,
            listed: false,
            pitches: [0; PITCHES as usize],
            len: 0,
        }
    }

    pub fn mapped(sounds: &[KeySound]) -> Self {
        let mut pitches: Vec<u8> = sounds.iter().map(|sound| sound.input).collect();
        pitches.sort_unstable_by(|a, b| b.cmp(a));
        pitches.dedup();
        Self::of(pitches, false)
    }

    pub fn lanes(pitches: impl IntoIterator<Item = u8>) -> Self {
        let mut seen = [false; PITCHES as usize];
        let mut listed: Vec<u8> = Vec::new();
        for pitch in pitches {
            if let Some(slot) = seen.get_mut(usize::from(pitch))
                && !*slot
            {
                *slot = true;
                listed.push(pitch);
            }
        }
        if listed.is_empty() {
            return Self::none();
        }
        Self::of(listed, true)
    }

    pub fn none() -> Self {
        Self {
            mapped: true,
            listed: true,
            pitches: [0; PITCHES as usize],
            len: 0,
        }
    }

    fn of(pitches: Vec<u8>, listed: bool) -> Self {
        if pitches.is_empty() || pitches.len() >= PITCHES as usize {
            return Self::all();
        }
        let mut rows = Self {
            mapped: true,
            listed,
            pitches: [0; PITCHES as usize],
            len: pitches.len() as u8,
        };
        rows.pitches[..pitches.len()].copy_from_slice(&pitches);
        rows
    }

    pub fn len(&self) -> usize {
        if self.mapped {
            usize::from(self.len)
        } else {
            PITCHES as usize
        }
    }

    pub fn is_all(&self) -> bool {
        !self.mapped
    }

    pub fn row_of(&self, pitch: i32) -> Option<usize> {
        let pitch = u8::try_from(pitch).ok()?;
        if !self.mapped {
            return Some((PITCHES as i32 - 1 - i32::from(pitch)) as usize);
        }
        let listed = &self.pitches[..usize::from(self.len)];
        if self.listed {
            return listed.iter().position(|&p| p == pitch);
        }
        listed
            .binary_search_by(|probe| probe.cmp(&pitch).reverse())
            .ok()
    }

    pub fn pitch_of(&self, row: usize) -> i32 {
        if !self.mapped {
            return PITCHES as i32 - 1 - row as i32;
        }
        let listed = &self.pitches[..usize::from(self.len)];
        if listed.is_empty() {
            return 60;
        }
        let row = row.min(listed.len() - 1);
        i32::from(listed[row])
    }

    pub fn visible(&self, rect: Rect, scroll: f32, row_height: f32, ppp: f32) -> Vec<(i32, f32)> {
        let snap = |y: f32| (y * ppp).round() / ppp;
        let first = (scroll / row_height).floor().max(0.0) as usize;
        let last = (((scroll + rect.height()) / row_height).ceil() as usize).min(self.len());
        (first..last)
            .map(|row| {
                (
                    self.pitch_of(row),
                    snap(rect.top() + row as f32 * row_height - scroll),
                )
            })
            .collect()
    }
}

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

pub(crate) fn content_height(row_height: f32) -> f32 {
    PITCHES as f32 * row_height
}

#[cfg(test)]
fn visible(rect: Rect, scroll: f32, row_height: f32, ppp: f32) -> Vec<(u8, f32)> {
    Rows::all()
        .visible(rect, scroll, row_height, ppp)
        .into_iter()
        .map(|(pitch, top)| (pitch as u8, top))
        .collect()
}

pub(crate) fn split_on_top(pitch: u8) -> bool {
    matches!(pitch % 12, 11 | 4)
}

pub(crate) fn lanes(
    painter: &Painter,
    rect: Rect,
    rows: Rows,
    scroll: f32,
    row_height: f32,
    sounds: Option<&[KeySound]>,
) {
    let p = theme::p();
    let ppp = painter.ctx().pixels_per_point();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, p.lane);
    for (index, (pitch, top)) in rows
        .visible(rect, scroll, row_height, ppp)
        .into_iter()
        .enumerate()
    {
        let bottom = top + row_height;
        let row = Rect::from_x_y_ranges(rect.x_range(), top..=bottom);
        if let Some(sounds) = sounds {
            if sounds.iter().any(|sound| i32::from(sound.input) == pitch) {
                if index % 2 == 1 {
                    painter.rect_filled(row, 0.0, p.lane_black);
                }
            } else {
                painter.rect_filled(row, 0.0, p.app.gamma_multiply(theme::alpha::INK_QUIET));
            }
            continue;
        }
        let pitch = pitch as u8;
        if is_black(pitch) {
            painter.rect_filled(row, 0.0, p.lane_black);
        }
        if split_on_top(pitch) {
            painter.hline(
                rect.x_range(),
                top,
                Stroke::new(theme::stroke::HAIRLINE, p.lane_split),
            );
        }
    }
}

pub(crate) const BLACK_KEY: f32 = 0.62;
const KEY_SHADOW_RADIUS: f32 = 1.5;

pub(crate) fn keys(
    painter: &Painter,
    rect: Rect,
    rows: Rows,
    scroll: f32,
    row_height: f32,
    sounds: Option<&[KeySound]>,
) {
    let Some(sounds) = sounds else {
        keys_piano(painter, rect, rows, scroll, row_height);
        return;
    };
    let p = theme::p();
    let ppp = painter.ctx().pixels_per_point();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 0.0, p.header);
    for (pitch, top) in rows.visible(rect, scroll, row_height, ppp) {
        let row = Rect::from_x_y_ranges(rect.x_range(), top..=top + row_height);
        let Some(sound) = sounds.iter().find(|s| i32::from(s.input) == pitch) else {
            painter.rect_filled(row, 0.0, p.app.gamma_multiply(theme::alpha::INK_DIM));
            continue;
        };
        painter.rect_filled(
            Rect::from_min_max(row.min, pos2(rect.left() + KEY_BAR, row.bottom())),
            0.0,
            sound.color,
        );
        let ink = if sound.silent { p.text_hint } else { p.text };
        let glyph = row.height().min(GLYPH_FIT) - 2.0;
        if glyph >= 8.0 {
            let at = Rect::from_center_size(
                pos2(
                    rect.left() + KEY_BAR + theme::space::S + glyph / 2.0,
                    row.center().y,
                ),
                vec2(glyph, glyph),
            )
            .round_to_pixels(ppp);
            crate::design::icons::paint_drum_glyph(&painter, at, sound.icon, ink);
        }
        let text_left = rect.left() + KEY_NAME;
        let text_right = if sound.silent {
            rect.right() - theme::space::L
        } else {
            rect.right() - theme::space::S
        };
        if row.height() >= 12.0 && text_right > text_left {
            let font = eframe::egui::FontId::proportional(theme::size::CAPTION);
            let text = widgets::fit(&painter, &sound.name, font.clone(), text_right - text_left);
            painter.text(
                pos2(text_left, row.center().y),
                Align2::LEFT_CENTER,
                text,
                font,
                ink,
            );
        }
        if sound.silent {
            painter.text(
                pos2(rect.right() - theme::space::S, row.center().y),
                Align2::RIGHT_CENTER,
                "M",
                eframe::egui::FontId::proportional(theme::size::SCALE),
                p.text_hint,
            );
        }
        painter.hline(
            rect.x_range(),
            row.bottom(),
            Stroke::new(theme::stroke::HAIRLINE, p.border),
        );
    }
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(theme::stroke::HAIRLINE, p.key_edge),
    );
}

const KEY_BAR: f32 = 3.0;
const KEY_NAME: f32 = 34.0;
const GLYPH_FIT: f32 = 18.0;

fn keys_piano(painter: &Painter, rect: Rect, rows: Rows, scroll: f32, row_height: f32) {
    let p = theme::p();
    let ppp = painter.ctx().pixels_per_point();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let ivory = (p.key_white.lerp_to_gamma(Color32::BLACK, 0.09), p.key_white);
    widgets::gradient(&painter, rect, ivory.0, ivory.1);
    let black_width = (rect.width() * BLACK_KEY).round();
    let divider = Stroke::new(
        theme::stroke::HAIRLINE,
        p.key_text.gamma_multiply(theme::alpha::EDGE),
    );
    let listed = rows.visible(rect, scroll, row_height, ppp);
    for &(pitch, top) in &listed {
        let pitch = pitch as u8;
        if is_black(pitch) {
            let middle = ((top + row_height / 2.0) * ppp).round() / ppp;
            painter.hline(rect.left() + black_width..=rect.right(), middle, divider);
        } else if split_on_top(pitch) {
            painter.hline(rect.x_range(), top, divider);
        }
    }
    for &(pitch, top) in &listed {
        let pitch = pitch as u8;
        let bottom = top + row_height;
        if is_black(pitch) {
            let key = Rect::from_min_max(
                pos2(rect.left(), top),
                pos2(rect.left() + black_width, bottom),
            );
            painter.rect_filled(
                key.translate(vec2(1.0, 1.0)).expand2(vec2(0.5, 0.0)),
                KEY_SHADOW_RADIUS,
                p.shade,
            );
            let face = (
                p.key_black.lerp_to_gamma(Color32::BLACK, 0.45),
                p.key_black.lerp_to_gamma(Color32::WHITE, 0.10),
            );
            widgets::gradient(&painter, key, face.0, face.1);
        }
        if pitch.is_multiple_of(12) && row_height >= 8.0 {
            painter.text(
                pos2(rect.right() - 4.0, top + row_height / 2.0),
                Align2::RIGHT_CENTER,
                pitch_name(pitch),
                eframe::egui::FontId::monospace(theme::size::SCALE),
                p.key_text,
            );
        }
    }
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(theme::stroke::HAIRLINE, p.key_edge),
    );
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RollView {
    pub time: TimeView,
    pub scroll: f32,
    pub row_height: f32,
    pub rows: Rows,
}

pub(crate) struct RollNotes<F> {
    pub boxes: Arc<Vec<BoxInstance>>,
    pub generation: u64,
    pub under: F,
}

pub(crate) fn roll(
    canvas: RollCanvas<'_>,
    view: RollView,
    grid: &GridLook,
    notes: RollNotes<impl FnOnce(&Painter)>,
    sounds: Option<&[KeySound]>,
) {
    let RollCanvas {
        painter,
        surface,
        rect,
        key_width,
    } = canvas;
    let ppp = painter.ctx().pixels_per_point();
    let split = rect.left() + key_width;
    let keys_rect = Rect::from_x_y_ranges(rect.left()..=split, rect.y_range());
    let lanes_rect = Rect::from_x_y_ranges(split..=rect.right(), rect.y_range());
    lanes(
        painter,
        lanes_rect,
        view.rows,
        view.scroll,
        view.row_height,
        sounds,
    );
    painter.add(canvas::roll_grid_shape(
        surface, lanes_rect, ppp, view.time, grid,
    ));
    (notes.under)(painter);
    painter.add(canvas::notes_shape(
        surface,
        lanes_rect,
        ppp,
        NoteFrame {
            boxes: notes.boxes,
            generation: notes.generation,
            time: view.time,
            scroll: view.scroll,
            row_height: view.row_height,
            style: canvas::note_style(ppp),
        },
    ));
    keys(
        painter,
        keys_rect,
        view.rows,
        view.scroll,
        view.row_height,
        sounds,
    );
}

#[derive(Clone, Copy)]
pub(crate) struct RollCanvas<'a> {
    pub painter: &'a Painter,
    pub surface: Surface,
    pub rect: Rect,
    pub key_width: f32,
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
    fn a_repeated_input_note_gets_one_lane_row() {
        let rows = Rows::lanes([42_u8, 36, 42, 38, 36]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.row_of(42), Some(0));
        assert_eq!(rows.row_of(36), Some(1));
        assert_eq!(rows.row_of(38), Some(2));
        assert_eq!(rows.pitch_of(0), 42);
        assert_eq!(rows.pitch_of(2), 38);
    }
    #[test]
    fn rows_run_from_the_highest_pitch_down() {
        let rect = Rect::from_min_max(pos2(0.0, 10.0), pos2(100.0, 50.0));
        let rows = visible(rect, 0.0, 10.0, 1.0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (127, 10.0));
        assert_eq!(rows[3], (124, 40.0));
        let rows = visible(rect, 15.0, 10.0, 1.0);
        assert_eq!(rows[0], (126, 5.0));
        assert_eq!(visible(rect, content_height(10.0), 10.0, 1.0).len(), 0);
    }

    #[test]
    fn white_key_splits_sit_on_the_top_edge_of_b_and_e() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 1000.0));
        let rows = visible(rect, 0.0, 10.0, 1.0);
        let top_of = |pitch: u8| rows.iter().find(|r| r.0 == pitch).unwrap().1;
        assert_eq!(top_of(71), top_of(72) + 10.0);
        assert_eq!(top_of(70) - top_of(71), 10.0);
        assert_eq!(top_of(64), top_of(65) + 10.0);
        assert!(split_on_top(71) && split_on_top(64));
        assert!(!split_on_top(72) && !split_on_top(65) && !split_on_top(70));
    }
}
