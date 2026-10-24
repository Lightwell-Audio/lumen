//! The Frozen badge on a track header (M7b slice E5, the owner's FR2): a small
//! lit word after the name, shown only while the track is frozen. A click
//! unfreezes the track, so the way out of the read-only state is always at
//! hand. It is on (`Tone::On`, like the Monitor badge): a state the user
//! chose, not something sounding.

use eframe::egui::{self, Align2, Id as EguiId, Rect, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::Track;

use crate::theme::Tone;
use crate::widgets::raised;
use crate::{motion, theme, widgets};

const WORD: &str = "Frozen";
const PADDING: f32 = 6.0;

fn font() -> egui::FontId {
    egui::FontId::proportional(theme::size::CAPTION)
}

/// The width the badge takes, with the gap before it.
pub(super) fn room(ui: &Ui) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(WORD.into(), font(), theme::p().text);
    galley.size().x + 2.0 * PADDING + 4.0
}

/// Draws the badge at the right end of `row` (the name's rectangle); returns
/// whether it was clicked.
pub(super) fn badge(ui: &mut Ui, row: Rect, id: EguiId, track: &Track, selected: bool) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(WORD.into(), font(), theme::p().text);
    let size = vec2(
        galley.size().x + 2.0 * PADDING,
        theme::size::CONTROL_SMALL - 4.0,
    );
    let rect = Rect::from_min_size(
        pos2(row.right() - size.x, row.center().y - size.y / 2.0),
        size,
    );
    let response = ui.interact(rect, id.with("frozen"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{} is frozen: unfreeze", track.name),
        )
    });
    let response = widgets::tip(
        response,
        "Frozen \u{b7} plays a rendered file and runs no plugin; its clips, notes and devices are read-only",
        None,
        Some("Click to unfreeze"),
    );
    let hover = motion::hover(ui.ctx(), id.with("frozen hover"), response.hovered());
    let face = raised::Face::new(theme::controls(selected))
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
        .lit(1.0, Tone::On);
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    ui.painter().text(
        body.center(),
        Align2::CENTER_CENTER,
        WORD,
        font(),
        face.text(),
    );
    raised::focus_ring(ui.painter(), &response, rect);
    response.clicked()
}
