//! The piano roll's lanes and key column (the owner's look A, "studio
//! keys"), drawn with egui's painter: only the pitches in view, a few
//! dozen rectangles. The grid and the notes are GPU layers
//! (`canvas::roll_grid_shape`, `canvas::notes_shape`).

use std::sync::Arc;

use eframe::egui::{Align2, Color32, Painter, Rect, Stroke, pos2, vec2};
use lumen_render::BoxInstance;

use eframe::emath::GuiRounding as _;

use crate::canvas::{self, GridLook, NoteFrame, Surface};
use crate::timeview::TimeView;
use crate::{theme, widgets};

use super::PITCHES;

/// Whether `pitch` is a black key.
pub(crate) fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// What the key column says about one of a drum map's sounds (M8 D1): its
/// drawing, its name and its colour. Built by the editor from the map the
/// clip's track reads.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KeySound {
    /// The note a clip stores for this sound.
    pub input: u8,
    pub name: String,
    pub icon: crate::design::icons::DrumGlyph,
    pub color: Color32,
    /// Muted in the map, or silenced by another sound's solo.
    pub silent: bool,
}

/// Which pitch rows the roll shows: all 128, a drum map's sounds only
/// ("Hide unmapped keys", M8 D1), or a drum map's sounds in the order the map
/// lists them (the drum editor's lanes, M8 D2). A mapped set lists the
/// sounds' input notes, highest first, so a folded roll's rows are its kit's,
/// packed together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rows {
    mapped: bool,
    /// Keep the order the list was given in (the lanes) rather than highest
    /// pitch first.
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
    /// The keyboard: every pitch from 127 down to 0.
    pub fn all() -> Self {
        Self {
            mapped: false,
            listed: false,
            pitches: [0; PITCHES as usize],
            len: 0,
        }
    }

    /// Only the input notes of `sounds`, highest first (a mapped track with
    /// Hide unmapped keys on). An empty list falls back to all 128 rows.
    pub fn mapped(sounds: &[KeySound]) -> Self {
        let mut pitches: Vec<u8> = sounds.iter().map(|sound| sound.input).collect();
        pitches.sort_unstable_by(|a, b| b.cmp(a));
        pitches.dedup();
        Self::of(pitches, false)
    }

    /// Only the input notes of `sounds`, in the order the list gives them
    /// (the drum editor's lanes: the map's order). An empty list is no rows at
    /// all: Hide unused with nothing used leaves the lanes empty.
    pub fn lanes(pitches: impl IntoIterator<Item = u8>) -> Self {
        let mut pitches: Vec<u8> = pitches.into_iter().collect();
        pitches.dedup();
        if pitches.is_empty() {
            return Self::none();
        }
        Self::of(pitches, true)
    }

    /// No rows at all (a drum map whose lanes are all hidden).
    pub fn none() -> Self {
        Self {
            mapped: true,
            listed: true,
            pitches: [0; PITCHES as usize],
            len: 0,
        }
    }

    /// A mapped row set of `pitches`, or the keyboard when there is none.
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

    /// The rows it lists (128 for the keyboard).
    pub fn len(&self) -> usize {
        if self.mapped {
            usize::from(self.len)
        } else {
            PITCHES as usize
        }
    }

    /// Whether it is the whole keyboard.
    pub fn is_all(&self) -> bool {
        !self.mapped
    }

    /// The row a pitch is drawn in, or `None` when it is hidden.
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

    /// The pitch of row `row`, or `None` past the end. A row above or below
    /// the list clamps to its ends, as the keyboard does.
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

    /// The rows whose pitches show in `rect` scrolled `scroll` points down,
    /// each with the top of its row.
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
/// each with the top of its row (the keyboard's rows: [`Rows::visible`]).
#[cfg(test)]
fn visible(rect: Rect, scroll: f32, row_height: f32, ppp: f32) -> Vec<(u8, f32)> {
    Rows::all()
        .visible(rect, scroll, row_height, ppp)
        .into_iter()
        .map(|(pitch, top)| (pitch as u8, top))
        .collect()
}

/// Whether the white keys meet on `pitch`'s top edge. Higher pitches sit
/// higher on screen, so B's top edge is B|C and E's is E|F, shared with the
/// C and F rows above.
pub(crate) fn split_on_top(pitch: u8) -> bool {
    matches!(pitch % 12, 11 | 4)
}

