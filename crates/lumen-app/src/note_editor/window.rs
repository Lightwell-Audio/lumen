//! The editor in its own window (M5 slice 3c, owner's C2): an egui
//! viewport, a real OS window where the backend has them. Closing it docks
//! the editor back into the detail panel. Both views share one
//! [`super::NoteEditor`].

use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::App;
use crate::canvas::Surface;

/// The window's first size, in points.
const SIZE: egui::Vec2 = egui::vec2(960.0, 560.0);
/// The smallest it goes.
const MIN_SIZE: egui::Vec2 = egui::vec2(480.0, 280.0);

impl App {
    /// Shows the pop-out window while the editor is popped out. Call once
    /// per frame.
    pub(crate) fn note_editor_window(&mut self, ctx: &egui::Context) {
        if !self.note_editor.popped || self.note_editor.clip.is_none() {
            self.note_editor.popped = false;
            self.note_editor.window_focused = false;
            return;
        }
        let title = self
            .note_editor
            .clip
            .and_then(|id| self.editor_clip(id))
            .map_or_else(|| "MIDI clip".to_owned(), |(clip, _)| clip.name.clone());
        let builder = ViewportBuilder::default()
            .with_title(format!("{title} \u{2013} MIDI clip editor"))
            .with_inner_size(SIZE)
            .with_min_inner_size(MIN_SIZE);
        let id = ViewportId::from_hash_of("note editor window");
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == ViewportClass::EmbeddedWindow {
                // The window is as big as the editor needs to be.
                let (area, _) = ui.allocate_exact_size(SIZE, egui::Sense::hover());
                self.note_editor_surface(ui, area, Surface::Window);
                // An embedded window has no focus of its own to speak of:
                // the pointer in it is what makes its keys live.
                self.note_editor.window_focused = false;
            } else {
                // The window has its own keyboard: the editor's keys are
                // read here, as the main window's are in `App::ui`, the
                // computer keyboard's notes first so a letter plays instead
                // of running a command, and a release or the window losing
                // focus lets go of what sounds.
                let ctx = ui.ctx().clone();
                self.computer_keyboard_keys(&ctx);
                self.command_keys(&ctx);
                egui::CentralPanel::default().show(ui, |ui| {
                    let area = ui.max_rect();
                    self.note_editor_surface(ui, area, Surface::Window);
                });
                let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(false));
                self.note_editor.window_focused = focused;
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    self.note_editor.popped = false;
                }
            }
        });
    }
}
