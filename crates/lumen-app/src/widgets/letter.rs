use eframe::egui::{self, Align2, FontId, Id as EguiId, Rect, Sense, Stroke, StrokeKind, Ui};
use eframe::emath::GuiRounding as _;

use crate::widgets::raised;
use crate::{motion, theme};

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
