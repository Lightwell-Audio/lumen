//! A lettered toggle the height of a row (M8 D1's table, shared with the drum
//! editor's lanes by D2): a small square with one letter in it, lit in its
//! tone when it is on and a raised control when it is off.

use eframe::egui::{self, Align2, FontId, Id as EguiId, Rect, Sense, Stroke, StrokeKind, Ui};
use eframe::emath::GuiRounding as _;

use crate::widgets::raised;
use crate::{motion, theme};

/// A lettered toggle at `rect` (Mute, Solo): lit in `tone` when `on`, a raised
/// control when off. Named `name` for assistive technology and on hover. True
/// when it was clicked (or Space/Enter was pressed on it).
pub fn letter_toggle(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    letter: &str,
    on: bool,
    name: &str,
    tone: theme::Tone,
) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    let response = response.on_hover_text(name);
    // Space and Enter on the focused toggle are a click: egui sets the
    // keyboard's click on a focused clickable widget itself, so nothing extra
    // is needed here (and a mouse click does not take the keyboard, so Space
    // stays the transport's after one).
    if ui.is_rect_visible(rect) {
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let radius = theme::radius::for_height(rect.height());
        let face = raised::Face::new(theme::controls(on))
            .hover(motion::hover(
                ui.ctx(),
                id.with("hover"),
                response.hovered(),
            ))
            .lit(if on { 1.0 } else { 0.0 }, tone);
        let body = raised::paint(ui.painter(), rect, radius, &face);
        if !on {
            // A hairline edge, as the lanes' M and S draw it.
            ui.painter().rect_stroke(
                rect,
                f32::from(radius),
                Stroke::new(theme::stroke::HAIRLINE, theme::p().border_strong),
                StrokeKind::Inside,
            );
        }
        let galley = ui.painter().layout_no_wrap(
            letter.to_owned(),
            FontId::proportional(theme::size::SCALE),
            face.text(),
        );
        let at = Align2::CENTER_CENTER
            .anchor_size(body.center(), galley.size())
            .round_to_pixels(ui.pixels_per_point())
            .min;
        ui.painter().galley(at, galley, face.text());
        raised::focus_ring(ui.painter(), &response, rect);
    }
    response.clicked()
}
