//! Meters on the track headers (M4 slice 5, the owner's H2, ADR-0040).
//!
//! An unfolded track's level runs along the bottom of its volume slider, on
//! the slider's own scale, so the level sits under the value that sets it:
//! the peak faint, the RMS solid, both sides, and a hold tick. A folded
//! track has no slider, so its level is a hairline along the header's
//! bottom edge. Either way, a clip lights a red mark at the right end that
//! stays until clicked (Alt+click clears every track's).

use eframe::egui::{self, Color32, CursorIcon, Id as EguiId, Painter, Rangef, Rect, Sense, Ui};
use lumen_project::model::Track;

use super::Arrangement;
use crate::app::info::{Held, Info, Mouse};
use crate::meters::{FLOOR_DB, Reading};
use crate::widgets::raised;
use crate::{light, theme};

/// The top of the volume slider's range (`SliderKind::Volume`): the meter
/// in it shares the slider's scale.
const TOP_DB: f32 = 6.0;
/// The two sides' bars inside the slider: 1 pt each, 0.5 pt apart, ending
/// 1.5 pt above its bottom. Thin and low, so the value text can sit exactly
/// centred (the owner's choice A, M9 R1: lifted text crowded the top edge).
const BAR: f32 = 1.0;
const BAR_INSET: f32 = 1.5;
const BAR_GAP: f32 = 0.5;
/// The folded hairline: 1.5 pt a side, 6 pt above the row's bottom (clear
/// of the resize edge).
const HAIRLINE: f32 = 1.5;
const HAIRLINE_ABOVE: f32 = 3.0;
/// The trough behind the hairline is too thin for more than a soft corner.
const HAIRLINE_RADIUS: u8 = 1;

/// Where `db` falls along `span`, on the volume slider's scale.
fn x_at(span: Rangef, db: f32) -> f32 {
    let t = (db.clamp(FLOOR_DB, TOP_DB) - FLOOR_DB) / (TOP_DB - FLOOR_DB);
    span.min + t * span.span()
}

/// One side's bar from the floor up to `db`, in its zones' colours.
fn bar(painter: &Painter, span: Rangef, ys: Rangef, db: f32, base: Color32, alpha: f32) {
    for (from, to, color) in crate::widgets::meter::zones(db, base) {
        let rect = Rect::from_x_y_ranges(x_at(span, from)..=x_at(span, to), ys);
        painter.rect_filled(rect, 0.0, color.gamma_multiply(alpha));
    }
}

/// Peak, RMS and hold for both sides, the sides stacked in `ys`.
fn draw(
    painter: &Painter,
    span: Rangef,
    ys: Rangef,
    thickness: f32,
    reading: &Reading,
    base: Color32,
) {
    for side in 0..2 {
        let top = ys.min + side as f32 * (thickness + BAR_GAP);
        let side_ys = Rangef::new(top, top + thickness);
        bar(
            painter,
            span,
            side_ys,
            reading.peak[side],
            base,
            theme::METER_PEAK_ALPHA,
        );
        bar(painter, span, side_ys, reading.rms[side], base, 1.0);
        // The head: the level is light rising (ADR-0043).
        let rms = reading.rms[side];
        if rms > FLOOR_DB {
            let color = theme::meter(rms, base);
            light::vline(painter, light::BEAM, x_at(span, rms), side_ys, color, 1.0);
        }
    }
    let hold = reading.hold_db();
    if hold > FLOOR_DB {
        let palette = theme::p();
        let color = if hold > 0.0 {
            palette.record
        } else {
            palette.text
        };
        let x = x_at(span, hold);
        painter.rect_filled(
            Rect::from_x_y_ranges(x - 0.75..=x + 0.75, ys.min - 1.0..=ys.max + 1.0),
            0.0,
            color,
        );
    }
}

