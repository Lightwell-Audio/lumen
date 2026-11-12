//! The file list as the app runs it (audit M12): drawing the rows in view,
//! the list's keys, and applying what a row or a key asked for. The items a
//! tab shows and the list's layout are `crate::browser::files::list`.

use std::path::Path;
use std::sync::Arc;

use eframe::egui::{self, Key, Modifiers, Rect, Ui, pos2, vec2};

use crate::app::panes::{self, Pane};
use crate::browser::Tab;
use crate::browser::files::At;
use crate::browser::files::list::{
    AHEAD, HEADING, Item, ItemsKey, LIST_KEYS, Out, PICTURES_FROM, ROW, ROW_TWO, made_items,
    top_places, two_lines,
};
use crate::browser::files::row::{RowLook, folder_line, heading, row_id};
use crate::browser::files::walk::{Entry, Kind};
use crate::browser::files::{saved, strip};
use crate::{App, theme, widgets};

impl App {
    /// Draws a file tab in `body`, under the tab strip.
    pub(crate) fn files_tab(&mut self, ui: &mut Ui, body: Rect) {
        let ctx = ui.ctx().clone();
        self.files_ready(&ctx);
        let tab = self.browser.tab;
        let side = (theme::space::M - theme::space::S) as i8;
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(side, 0))
            .show(ui, |ui| self.files_controls(ui, &ctx, tab));
        ui.add_space(theme::space::XS);

