use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::App;
use crate::note_editor::window::{MIN_SIZE, SIZE, opening_place, place_of};

pub(super) fn viewport() -> ViewportId {
    ViewportId::from_hash_of("audio editor window")
}

pub(super) fn its_keyboard(popped: bool, reading: ViewportId) -> bool {
    popped && reading == viewport()
}

impl App {
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
                let ctx = ui.ctx().clone();
                self.computer_keyboard_keys(&ctx);
                self.audio_handle_keys(&ctx);
                self.command_keys(&ctx);
                egui::CentralPanel::default().show(ui, |ui| {
                    let area = ui.max_rect();
                    self.draw_popped_audio_editor(ui, area, id);
                });
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

    fn draw_popped_audio_editor(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        id: lumen_project::model::Id,
    ) {
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