impl Arrangement {
    /// The colour meters are drawn in below the warning level.
    fn meter_base(&self, track: &Track) -> Color32 {
        if self.track_colour_meters {
            theme::wave(super::color_of(track.color))
        } else {
            theme::p().accent
        }
    }

    /// An unfolded track: its level along the bottom of `slider`.
    pub(super) fn slider_meter(&mut self, ui: &mut Ui, slider: Rect, track: &Track, id: EguiId) {
        let reading = self.meters.reading(track.id.0);
        let painter = ui.painter_at(slider.shrink2(egui::vec2(1.0, 0.0)));
        let bottom = slider.bottom() - BAR_INSET;
        let ys = Rangef::new(bottom - 2.0 * BAR - BAR_GAP, bottom);
        draw(
            &painter,
            slider.x_range(),
            ys,
            BAR,
            &reading,
            self.meter_base(track),
        );
        if reading.clipped {
            let mark = Rect::from_x_y_ranges(
                slider.right() - 5.0..=slider.right() - 2.0,
                slider.top() + 3.0..=slider.bottom() - 3.0,
            );
            self.clip_mark(ui, mark, slider.right() - 10.0..=slider.right(), track, id);
        }
    }

    /// A folded track: its level as a hairline along the header's bottom,
    /// from `left` to `right`.
    pub(super) fn hairline_meter(
        &mut self,
        ui: &mut Ui,
        row: Rect,
        (left, right): (f32, f32),
        track: &Track,
        id: EguiId,
    ) {
        let reading = self.meters.reading(track.id.0);
        let bottom = row.bottom() - HAIRLINE_ABOVE;
        let ys = Rangef::new(bottom - 2.0 * HAIRLINE - BAR_GAP, bottom);
        let span = Rangef::new(left, right);
        let painter = ui.painter_at(Rect::from_x_y_ranges(span, ys.expand(1.0)));
        // The level runs in a sunken trough (the design system's `.hair`).
        raised::paint_sunken(&painter, Rect::from_x_y_ranges(span, ys), HAIRLINE_RADIUS);
        if !reading.at_rest() {
            draw(
                &painter,
                span,
                ys,
                HAIRLINE,
                &reading,
                self.meter_base(track),
            );
        }
        if reading.clipped {
            let mark = Rect::from_x_y_ranges(right - 4.0..=right, ys.expand(1.0));
            self.clip_mark(ui, mark, right - 10.0..=right, track, id);
        }
    }

    /// The red clip mark, lit until clicked: a click clears this track's,
    /// Alt+click every track's.
    fn clip_mark(
        &mut self,
        ui: &mut Ui,
        mark: Rect,
        hit: std::ops::RangeInclusive<f32>,
        track: &Track,
        id: EguiId,
    ) {
        let palette = theme::p();
        light::halo_around(ui.painter(), light::FLARE, mark, 1.0, palette.record);
        ui.painter().rect_filled(mark, 1.0, palette.record);
        let area = Rect::from_x_y_ranges(hit, mark.y_range().expand(3.0));
        let response = ui
            .interact(area, id.with("clip mark"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let name = format!("Clear the clip light of {}", track.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if response.hovered() {
            self.info = Some(
                Info::new(format!("{} clipped", track.name))
                    .on(Held::NONE, Mouse::Click, "clear its clip light")
                    .on(Held::ALT, Mouse::Click, "clear every clip light"),
            );
        }
        if response.clicked() {
            if ui.input(|i| i.modifiers.alt) {
                self.meters.clear_all_clips();
            } else {
                self.meters.clear_clip(track.id.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_shares_the_volume_sliders_scale() {
        let span = Rangef::new(0.0, 66.0);
        assert_eq!(x_at(span, -60.0), 0.0);
        assert_eq!(x_at(span, 0.0), 60.0);
        assert_eq!(x_at(span, 6.0), 66.0);
        // Out of range stays on the slider.
        assert_eq!(x_at(span, -90.0), 0.0);
        assert_eq!(x_at(span, 12.0), 66.0);
    }
}
