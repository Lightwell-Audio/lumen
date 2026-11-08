//! The audio editor in its own window (M9.5 S2c, D25): the same viewport
//! code as the note editor's pop-out, with the same remembered place. The
//! clip's properties stay in the detail panel beside a note that the editor
//! is in its own window, with a Dock action. Closing the window docks the
//! editor back.

use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::App;
use crate::note_editor::window::{MIN_SIZE, SIZE, opening_place, place_of};

/// The pop-out window's viewport.
pub(super) fn viewport() -> ViewportId {
    ViewportId::from_hash_of("audio editor window")
}

/// Whether the keys being read are the popped-out editor's own window's,
/// where all of the keyboard is the editor's whatever the pointer is over.
pub(super) fn its_keyboard(popped: bool, reading: ViewportId) -> bool {
    popped && reading == viewport()
}

impl App {
    /// Shows the pop-out window while the audio editor is popped out. Call
    /// once per frame.
    pub(crate) fn audio_editor_window(&mut self, ctx: &egui::Context) {
        let clip = self.detail_audio_clip();
        let Some(id) = clip.filter(|_| self.detail.audio_popped) else {
            self.detail.audio_popped = false;
            self.audio_editor.window_at = None;
            return;
        };
        let opened_at = *self
            .audio_editor
            .window_at
            .get_or_insert_with(|| opening_place(self.settings.clip_editor_window));
        let title = self
            .shown_clip(id)
            .map_or_else(|| "Audio clip".to_owned(), |c| c.name);
        let builder = ViewportBuilder::default()
            .with_title(format!("{title} \u{2013} audio clip editor"))
            .with_inner_size(egui::vec2(opened_at.width, opened_at.height))
            .with_min_inner_size(MIN_SIZE);
        let builder = if opened_at.x.is_nan() {
            builder
        } else {
            builder.with_position(egui::pos2(opened_at.x, opened_at.y))
        };
        ctx.show_viewport_immediate(viewport(), builder, |ui, class| {
            if class == ViewportClass::EmbeddedWindow {
                let (area, _) = ui.allocate_exact_size(SIZE, egui::Sense::hover());
                self.draw_popped_audio_editor(ui, area, id);
            } else {
                // The window has its own keyboard, read here as the main
                // window's is in `App::ui`.
                let ctx = ui.ctx().clone();
                self.computer_keyboard_keys(&ctx);
                self.audio_handle_keys(&ctx);
                self.command_keys(&ctx);
                egui::CentralPanel::default().show(ui, |ui| {
                    let area = ui.max_rect();
                    self.draw_popped_audio_editor(ui, area, id);
                });
                // A drag here is this window's pointer, not the main one's:
                // the history keeps the gesture open while it is down.
                self.popped_pointer_down |= ctx.input(|i| i.pointer.any_down());
                let place = ui.ctx().input(|i| place_of(i.viewport()));
                if let Some(place) = place
                    && self.settings.clip_editor_window != Some(place)
                {
                    self.settings.clip_editor_window = Some(place);
                    self.settings_changed = Some(std::time::Instant::now());
                }
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    self.detail.audio_popped = false;
                }
            }
        });
    }

    /// The editor over the whole `area` of its window.
    fn draw_popped_audio_editor(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        id: lumen_project::model::Id,
    ) {
        // No properties column here: it stays in the panel, so the header
        // offers it in its menu as it does when the column is folded.
        self.audio_editor.folded = true;
        let mut ed = std::mem::take(&mut self.audio_editor);
        self.draw_audio_editor(ui, area, id, &mut ed);
        self.audio_editor = ed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_popped_editors_own_window_reads_its_keys_as_the_editors() {
        assert!(its_keyboard(true, viewport()));
        assert!(!its_keyboard(true, ViewportId::ROOT), "the main window");
        assert!(!its_keyboard(false, viewport()), "docked");
    }
}
