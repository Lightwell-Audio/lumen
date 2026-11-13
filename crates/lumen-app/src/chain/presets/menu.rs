//! The menu under a card's preset field: the search, the rows, and Save, Save
//! as and Show presets folder.

use super::state::{Choice, Focus, MenuState};
use super::*;

/// Where the field of the card of `device` is: its id, for opening the menu
/// from a key.
pub(in crate::chain) fn field_id(device: Id) -> egui::Id {
    egui::Id::new(("device", device.0)).with("preset field")
}

impl App {
    /// Opens the menu of the chosen device's field (the command); with
    /// `naming` it opens on the name for Save as….
    pub(crate) fn open_preset_menu(&mut self, ctx: &egui::Context, device: Id, naming: bool) {
        let offered = naming.then(|| self.current_name(device));
        self.chain.presets.menu = MenuState {
            open_for: Some(device),
            focus: if naming { Focus::Name } else { Focus::Search },
            naming: offered,
            ..MenuState::default()
        };
        if let Some(folders) = Some(self.preset_folders(device)).filter(|f| !f.is_empty()) {
            self.chain.presets.relist(ctx, device, folders);
        }
        widgets::Menu::open_for(ctx, field_id(device));
    }

    /// The menu under `device`'s field, when it is open.
    pub(in crate::chain) fn preset_menu(&mut self, field: &egui::Response, device: Id) {
        // Staying: typing a name is no reason for the menu to go when the
        // pointer leaves it; Esc or a click elsewhere closes it.
        let shown = widgets::Menu::below(field)
            .staying()
            .show(|ui| self.preset_menu_body(ui, device));
        if shown.is_none() && self.chain.presets.menu.open_for == Some(device) {
            self.chain.presets.menu = MenuState::default();
        }
    }

