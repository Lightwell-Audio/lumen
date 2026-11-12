//! While files are dragged in from the system, where the pointer is.

use eframe::egui;

use crate::App;

impl App {
    /// While files are dragged in from the system: where the pointer is
    /// (`App::drag_pointer`), and a frame after frame, so the line or the
    /// lit lane follows it: the system sends no events while it moves.
    pub(crate) fn follow_file_drag(&mut self, ctx: &egui::Context) {
        let (hovered, dropped) = ctx.input(|i| {
            (
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty(),
            )
        });
        self.drag_pointer = if hovered || dropped {
            self.system_cursor.or_else(|| {
                self.owner
                    .and_then(|window| window.cursor(ctx.pixels_per_point()))
            })
        } else {
            None
        };
        if hovered {
            ctx.request_repaint();
        }
    }
}
