//! The edge between the track headers and the lanes: drag to size them.

use eframe::egui::{self, Sense};

use crate::{App, arrangement, motion, widgets};

impl App {
    /// The edge between the track headers and the lanes (MT-MIX-3 run 1):
    /// drag to widen the headers, double-click to go back to the default
    /// width. It glows in the accent while hovered or dragged, as a track's
    /// bottom edge does. Not an edit: saved with the project's view.
    pub(super) fn header_divider(&mut self, ui: &mut egui::Ui, edge: egui::Rect, line: f32) {
        let id = ui.id().with("header divider");
        let response = ui
            .interact(edge, id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Resize the track headers")
        });
        if response.hovered() {
            use crate::app::info::{Held, Info, Mouse};
            self.arrangement.info = Some(
                Info::new("Track headers' edge")
                    .on(Held::NONE, Mouse::Drag, "widen or narrow")
                    .on(Held::NONE, Mouse::DoubleClick, "default width"),
            );
        }
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(id, self.arrangement.header_width()));
        }
        if response.dragged() {
            let start: f32 = ui
                .data(|d| d.get_temp(id))
                .unwrap_or(arrangement::HEADER_WIDTH);
            let moved = response.total_drag_delta().map_or(0.0, |d| d.x);
            self.arrangement.set_header_width(start + moved);
        }
        if response.double_clicked() {
            self.arrangement.set_header_width(arrangement::HEADER_WIDTH);
        }
        let glow = motion::hover(
            ui.ctx(),
            id.with("glow"),
            response.hovered() || response.dragged(),
        );
        if glow > 0.0 {
            widgets::draw_edge_glow_vertical(ui.painter(), line, edge.y_range(), glow);
        }
    }
}
