//! The editor in its own window (M5 slice 3c, owner's C2): an egui
//! viewport, a real OS window where the backend has them. Closing it docks
//! the editor back into the detail panel. Both views share one
//! [`super::NoteEditor`].

use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::App;
use crate::canvas::Surface;
use crate::settings::WindowPlace;

/// The window's first size, in points.
pub(crate) const SIZE: egui::Vec2 = egui::vec2(960.0, 560.0);
/// The smallest it goes.
pub(crate) const MIN_SIZE: egui::Vec2 = egui::vec2(480.0, 280.0);

impl App {
    /// Shows the pop-out window while the editor is popped out. Call once
    /// per frame.
    pub(crate) fn note_editor_window(&mut self, ctx: &egui::Context) {
        if !self.note_editor.popped || self.note_editor.clip.is_none() {
            self.note_editor.popped = false;
            self.note_editor.window_focused = false;
            self.note_editor.window_opened_at = None;
            return;
        }
        // Where it was last (KNOWN_ISSUES row 127), taken once as it opens.
        let opened_at = *self
            .note_editor
            .window_opened_at
            .get_or_insert_with(|| opening_place(self.settings.clip_editor_window));
        let title = self
            .note_editor
            .clip
            .and_then(|id| self.editor_clip(id))
            .map_or_else(|| "MIDI clip".to_owned(), |(clip, _)| clip.name.clone());
        let builder = ViewportBuilder::default()
            .with_title(format!("{title} \u{2013} MIDI clip editor"))
            .with_inner_size(egui::vec2(opened_at.width, opened_at.height))
            .with_min_inner_size(MIN_SIZE);
        let builder = if opened_at.x.is_nan() {
            builder
        } else {
            builder.with_position(egui::pos2(opened_at.x, opened_at.y))
        };
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
                // A drag here is this window's pointer, not the main one's:
                // the history keeps the gesture open while it is down.
                self.popped_pointer_down |= ctx.input(|i| i.pointer.any_down());
                let focused = ui.ctx().input(|i| i.viewport().focused.unwrap_or(false));
                self.note_editor.window_focused = focused;
                let place = ui.ctx().input(|i| place_of(i.viewport()));
                if let Some(place) = place
                    && self.settings.clip_editor_window != Some(place)
                {
                    self.settings.clip_editor_window = Some(place);
                    self.settings_changed = Some(std::time::Instant::now());
                }
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    self.note_editor.popped = false;
                }
            }
        });
    }
}

/// Where the window opens: its last place when there is one that is a
/// place, else the first size where the system puts it (`x` is NaN).
pub(crate) fn opening_place(saved: Option<WindowPlace>) -> WindowPlace {
    saved
        .filter(|p| {
            [p.x, p.y, p.width, p.height].iter().all(|v| v.is_finite())
                && p.width >= MIN_SIZE.x
                && p.height >= MIN_SIZE.y
                && p.width <= 16_384.0
                && p.height <= 16_384.0
        })
        .unwrap_or(WindowPlace {
            x: f32::NAN,
            y: f32::NAN,
            width: SIZE.x,
            height: SIZE.y,
        })
}

/// The place of a window as the system reports it, unless it is minimized
/// (a minimized window has no place worth keeping) or not known yet.
pub(crate) fn place_of(info: &egui::ViewportInfo) -> Option<WindowPlace> {
    if info.minimized == Some(true) {
        return None;
    }
    let (outer, inner) = (info.outer_rect?, info.inner_rect?);
    Some(WindowPlace {
        x: outer.min.x,
        y: outer.min.y,
        width: inner.width(),
        height: inner.height(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_opens_where_it_was_unless_that_is_no_place() {
        let kept = WindowPlace {
            x: 1920.0,
            y: 40.0,
            width: 1200.0,
            height: 700.0,
        };
        assert_eq!(opening_place(Some(kept)), kept);
        for odd in [
            WindowPlace {
                width: 10.0,
                ..kept
            },
            WindowPlace {
                x: f32::INFINITY,
                ..kept
            },
            WindowPlace {
                height: 1.0e6,
                ..kept
            },
        ] {
            let first = opening_place(Some(odd));
            assert!(first.x.is_nan(), "{odd:?}");
            assert_eq!((first.width, first.height), (SIZE.x, SIZE.y));
        }
        assert!(opening_place(None).x.is_nan());
    }

    #[test]
    fn a_minimized_or_unknown_window_has_no_place() {
        let mut info = egui::ViewportInfo {
            outer_rect: Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 50.0),
                egui::vec2(980.0, 600.0),
            )),
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(110.0, 80.0),
                egui::vec2(960.0, 560.0),
            )),
            ..Default::default()
        };
        assert_eq!(
            place_of(&info),
            Some(WindowPlace {
                x: 100.0,
                y: 50.0,
                width: 960.0,
                height: 560.0
            })
        );
        info.minimized = Some(true);
        assert_eq!(place_of(&info), None);
        info.minimized = None;
        info.inner_rect = None;
        assert_eq!(place_of(&info), None);
    }
}
