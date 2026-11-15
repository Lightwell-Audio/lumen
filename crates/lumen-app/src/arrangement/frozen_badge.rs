use eframe::egui::{self, Align2, Id as EguiId, Rect, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::Track;

use crate::icons::{self, Icon};
use crate::theme::Tone;
use crate::widgets::raised;
use crate::{motion, theme, widgets};

const WORD: &str = "Frozen";
const PADDING: f32 = 6.0;
const GAP: f32 = 4.0;
const NAME_MIN: f32 = 44.0;
const LOCK: f32 = 10.0;

fn font() -> egui::FontId {
    egui::FontId::proportional(theme::size::CAPTION)
}

fn width(ui: &Ui, available: f32) -> (f32, bool) {
    let word = ui
        .painter()
        .layout_no_wrap(WORD.into(), font(), theme::p().text)
        .size()
        .x
        + 2.0 * PADDING;
    if available - word - GAP >= NAME_MIN {
        (word, false)
    } else {
        (LOCK + 2.0 * PADDING, true)
    }
}

pub(super) fn room(ui: &Ui, available: f32) -> f32 {
    width(ui, available).0 + GAP
}

pub(super) fn badge(ui: &mut Ui, row: Rect, id: EguiId, track: &Track, selected: bool) -> bool {
    let (wide, compact) = width(ui, row.width());
    let size = vec2(wide, theme::size::CONTROL_SMALL - 4.0);
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
    if compact {
        let at = Rect::from_center_size(body.center(), vec2(LOCK, LOCK));
        ui.painter()
            .extend(icons::shapes(Icon::Lock, at, face.text()));
    } else {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            WORD,
            font(),
            face.text(),
        );
    }
    raised::focus_ring(ui.painter(), &response, rect);
    response.clicked()
}
