//! A sunk well cut into pieces (ADR-0101): the M S ● well of a track
//! header, and the header corner's two wells, ◂ Set ▸ and grid, tool and
//! tempo lanes (the interface audit's H1). The well is `well` with its
//! `shade` lip; each piece sits `space::XXS` inside it, 1 pt from the next,
//! bare at rest, with the Ember wash under the pointer and filled while on.

use eframe::egui::{self, Color32, Painter, Rect, Response, Sense, Ui, pos2};
use eframe::emath::GuiRounding as _;

use super::raised;
use crate::{motion, theme};

/// A piece's corner: the well's radius less its inset.
pub const PIECE_RADIUS: u8 = theme::radius::SMALL - theme::space::XXS as u8;

/// The gap between two pieces.
pub const PIECE_GAP: f32 = 1.0;

/// How wide a well is that holds pieces `widths` wide.
pub fn width(widths: &[f32]) -> f32 {
    let gaps = widths.len().saturating_sub(1) as f32 * PIECE_GAP;
    2.0 * theme::space::XXS + widths.iter().sum::<f32>() + gaps
}

/// Paints the well in `rect` and returns its pieces, left to right, each as
/// wide as in `widths`; a width of 0 takes what the others leave.
pub fn paint(painter: &Painter, rect: Rect, widths: &[f32]) -> Vec<Rect> {
    raised::paint_sunken(painter, rect, theme::radius::SMALL);
    let inner = rect.shrink(theme::space::XXS);
    let fixed: f32 = widths.iter().sum();
    let flexible = widths.iter().filter(|w| **w <= 0.0).count().max(1) as f32;
    let gaps = widths.len().saturating_sub(1) as f32 * PIECE_GAP;
    let share = ((inner.width() - fixed - gaps) / flexible).max(0.0);
    let mut x = inner.left();
    widths
        .iter()
        .map(|&w| {
            let w = if w <= 0.0 { share } else { w };
            let piece = Rect::from_min_max(pos2(x, inner.top()), pos2(x + w, inner.bottom()));
            x += w + PIECE_GAP;
            piece
        })
        .collect()
}

/// Paints a piece in `rect`: bare while off, with the Ember wash under the
/// pointer, `look.press` while pressed, and filled in its tone by `lit` (a
/// lit M is `mute`, a lit S accent). Returns the colour for its letter.
pub fn paint_piece(painter: &Painter, rect: Rect, face: &raised::Face) -> Color32 {
    let palette = theme::p();
    let r = f32::from(PIECE_RADIUS);
    if face.pressed {
        painter.rect_filled(rect, r, face.surface.press);
    } else if face.hover > 0.0 {
        painter.rect_filled(rect, r, palette.ember.gamma_multiply(face.hover));
    }
    if face.lit > 0.0 {
        let on = theme::lit(face.tone);
        raised::gradient(
            painter,
            rect,
            r,
            on.top.gamma_multiply(face.lit),
            on.bottom.gamma_multiply(face.lit),
        );
        raised::lip(
            painter,
            rect,
            PIECE_RADIUS,
            on.light.gamma_multiply(face.lit),
        );
    }
    face.text()
}

/// A piece that is a button, in `rect`: lit by `lit` (0 to 1) in `tone`,
/// greyed when not `enabled`, with the focus ring on keyboard focus.
/// Returns its response and the colour for what it shows.
pub fn button(
    ui: &Ui,
    rect: Rect,
    id: egui::Id,
    (lit, tone): (f32, theme::Tone),
    enabled: bool,
) -> (Response, Color32) {
    let response = ui.interact(rect, id, Sense::click());
    let hover = motion::hover(ui.ctx(), id.with("hover"), enabled && response.hovered());
    let face = raised::Face::new(theme::controls(false))
        .hover(hover)
        .pressed(enabled && response.is_pointer_button_down_on())
        .lit(lit, tone)
        .enabled(enabled);
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let ink = paint_piece(ui.painter(), rect, &face);
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, PIECE_RADIUS);
    }
    (response, ink)
}

#[cfg(test)]
mod tests {
    use eframe::egui::vec2;

    use super::*;

    #[test]
    fn pieces_fill_the_well_and_the_flexible_one_takes_the_rest() {
        let widths = [30.0, 0.0, 24.0];
        let rect = Rect::from_min_size(pos2(10.0, 0.0), vec2(160.0, 20.0));
        let ctx = eframe::egui::Context::default();
        let painter = Painter::new(ctx, eframe::egui::LayerId::background(), Rect::EVERYTHING);
        let pieces = paint(&painter, rect, &widths);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].left(), rect.left() + theme::space::XXS);
        assert_eq!(pieces[2].right(), rect.right() - theme::space::XXS);
        assert_eq!(pieces[0].width(), 30.0);
        assert_eq!(pieces[1].left() - pieces[0].right(), PIECE_GAP);
        assert_eq!(pieces[1].width(), 160.0 - 4.0 - 54.0 - 2.0);
    }

    #[test]
    fn a_well_of_fixed_pieces_is_as_wide_as_they_need() {
        // M S ●: 60 pt; a return's M S: 41 pt.
        let piece = theme::size::WELL_PIECE_W;
        assert_eq!(width(&[piece; 3]), 60.0);
        assert_eq!(width(&[piece; 2]), 41.0);
    }
}