/// The lanes behind the notes: black-key pitches a little darker than
/// white-key ones, and faint lines where the white keys meet (B|C and
/// E|F). The grid's vertical lines go over them.
///
/// On a track with a drum map (`sounds`) the rows are the kit's, not a
/// keyboard's: a mapped row is a pad's lane, an alternate one reads a shade
/// darker, and the rows the map does not use are dimmed (M8 D1).
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
                // A key the map does not use: dimmed, so the kit's rows read
                // as the ones that matter.
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

/// How much of the key column's width a black key takes.
pub(crate) const BLACK_KEY: f32 = 0.62;
/// The corner radius of the shadow a black key casts (a 1 pt offset, so it
/// stays inside the key's own rounding).
const KEY_SHADOW_RADIUS: f32 = 1.5;

/// The key column, as a real keyboard: white keys in an ivory gradient
/// (darker at the left, as if lit from the right), the white keys' dividers
/// behind the black keys, and black keys that are shorter, darker and cast a
/// shadow. Only C is labelled ("C4"), in the mono face.
///
/// With a map (`sounds`) the column is the kit instead (M8 D1): a sound's
/// colour bar, its 16 pt drawing and its name at the note it answers to;
/// the keys the map does not use are dimmed, and a silent sound (muted, or
/// unsoloed while another is soloed) reads quiet with an M at the right.
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
            // A key the map does not use.
            painter.rect_filled(row, 0.0, p.app.gamma_multiply(theme::alpha::INK_DIM));
            continue;
        };
        // The bar down the row's left edge, in the sound's colour, as the
        // lanes draw it.
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

/// The colour bar down a mapped row, and where its name starts: the lanes'
/// name column (3 pt bar, 16 pt drawing, the name at 34) as M8's specimens
/// draw it.
const KEY_BAR: f32 = 3.0;
const KEY_NAME: f32 = 34.0;
/// The least room a mapped row's drawing takes.
const GLYPH_FIT: f32 = 18.0;

/// The key column of a track with no map: a keyboard (see [`keys`]).
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
    // White keys meet at B|C and E|F (a row's top edge) and behind each black
    // key (its row's middle).
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
            // The shadow a black key throws onto the white keys.
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
    // The edge against the lanes.
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(theme::stroke::HAIRLINE, p.key_edge),
    );
}

/// What the piano roll shows this frame. The caller keeps `time` shared
/// with the arrangement; `scroll` and `row_height` are the roll's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RollView {
    pub time: TimeView,
    /// Points scrolled down from the top of the first row.
    pub scroll: f32,
    /// Points per pitch row.
    pub row_height: f32,
    /// Which rows show: every pitch, or a drum map's sounds.
    pub rows: Rows,
}

/// The notes layer of [`roll`]: the shared boxes and their generation, and
/// what draws under them (a hook between the grid and the notes; the
/// notes themselves cast no shadow, as only windows and popovers do).
pub(crate) struct RollNotes<F> {
    pub boxes: Arc<Vec<BoxInstance>>,
    pub generation: u64,
    pub under: F,
}

/// Draws the whole roll in `rect`: lanes, the grid's vertical lines,
/// whatever `under` draws, the notes, and the key column (`key_width` wide,
/// on the left). `sounds` is the drum map the track reads, when it reads
/// one: the rows are then the kit's and the key column names them.
///
/// Per frame the caller supplies the view and the shared `boxes`; the
/// boxes only change with `generation` (the clip's content or the
/// selection), and are uploaded then, not per frame.
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

/// The surface the roll draws on: the painter, the layer kind, its area and
/// the key column's width.
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

    #[test]
    fn white_key_splits_sit_on_the_top_edge_of_b_and_e() {
        // Pitches 127 down to 28 at 10 points a row.
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 1000.0));
        let rows = visible(rect, 0.0, 10.0, 1.0);
        let top_of = |pitch: u8| rows.iter().find(|r| r.0 == pitch).unwrap().1;
        // C5 is one row above B4: C5's bottom edge is B4's top edge, the B|C
        // split. The old line sat on B4's bottom, which is B4|A#4.
        assert_eq!(top_of(71), top_of(72) + 10.0);
        assert_eq!(top_of(70) - top_of(71), 10.0);
        // F4 is one row above E4: the E|F split is E4's top edge.
        assert_eq!(top_of(64), top_of(65) + 10.0);
        assert!(split_on_top(71) && split_on_top(64));
        assert!(!split_on_top(72) && !split_on_top(65) && !split_on_top(70));
    }
}
