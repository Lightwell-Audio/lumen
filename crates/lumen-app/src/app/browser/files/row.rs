//! One row of the file list as the app runs it (audit M12): what a click,
//! a double-click, a drag and the play button do, and the row's menu. How a
//! row looks is `crate::browser::files::row::paint_row`.

use std::path::Path;

use eframe::egui::{self, Rect, Sense, Ui};

use crate::browser::files::list::{Dragged, Out};
use crate::browser::files::row::{Drawn, RowLook, paint_row, play_rect, row_id};
use crate::browser::files::saved;
use crate::browser::files::walk::{Entry, Kind};
use crate::{App, theme, widgets};

impl App {
    /// One row: a sound (play, name, picture, length), a project, or a
    /// folder.
    pub(crate) fn file_row(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        entry: &Entry,
        look: &RowLook,
        out: &mut Out,
    ) {
        let ctx = ui.ctx().clone();
        let id = row_id(&entry.path);
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let thumb = self.browser.files.thumbs.get(&entry.path).cloned();
        let bad = matches!(thumb, Some(Err(_)));
        let said = match (&thumb, entry.kind) {
            (Some(Ok(t)), Kind::Audio) => {
                format!("{}, {}, {}", entry.name, entry.kind.said(), t.said())
            }
            (Some(Err(_)), _) => format!("{}, {}, cannot be read", entry.name, entry.kind.said()),
            _ if !look.there => format!("{}, place, not there", entry.name),
            _ => format!("{}, {}", entry.name, entry.kind.said()),
        };
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                look.selected,
                &said,
            )
        });
        // A sound or a project can be dragged onto the arrangement.
        if matches!(entry.kind, Kind::Audio | Kind::Midi) && !bad {
            response.dnd_set_drag_payload(Dragged(vec![entry.path.clone()]));
            if response.dragged() {
                crate::chain::paint_drag_ghost(&ctx, &entry.file_name());
            }
        }
        let playing = self
            .browser
            .files
            .preview
            .as_ref()
            .is_some_and(|p| p.is_playing(&entry.path));
        // The play button, before the name.
        let play_rect = play_rect(rect);
        let play = (entry.kind == Kind::Audio && !bad).then(|| {
            let r = ui.interact(play_rect, id.with("play"), Sense::click());
            r.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    if playing { "Stop preview" } else { "Preview" },
                )
            });
            r
        });
        if let Some(play) = &play
            && play.clicked()
        {
            out.preview = Some(entry.path.clone());
        } else if response.double_clicked() {
            out.open = Some(entry.clone());
        } else if response.clicked() {
            out.select = Some(entry.path.clone());
        }
        widgets::Menu::context(&response).show(|ui| self.row_menu(ui, entry, look, out));

        paint_row(
            ui,
            rect,
            entry,
            look,
            &Drawn {
                thumb: thumb.as_ref(),
                playing,
                response: &response,
                play: play.as_ref(),
            },
        );
        if response.has_focus() {
            widgets::raised::paint_focus(ui.painter(), rect, theme::radius::MEDIUM);
        }
        if response.hovered() && !look.there {
            response.on_hover_text("This place is not there: an unplugged drive, or a folder that moved. Right-click to find it or remove it.");
        }
    }

    /// A row's menu.
    fn row_menu(&mut self, ui: &mut Ui, entry: &Entry, look: &RowLook, out: &mut Out) {
        let computer = entry.path == Path::new(saved::COMPUTER);
        if look.place {
            if !look.there && widgets::menu_item(ui, "Find it\u{2026}", None, true).clicked() {
                out.find_place = Some(entry.path.clone());
                ui.close();
            }
            if let Some(place) = self
                .settings
                .browser
                .places
                .iter_mut()
                .find(|p| Path::new(&p.path) == entry.path)
            {
                widgets::menu_caption(ui, "Name");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut place.name)
                        .desired_width(160.0)
                        .hint_text("Rename"),
                );
                if field.changed() {
                    self.settings_changed = Some(std::time::Instant::now());
                }
            }
        }
        if !computer
            && look.there
            && widgets::menu_item(ui, "Reveal in the file manager", None, true).clicked()
        {
            out.reveal = Some(entry.path.clone());
            ui.close();
        }
        if entry.kind == Kind::Folder
            && !look.place
            && !computer
            && widgets::menu_item(ui, "Add as a place", None, true).clicked()
        {
            out.add_place = Some(entry.path.clone());
            ui.close();
        }
        if look.place && widgets::menu_item(ui, "Remove", None, true).clicked() {
            out.remove_place = Some(entry.path.clone());
            ui.close();
        }
        if entry.kind == Kind::Audio && widgets::menu_item(ui, "Preview", None, true).clicked() {
            out.preview = Some(entry.path.clone());
            ui.close();
        }
        if matches!(entry.kind, Kind::Audio | Kind::Midi)
            && widgets::menu_item(ui, "Add to the arrangement", None, true).clicked()
        {
            out.open = Some(entry.clone());
            ui.close();
        }
        if entry.kind == Kind::Project && widgets::menu_item(ui, "Open", None, true).clicked() {
            out.open = Some(entry.clone());
            ui.close();
        }
    }
}
