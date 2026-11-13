//! The drum glyph sheet for the shots harness (M8 D1): every glyph at the
//! sizes it is drawn at and in a few of the track colours, so the look can be
//! judged in both themes and at every UI scale. Not part of the interface.

use eframe::egui::{self, Align2, FontId, Rect, Sense, Ui, Vec2, pos2};

use crate::design::icons::{DrumGlyph, paint_drum_glyph};
use crate::design::{fonts, theme};

/// The sizes a drum glyph is drawn at: a lane's icon up to a track header's.
const GLYPH_SIZES: [f32; 4] = [12.0, 16.0, 20.0, 28.0];

/// The track colours the sheet's second half uses, as indices of
/// [`theme::CLIP_COLORS`]: a blue, a rose and a violet-blue.
const TRACK_COLOURS: [usize; 3] = [0, 4, 6];

/// The sheet's size: a window at 100% shows it whole.
const SHEET: Vec2 = Vec2::new(520.0, 780.0);

/// How wide the names' column is.
const NAME_COLUMN: f32 = 112.0;

/// How wide a size column is.
const COLUMN: f32 = 76.0;

/// How tall a row is: a 28 pt glyph with 3 pt above and below.
const ROW: f32 = 34.0;

/// Prepares a bare harness as the app prepares itself: the typefaces, and
/// the palette for `appearance` ("dark" or "light"). Call once, before the
/// first frame.
pub fn prepare_look(ctx: &egui::Context, appearance: &str) {
    fonts::install(ctx);
    theme::apply(ctx, theme::Appearance::from_key(appearance), 0.0);
}

/// Paints the sheet: the fifteen glyphs in a grid of their four sizes, then
/// three rows of every glyph in a track colour.
pub fn drum_icon_sheet(ui: &mut Ui) {
    let c = theme::p();
    let (rect, _) = ui.allocate_exact_size(SHEET, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, c.app);
    let left = rect.left() + theme::space::L;
    let mut y = rect.top() + theme::space::L;

    let title = painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Drum glyphs",
        fonts::display(theme::size::TITLE),
        c.text,
    );
    y = title.bottom() + theme::space::XS;
    let subtitle = painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Fifteen sounds on the 16 pt grid, at the sizes they are drawn at.",
        FontId::proportional(theme::size::CAPTION),
        c.text_secondary,
    );
    y = subtitle.bottom() + theme::space::M;

    // The size grid: a name column, then one column per size.
    let section = painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Every glyph at 12, 16, 20 and 28 pt",
        fonts::medium(theme::size::CAPTION),
        c.text,
    );
    y = section.bottom() + theme::space::S;
    let column_x = |index: usize| left + NAME_COLUMN + COLUMN * (index as f32 + 0.5);
    let mut captions = Rect::NOTHING;
    for (index, size) in GLYPH_SIZES.iter().enumerate() {
        captions = captions.union(painter.text(
            pos2(column_x(index), y),
            Align2::CENTER_TOP,
            format!("{size} pt"),
            FontId::monospace(theme::size::MONO_SMALL),
            c.text_hint,
        ));
    }
    y = captions.bottom() + theme::space::S;

    for glyph in DrumGlyph::ALL {
        painter.text(
            pos2(left, y + ROW / 2.0),
            Align2::LEFT_CENTER,
            glyph.name(),
            FontId::proportional(theme::size::CAPTION),
            c.text_secondary,
        );
        for (index, size) in GLYPH_SIZES.iter().enumerate() {
            let cell =
                Rect::from_center_size(pos2(column_x(index), y + ROW / 2.0), Vec2::splat(*size));
            paint_drum_glyph(painter, cell, glyph, c.text);
        }
        y += ROW;
    }

    // The same glyphs in a few track colours, at a lane's size.
    y += theme::space::M;
    let section = painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Every glyph in three track colours, at 16 pt",
        fonts::medium(theme::size::CAPTION),
        c.text,
    );
    y = section.bottom() + theme::space::S;
    let spacing = 24.0;
    for index in TRACK_COLOURS {
        let colour = theme::CLIP_COLORS[index];
        painter.text(
            pos2(left, y + ROW / 2.0),
            Align2::LEFT_CENTER,
            format!("Colour {}", index + 1),
            FontId::proportional(theme::size::CAPTION),
            c.text_secondary,
        );
        for (slot, glyph) in DrumGlyph::ALL.iter().enumerate() {
            let centre = pos2(
                left + NAME_COLUMN + 14.0 + spacing * slot as f32,
                y + ROW / 2.0,
            );
            paint_drum_glyph(
                painter,
                Rect::from_center_size(centre, Vec2::splat(16.0)),
                *glyph,
                colour,
            );
        }
        y += ROW;
    }
}