    fn preset_menu_body(&mut self, ui: &mut Ui, device: Id) {
        let ctx = ui.ctx().clone();
        ui.set_width(WIDTH);
        if self.chain.presets.menu.open_for != Some(device) {
            // Opened by a click on the field.
            self.chain.presets.menu = MenuState {
                open_for: Some(device),
                focus: Focus::Search,
                ..MenuState::default()
            };
            let folders = self.preset_folders(device);
            if !folders.is_empty() {
                self.chain.presets.relist(&ctx, device, folders);
            }
        }
        let mut menu = std::mem::take(&mut self.chain.presets.menu);
        let mut close = false;
        enum Act {
            Save,
            SaveAs,
            Folder,
        }
        let mut act: Option<Act> = None;

        if let Some(name) = menu.naming.as_mut() {
            widgets::menu_caption(ui, "Save as");
            let field = ui.add(
                egui::TextEdit::singleline(name)
                    .id(egui::Id::new(("preset name", device.0)))
                    .hint_text("Name")
                    .char_limit(NAME_LIMIT)
                    .desired_width(f32::INFINITY),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Preset name")
            });
            if menu.focus == Focus::Name {
                field.request_focus();
                menu.focus = Focus::None;
                // The name offered is all selected: typing replaces it.
                if let Some(mut edit) = egui::TextEdit::load_state(&ctx, field.id) {
                    let end = egui::text::CCursor::new(name.chars().count());
                    edit.cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            end,
                        )));
                    edit.store(&ctx, field.id);
                }
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut save = enter;
            ui.horizontal(|ui| {
                save |= crate::widgets::raised::primary_button(ui, "Save preset").clicked();
                if crate::widgets::raised::button(ui, "Cancel").clicked() {
                    menu.naming = None;
                }
            });
            if save {
                let typed = menu.naming.take().unwrap_or_default();
                if typed.trim().is_empty() {
                    self.say("Give the preset a name first.".to_owned());
                } else {
                    let name: String = typed.trim().chars().take(NAME_LIMIT).collect();
                    self.begin_preset_save(&ctx, device, name, None);
                    close = true;
                }
            }
        } else {
            let search = ui.add(
                egui::TextEdit::singleline(&mut menu.search)
                    .id(egui::Id::new(("preset search", device.0)))
                    .hint_text("Search presets")
                    .desired_width(f32::INFINITY),
            );
            search.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search presets")
            });
            if menu.focus == Focus::Search {
                search.request_focus();
                menu.focus = Focus::None;
            }
            ui.add_space(theme::space::XS);
            let choices = self.choices(device);
            let shown = filter(&choices, &menu.search);
            if let Some(picked) = self.preset_list(ui, device, &choices, &shown) {
                self.choose_preset(&ctx, device, picked);
                close = true;
            }
            ui.separator();
            if widgets::menu_toggle(ui, "Save", None, false).clicked() {
                act = Some(Act::Save);
            }
            if widgets::menu_toggle(ui, "Save as\u{2026}", None, false).clicked() {
                act = Some(Act::SaveAs);
            }
            if widgets::menu_toggle(ui, "Show presets folder", None, false).clicked() {
                act = Some(Act::Folder);
            }
        }
        match act {
            Some(Act::Save) => match self.save_over(device) {
                Some((name, path)) => {
                    close = true;
                    self.begin_preset_save(&ctx, device, name, Some(path));
                }
                // No file of the user's own to write over: ask for a name.
                None => {
                    menu.naming = Some(self.current_name(device));
                    menu.focus = Focus::Name;
                }
            },
            Some(Act::SaveAs) => {
                menu.naming = Some(self.current_name(device));
                menu.focus = Focus::Name;
            }
            Some(Act::Folder) => {
                self.show_presets_folder(ui.ctx(), device);
                close = true;
            }
            None => {}
        }
        self.chain.presets.menu = menu;
        self.finish_menu(ui, close);
    }

    fn finish_menu(&mut self, ui: &mut Ui, close: bool) {
        if close {
            self.chain.presets.menu = MenuState::default();
            ui.close();
        }
    }

    /// The name offered for Save as…: the sound's own, cut to the limit.
    fn current_name(&self, device: Id) -> String {
        self.chain
            .presets
            .current
            .get(&device)
            .map(|c| c.name.chars().take(NAME_LIMIT).collect())
            .unwrap_or_default()
    }

    /// The list of sounds: headings, then rows, as many as are in view. The
    /// one picked, if any.
    fn preset_list(
        &self,
        ui: &mut Ui,
        device: Id,
        all: &[Choice],
        shown: &[usize],
    ) -> Option<Choice> {
        if shown.is_empty() {
            let words = if all.is_empty() {
                "This device lists no presets, and none are saved for it."
            } else {
                "No preset matches. Clear the search to see them all."
            };
            ui.add_space(theme::space::XS);
            ui.label(egui::RichText::new(words).small().weak());
            return None;
        }
        // Rows and the headings between them, as a flat list so only the
        // rows in view are drawn.
        enum Line<'a> {
            Heading(String),
            Item(&'a Choice),
        }
        let mut lines: Vec<Line<'_>> = Vec::new();
        let mut last: Option<String> = None;
        for &i in shown {
            let heading = all[i].heading();
            if last.as_deref() != Some(heading.as_str()) {
                lines.push(Line::Heading(heading.clone()));
                last = Some(heading);
            }
            lines.push(Line::Item(&all[i]));
        }
        let current = self.chain.presets.current.get(&device);
        let font = FontId::proportional(theme::size::BODY);
        let row_height = ui.spacing().interact_size.y + theme::space::XS;
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt(("preset list", device.0))
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, lines.len(), |ui, range| {
                for line in &lines[range] {
                    match line {
                        Line::Heading(text) => {
                            // As tall as a row, so the list's height is exact.
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), row_height),
                                egui::Sense::hover(),
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + theme::space::S, rect.bottom() - 2.0),
                                egui::Align2::LEFT_BOTTOM,
                                text,
                                FontId::proportional(theme::size::CAPTION),
                                theme::p().text_hint,
                            );
                        }
                        Line::Item(choice) => {
                            let on = current.is_some_and(|c| choice.is_current(c));
                            let label = widgets::fit(
                                ui.painter(),
                                choice.name(),
                                font.clone(),
                                WIDTH - 2.0 * theme::space::M,
                            );
                            let row = widgets::choice_row(ui, &label, on);
                            if row.clicked() {
                                picked = Some((*choice).clone());
                            }
                        }
                    }
                }
            });
        picked
    }
}

/// The indexes of the choices that match every word of `search`, in order.
pub(in crate::chain) fn filter(choices: &[Choice], search: &str) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    choices
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let text = format!("{} {}", c.name(), c.group().unwrap_or("")).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}
