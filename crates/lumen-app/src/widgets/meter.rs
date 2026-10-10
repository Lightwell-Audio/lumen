//! Level meters (M4 slice 5, ADR-0040): the zones a level is drawn in, and
//! the strip's stereo meter with its scale. Colours come from
//! `theme::meter` (the owner's M1, with M2 as a setting); readings from
//! `crate::meters`.

use eframe::egui::{Align2, Color32, FontId, Painter, Rangef, Rect, Stroke};

use super::knob::position;
use crate::meters::{FLOOR_DB, Reading};
use crate::theme;

/// The loudest a meter shows.
pub const TOP_DB: f32 = 6.0;

/// The stretches of a bar from the floor up to `db`, each in one colour:
/// below the warning level, up to full scale, and over it. Each is
/// `(from, to, colour)`.
pub fn zones(db: f32, base: Color32) -> impl Iterator<Item = (f32, f32, Color32)> {
    let mut from = FLOOR_DB;
    [theme::METER_WARN_DB, 0.0, TOP_DB]
        .into_iter()
        .filter_map(move |to| {
            if db <= from {
                return None;
            }
            let top = db.min(to);
            let zone = (from, top, theme::meter((from + top) / 2.0, base));
            from = to;
            Some(zone)
        })
}

/// Where `db` sits on a vertical meter or fader spanning `ys`.
pub fn y_at(ys: Rangef, db: f32) -> f32 {
    ys.max - position(db) * ys.span()
}

/// A stereo meter: two bars, left then right, in `rect` with `gap`
/// between them; the peak faint behind the solid RMS, the hold line
/// across. `heard`: drawn quieter when the track is not heard.
pub fn vertical(
    painter: &Painter,
    rect: Rect,
    gap: f32,
    reading: &Reading,
    base: Color32,
    heard: bool,
) {
    let palette = theme::p();
    let width = (rect.width() - gap) / 2.0;
    let strength = if heard { 1.0 } else { 0.45 };
    for side in 0..2 {
        let left = rect.left() + side as f32 * (width + gap);
        let bar = Rect::from_x_y_ranges(left..=left + width, rect.y_range());
        painter.rect_filled(bar, 1.0, palette.well);
        for (db, alpha) in [
            (reading.peak[side], theme::METER_PEAK_ALPHA),
            (reading.rms[side], 1.0),
        ] {
            for (from, to, color) in zones(db, base) {
                let ys = Rangef::new(y_at(rect.y_range(), to), y_at(rect.y_range(), from));
                painter.rect_filled(
                    Rect::from_x_y_ranges(bar.x_range(), ys),
                    0.0,
                    color.gamma_multiply(alpha * strength),
                );
            }
        }
        let hold = reading.hold[side];
        if hold > FLOOR_DB {
            let y = y_at(rect.y_range(), hold).round();
            let color = if hold > 0.0 {
                palette.record
            } else {
                palette.text
            };
            painter.hline(bar.x_range(), y, Stroke::new(1.5, color));
        }
    }
}

/// The scale between a fader and its meter: dB marks right-aligned at `x`.
pub fn scale(painter: &Painter, x: f32, ys: Rangef, marks: &[f32]) {
    let palette = theme::p();
    for &db in marks {
        let text = if db == 0.0 {
            "0".to_owned()
        } else {
            format!("{}{}", if db > 0.0 { "+" } else { "\u{2212}" }, db.abs())
        };
        painter.text(
            egui::pos2(x, y_at(ys, db)),
            Align2::RIGHT_CENTER,
            text,
            FontId::monospace(theme::size::SCALE),
            if db == 0.0 {
                palette.text_secondary
            } else {
                palette.text_hint
            },
        );
    }
}

use eframe::egui;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_splits_into_its_zones() {
        let base = Color32::BLUE;
        let below: Vec<_> = zones(-20.0, base).collect();
        assert_eq!(below.len(), 1);
        assert_eq!((below[0].0, below[0].1), (FLOOR_DB, -20.0));
        let over: Vec<_> = zones(3.0, base).collect();
        assert_eq!(over.len(), 3);
        assert_eq!((over[1].0, over[1].1), (theme::METER_WARN_DB, 0.0));
        assert_eq!((over[2].0, over[2].1), (0.0, 3.0));
        assert_eq!(zones(f32::NEG_INFINITY, base).count(), 0);
    }
}
