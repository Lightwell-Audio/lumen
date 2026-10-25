//! Meters on the track headers (M4 slice 5, ADR-0040; redrawn in M9 slice
//! R1b, ADR-0101).
//!
//! A track's level rises in a thin strip beside its colour rail, the full
//! height of the row less a margin at each end, folded or not: both sides,
//! the peak faint, the RMS solid with a light at its head, and a hold
//! tick, on the mixer strips' scale. A clip lights a red mark at the
//! strip's top that stays until clicked (Alt+click clears every track's).

use eframe::egui::{self, CursorIcon, Id as EguiId, Rect, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::Track;

use super::Arrangement;
use super::nesting::RAIL;
use crate::app::info::{Held, Info, Mouse};
use crate::widgets::raised;
use crate::{light, theme};

/// The strip sits this far right of the rail's left edge (1 pt clear of
/// the 4 pt rail), and this far inside the row's top and bottom.
const STRIP_X: f32 = RAIL + 1.0;
const STRIP_INSET: f32 = 5.0;
/// The two sides are 1 pt bars with 1 pt between them.
const SIDE_GAP: f32 = 1.0;
/// The clip mark at the strip's top.
const CLIP_MARK: f32 = 3.0;
/// The trough is too thin for more than a soft corner.
const STRIP_RADIUS: u8 = 1;

/// Where the level strip of a row in `row` goes, its rail starting at
/// `rail_left`.
pub(super) fn strip_rect(row: Rect, rail_left: f32) -> Rect {
    Rect::from_min_max(
        pos2(rail_left + STRIP_X, row.top() + STRIP_INSET),
        pos2(
            rail_left + STRIP_X + theme::size::LEVEL_STRIP,
            row.bottom() - STRIP_INSET,
        ),
    )
}

impl Arrangement {
    /// The colour meters are drawn in below the warning level.
    fn meter_base(&self, track: &Track) -> egui::Color32 {
        if self.track_colour_meters {
            theme::wave(super::color_of(track.color))
        } else {
            theme::p().accent
        }
    }

    /// The level strip of `track` beside its rail (which starts at
    /// `rail_left`) in `row`. `heard`: quieter when the track is not heard.
    pub(super) fn level_strip(
        &mut self,
        ui: &mut Ui,
        row: Rect,
        rail_left: f32,
        track: &Track,
        (id, heard): (EguiId, bool),
    ) {
        let strip = strip_rect(row, rail_left).round_to_pixels(ui.pixels_per_point());
        if strip.height() <= 2.0 * CLIP_MARK {
            return;
        }
        let reading = self.meters.reading(track.id.0);
        let painter = ui.painter_at(strip.expand(1.0));
        raised::paint_sunken(&painter, strip, STRIP_RADIUS);
        if !reading.at_rest() {
            crate::widgets::meter::vertical(
                &painter,
                strip,
                SIDE_GAP,
                &reading,
                self.meter_base(track),
                heard,
            );
        }
        if reading.clipped {
            let mark = Rect::from_min_size(strip.min, vec2(strip.width(), CLIP_MARK));
            self.clip_mark(ui, mark, track, id);
        }
    }

    /// The red clip mark, lit until clicked: a click clears this track's,
    /// Alt+click every track's.
    fn clip_mark(&mut self, ui: &mut Ui, mark: Rect, track: &Track, id: EguiId) {
        let palette = theme::p();
        light::halo_around(ui.painter(), light::FLARE, mark, 1.0, palette.record);
        ui.painter().rect_filled(mark, 1.0, palette.record);
        let area = mark.expand2(vec2(4.0, 3.0));
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
    fn the_strip_sits_beside_the_rail_inside_the_row() {
        let row = Rect::from_min_size(pos2(100.0, 50.0), vec2(204.0, 44.0));
        let strip = strip_rect(row, 112.0);
        // 1 pt clear of the 4 pt rail, 3 pt wide, 5 pt in at each end.
        assert_eq!(strip.left(), 117.0);
        assert_eq!(strip.width(), 3.0);
        assert_eq!(strip.top(), 55.0);
        assert_eq!(strip.bottom(), 89.0);
        // Content starts 12 pt after the rail: 4 pt clear of the strip.
        assert!(strip.right() + 4.0 <= 112.0 + super::super::headers::CONTENT_X);
    }
}