        let items = self.items(tab);
        // A read that found files and also met a folder it could not read
        // says so above what it found, in full (an empty list says it in
        // its designed state instead).
        let error = self.browser.files.listing(tab).error.clone();
        if let Some(error) = error.filter(|_| !items.is_empty()) {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(side, 0))
                .show(ui, |ui| widgets::warning(ui, &error, 0.0));
            ui.add_space(theme::space::XS);
        }
        let strip_height = self.strip_height(ui, tab, body.width());
        let top = ui.cursor().top();
        let split = (body.bottom() - strip_height).max(top);
        let area = Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), split));
        // An empty folder still takes Left and Backspace, and Esc still
        // stops a preview.
        let moved = self.files_keys(&ctx, tab, &items);
        if items.is_empty() {
            self.files_empty(ui, area, tab);
        } else {
            let out = self.files_list(ui, area, tab, &items, moved);
            self.apply_files(&ctx, tab, out);
        }
        if strip_height > 0.0 {
            let rect = Rect::from_min_max(pos2(body.left(), split), body.max);
            self.files_strip(ui, rect, tab);
        }
    }

    /// The rows the open tab shows. Made again only when what they come
    /// from changes, so a frame of a long list copies nothing.
    fn items(&mut self, tab: Tab) -> Arc<[Item]> {
        let words = self.browser.files.words(tab).trim().to_owned();
        if tab == Tab::Places && words.is_empty() && self.browser.files.at == At::Top {
            return top_places(&self.settings.browser.places, &self.browser.files.present).into();
        }
        let narrow = (tab == Tab::Samples)
            .then(|| self.browser.files.narrow.clone())
            .flatten();
        let recent = if tab == Tab::Projects {
            self.settings.projects.recent.clone()
        } else {
            Vec::new()
        };
        let key = ItemsKey {
            tab,
            generation: self.browser.files.listing(tab).generation,
            words,
            narrow,
            recent,
        };
        if let Some((made_for, items)) = &self.browser.files.items
            && *made_for == key
        {
            return Arc::clone(items);
        }
        let items: Arc<[Item]> = made_items(self.browser.files.listing(key.tab), &key).into();
        self.browser.files.items = Some((key, Arc::clone(&items)));
        items
    }

    /// The list's keys while the pane has the keyboard. Returns whether the
    /// selection moved (to bring it into view).
    fn files_keys(&mut self, ctx: &egui::Context, tab: Tab, items: &[Item]) -> bool {
        let had_keys = std::mem::replace(
            &mut self.browser.files.had_keys,
            panes::focused(ctx) == Some(Pane::Browser),
        );
        if ctx.text_edit_focused()
            || self.typing
            || !ctx.input(|i| LIST_KEYS.iter().any(|k| i.key_pressed(*k)))
        {
            return false;
        }
        let rows: Vec<&Entry> = items.iter().filter_map(Item::entry).collect();
        let last = rows.len().checked_sub(1);
        // The list has the keys while the pane does, or one of its rows
        // (reached with Tab).
        let row_focused = ctx
            .memory(egui::Memory::focused)
            .is_some_and(|f| rows.iter().any(|r| row_id(&r.path) == f));
        // Esc drops egui's focus before the list sees it: what counts is
        // whether the pane had it the frame before.
        let escaped = ctx.input(|i| i.key_pressed(Key::Escape));
        let pane = panes::focused(ctx) == Some(Pane::Browser) || (escaped && had_keys);
        if !pane && !row_focused {
            return false;
        }
        // The strip's waveform and level keep their arrow keys while they
        // have the keyboard.
        let strip = [
            egui::Id::new(strip::WELL_ID),
            egui::Id::new(strip::LEVEL_ID),
        ];
        if ctx
            .memory(egui::Memory::focused)
            .is_some_and(|f| strip.contains(&f))
        {
            return false;
        }
        let key = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        let selected = self.browser.files.selected.get(&tab).cloned();
        let at = selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| &r.path == s));
        let to = if key(Modifiers::NONE, Key::ArrowDown) {
            last.map(|last| at.map_or(0, |i| (i + 1).min(last)))
        } else if key(Modifiers::NONE, Key::ArrowUp) {
            last.map(|last| at.map_or(last, |i| i.saturating_sub(1)))
        } else if key(Modifiers::NONE, Key::Home) {
            last.map(|_| 0)
        } else if key(Modifiers::NONE, Key::End) {
            last
        } else {
            None
        };
        let current = at.map(|i| rows[i].clone());
        if key(Modifiers::SHIFT, Key::Enter) {
            if let Some(row) = &current {
                self.preview_file(&row.path, 0.0);
            }
        } else if key(Modifiers::NONE, Key::Enter) {
            if let Some(row) = &current {
                self.open_entry(ctx, row);
            }
        } else if key(Modifiers::NONE, Key::ArrowRight) {
            if let Some(row) = current.as_ref().filter(|r| r.kind == Kind::Folder) {
                self.open_entry(ctx, row);
            }
        } else if (key(Modifiers::NONE, Key::ArrowLeft) || key(Modifiers::NONE, Key::Backspace))
            && tab == Tab::Places
        {
            self.folder_up();
        } else if self.previewing() && key(Modifiers::NONE, Key::Escape) {
            // The first Esc stops the preview and keeps the keyboard here;
            // the next gives it back.
            self.stop_preview();
            panes::focus(ctx, Pane::Browser);
        } else if key(Modifiers::SHIFT, Key::F10)
            && let Some(row) = &current
        {
            widgets::Menu::open_for(ctx, row_id(&row.path));
        }
        match to {
            Some(i) if Some(i) != at => {
                let row = rows[i].clone();
                self.choose_row(tab, &row);
                true
            }
            _ => false,
        }
    }

    /// A row was chosen (a click or the arrows): it is selected, and with
    /// Auto on a sound plays (the owner's P A).
    fn choose_row(&mut self, tab: Tab, row: &Entry) {
        self.browser.files.selected.insert(tab, row.path.clone());
        match row.kind {
            Kind::Audio if self.settings.browser.auto_preview => self.preview_file(&row.path, 0.0),
            Kind::Project => self.browser.files.want_about(&row.path),
            _ => {}
        }
    }

    /// Enter or a double-click: Computer and folders open, the rest as
    /// [`App::open_row`].
    fn open_entry(&mut self, ctx: &egui::Context, row: &Entry) {
        if row.path == Path::new(saved::COMPUTER) {
            self.browser.files.at = At::Computer;
            self.browser.files.scroll_to = true;
            return;
        }
        self.open_row(row, ctx);
    }

    /// The rows in view, painted one by one.
    fn files_list(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        tab: Tab,
        items: &[Item],
        moved: bool,
    ) -> Out {
        let mut out = Out::default();
        let two = two_lines(tab, self.browser.files.words(tab));
        let row_h = if two { ROW_TWO } else { ROW };
        let height = |i: &Item| {
            if matches!(i, Item::Heading(_)) {
                HEADING
            } else {
                row_h
            }
        };
        // Only Projects has headings, and a list that has any starts with
        // one: every other list's rows are one height, and only the rows in
        // view are visited.
        let uniform = !matches!(items.first(), Some(Item::Heading(_)));
        let total: f32 = if uniform {
            row_h * items.len() as f32
        } else {
            items.iter().map(height).sum()
        };
        let selected = self.browser.files.selected.get(&tab).cloned();
        let scroll = moved || std::mem::take(&mut self.browser.files.scroll_to);
        // Looked for only when it is to be brought into view.
        let wanted = selected.as_ref().filter(|_| scroll).and_then(|s| {
            items
                .iter()
                .position(|i| i.entry().is_some_and(|e| &e.path == s))
        });
        let pictures = rect.width() >= PICTURES_FROM;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let width = rect.width();
        let roots = self.sample_folders();
        egui::ScrollArea::vertical()
            .id_salt(("file list", tab))
            .auto_shrink([false, false])
            .show_viewport(&mut child, |ui, viewport| {
                let origin = ui.cursor().min;
                ui.allocate_space(vec2(width, total));
                let first = if uniform {
                    ((viewport.top() / row_h).floor().max(0.0) as usize).saturating_sub(AHEAD)
                } else {
                    0
                };
                let mut top = if uniform { first as f32 * row_h } else { 0.0 };
                for (i, item) in items.iter().enumerate().skip(first) {
                    let height = height(item);
                    let bottom = top + height;
                    let near = bottom >= viewport.top() - AHEAD as f32 * row_h
                        && top <= viewport.bottom() + AHEAD as f32 * row_h;
                    let bring = scroll && Some(i) == wanted;
                    if top > viewport.bottom() + AHEAD as f32 * row_h
                        && !bring
                        && (uniform || wanted.is_none_or(|w| w < i))
                    {
                        break;
                    }
                    if near || bring {
                        let at = Rect::from_min_size(
                            pos2(origin.x + theme::space::XS, origin.y + top),
                            vec2(width - 2.0 * theme::space::XS - theme::space::XS, height),
                        );
                        if bring {
                            ui.scroll_to_rect(at, None);
                        }
                        let visible = at.intersects(viewport.translate(origin.to_vec2()));
                        match item {
                            Item::Heading(text) => heading(ui, at, text),
                            Item::Row(entry) | Item::Place(entry, _) => {
                                let there = !matches!(item, Item::Place(_, false));
                                let look = RowLook {
                                    selected: selected.as_ref() == Some(&entry.path),
                                    two,
                                    pictures,
                                    there,
                                    place: matches!(item, Item::Place(..)),
                                    folder_line: folder_line(entry, &roots),
                                };
                                if entry.kind != Kind::Folder && entry.kind != Kind::Project {
                                    self.browser.files.want_picture(&entry.path);
                                }
                                if visible || bring {
                                    self.file_row(ui, at, entry, &look, &mut out);
                                }
                            }
                        }
                    }
                    top = bottom;
                }
            });
        out
    }

    fn apply_files(&mut self, ctx: &egui::Context, tab: Tab, out: Out) {
        if let Some(path) = out.select {
            let kind = if path == Path::new(saved::COMPUTER) {
                Kind::Folder
            } else {
                Kind::of_file(&path).unwrap_or(Kind::Folder)
            };
            let entry = Entry::new(path, kind);
            self.choose_row(tab, &entry);
            panes::focus(ctx, Pane::Browser);
        }
        if let Some(path) = out.preview {
            if self
                .browser
                .files
                .preview
                .as_ref()
                .is_some_and(|p| p.is_playing(&path))
            {
                self.stop_preview();
            } else {
                self.browser.files.selected.insert(tab, path.clone());
                self.preview_file(&path, 0.0);
            }
        }
        if out.stop {
            self.stop_preview();
        }
        if let Some(entry) = out.open {
            panes::focus(ctx, Pane::Browser);
            self.open_entry(ctx, &entry);
        }
        if let Some(path) = out.reveal {
            crate::crash::reveal(&path);
        }
        if let Some(path) = out.add_place {
            self.add_place(&path);
        }
        if let Some(path) = out.remove_place {
            self.remove_place(&path);
        }
        if let Some(path) = out.find_place {
            self.find_place(ctx, path);
        }
    }
}
