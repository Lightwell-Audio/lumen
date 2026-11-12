//! The file list's header as the app runs it (audit M12): the search field
//! and what sits under it, the breadcrumb, the count and the folder menu,
//! and the designed empty state, applied to what was pressed. The words and
//! the card are `crate::browser::files::head`.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect, Ui, vec2};

use crate::app::panes::{self, Pane};
use crate::browser::Tab;
use crate::browser::controls;
use crate::browser::files::At;
use crate::browser::files::head::{self, MusicAnswer};
use crate::browser::files::row::ICON;
use crate::browser::files::walk::Kind;
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::{App, fonts, theme, widgets};

impl App {
    /// The search field and what sits under it: the count and Refresh, the
    /// folder menu (Samples), the breadcrumb (Places).
    pub(crate) fn files_controls(&mut self, ui: &mut Ui, ctx: &egui::Context, tab: Tab) {
        let hint = match tab {
            Tab::Samples => "Search samples",
            Tab::Projects => "Search projects",
            _ => "Search places",
        };
        let files = &mut self.browser.files;
        let text = files.text.entry(tab).or_default();
        let searched = controls::search(ui, text, &mut files.focus_search, hint);
        if searched.escaped {
            if text.is_empty() {
                panes::focus(ctx, Pane::Browser);
            } else {
                text.clear();
            }
        }
        ui.add_space(theme::space::XS);
        if tab == Tab::Samples
            && self.settings.browser.sample_folders.is_empty()
            && !self.settings.browser.music_answered
        {
            match head::music_card(ui) {
                Some(MusicAnswer::Choose) => self.pick_sample_folder(ctx),
                Some(MusicAnswer::Keep) => self.keep_music(),
                None => {}
            }
        }
        if tab == Tab::Places && self.browser.files.words(tab).trim().is_empty() {
            self.breadcrumb(ui, ctx);
        }
        self.count_line(ui, ctx, tab);
    }

    /// Places' way back up, and where it is; at the top, Add a place….
    fn breadcrumb(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let at = self.browser.files.at.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::XS;
            match &at {
                At::Top => {
                    let keys = self.keymap.keys(Command::AddPlace);
                    let add = widgets::tip(
                        widgets::raised::button(ui, "+ Add a place\u{2026}"),
                        "Add a place",
                        keys.as_ref(),
                        Some("Choose a folder to keep here, to browse it without looking for it again"),
                    );
                    if add.clicked() {
                        self.pick_place(ctx);
                    }
                }
                At::Computer | At::Folder(_) => {
                    let up = widgets::raised::icon_button(ui, "Up a folder", |p, at, c| {
                        let s = egui::Stroke::new(theme::stroke::LIT, c);
                        p.line_segment([at + vec2(0.0, 5.0), at - vec2(0.0, 5.0)], s);
                        p.line_segment([at - vec2(0.0, 5.0), at + vec2(-4.0, -1.0)], s);
                        p.line_segment([at - vec2(0.0, 5.0), at + vec2(4.0, -1.0)], s);
                    });
                    if widgets::tip(up, "Up a folder", None, Some("Or press Left or Backspace in the list"))
                        .clicked()
                    {
                        self.folder_up();
                    }
                    let crumb = match &at {
                        At::Folder(dir) => head::crumb_of(dir, &self.settings.browser.places),
                        _ => "Computer".to_owned(),
                    };
                    let label = egui::Label::new(
                        egui::RichText::new(crumb)
                            .font(fonts::medium(theme::size::CAPTION))
                            .color(theme::p().text),
                    )
                    .truncate()
                    .selectable(false);
                    ui.add(label);
                }
            }
        });
        ui.add_space(theme::space::XS);
    }

    /// The count, what a read is doing (with Cancel), Refresh, and in
    /// Samples the folder menu.
    fn count_line(&mut self, ui: &mut Ui, ctx: &egui::Context, tab: Tab) {
        let words = self.browser.files.words(tab).trim().to_owned();
        let top = tab == Tab::Places && words.is_empty() && self.browser.files.at == At::Top;
        if top {
            return;
        }
        let narrow = (tab == Tab::Samples)
            .then(|| self.browser.files.narrow.clone())
            .flatten();
        let listing = self.browser.files.listing(tab);
        let shown = listing.shown(&words, narrow.as_deref()).len();
        // Only an open folder in Places lists folders: they are counted
        // apart from the files.
        let folders = if tab == Tab::Places && words.is_empty() {
            let indexes = listing.shown(&words, None).to_vec();
            indexes
                .iter()
                .filter(|&&i| listing.rows[i].kind == Kind::Folder)
                .count()
        } else {
            0
        };
        let found = head::count_with_folders(folders, shown - folders, tab);
        let reading = listing.reading;
        let said = head::count_said(found, reading, listing.outcome);
        let mut stop = false;
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::space::XS;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(said)
                        .size(theme::size::CAPTION)
                        .color(theme::p().text_secondary),
                )
                .truncate()
                .selectable(false),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if reading {
                    stop = widgets::raised::button(ui, "Cancel").clicked();
                } else {
                    let r = widgets::raised::icon_button(ui, "Refresh", |p, at, c| {
                        let rect = Rect::from_center_size(at, vec2(ICON, ICON));
                        p.extend(icons::shapes(Icon::Loop, rect, c));
                    });
                    let keys = self.keymap.keys(Command::RefreshFiles);
                    refresh =
                        widgets::tip(r, "Refresh", keys.as_ref(), Some("Reads the folders again"))
                            .clicked();
                }
                if tab == Tab::Samples {
                    self.folder_menu(ui, ctx);
                }
            });
        });
        if stop {
            self.browser.files.listing(tab).stop();
        }
        if refresh {
            self.browser.files.ask_refresh();
        }
        ui.add_space(theme::space::XS);
    }

    /// Samples' folder menu: every folder, one folder of them, adding and
    /// removing sample folders.
    fn folder_menu(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let narrow = self.browser.files.narrow.clone();
        let label = narrow.as_ref().map_or_else(
            || "All folders".to_owned(),
            |p| {
                p.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            },
        );
        let button = widgets::chip_button(ui, ui.id().with("sample folders"), &label, true)
            .on_hover_text("Narrow the list to one folder, or add a sample folder");
        let chosen = self.settings.browser.sample_folders.clone();
        widgets::Menu::below(&button).show(|ui| {
            // The folders one level in, from what was read: looked for only
            // while the menu is open.
            let inner = head::inner_sample_folders(
                &self.sample_folders(),
                &self.browser.files.samples.rows,
            );
            if widgets::menu_toggle(ui, "All folders", None, narrow.is_none()).clicked() {
                self.browser.files.narrow = None;
                ui.close();
            }
            for folder in inner.iter().take(40) {
                let name = folder
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                if widgets::menu_toggle(ui, &name, None, narrow.as_ref() == Some(folder)).clicked()
                {
                    self.browser.files.narrow = Some(folder.clone());
                    ui.close();
                }
            }
            ui.separator();
            if self.menu_command(
                ui,
                Command::AddSampleFolder,
                Command::AddSampleFolder.name(),
            ) {
                ui.close();
            }
            for folder in &chosen {
                let name = Path::new(folder)
                    .file_name()
                    .map_or_else(|| folder.clone(), |n| n.to_string_lossy().into_owned());
                if widgets::menu_item(ui, &format!("Remove {name}"), None, true)
                    .on_hover_text(folder)
                    .clicked()
                {
                    self.remove_sample_folder(Path::new(folder));
                    ui.close();
                }
            }
        });
        let _ = ctx;
    }

    /// The list is empty: a designed state for each reason.
    pub(crate) fn files_empty(&mut self, ui: &mut Ui, area: Rect, tab: Tab) {
        let words = self.browser.files.words(tab).trim().to_owned();
        let listing = self.browser.files.listing(tab);
        let (reading, error) = (listing.reading, listing.error.clone());
        let missing_place = match &self.browser.files.at {
            At::Folder(dir) if tab == Tab::Places => self
                .settings
                .browser
                .places
                .iter()
                .find(|p| dir.starts_with(&p.path))
                .filter(|_| error.is_some())
                .map(|p| PathBuf::from(&p.path)),
            _ => None,
        };
        // What could not be read is said before "No files match": a search
        // of a folder that is not there would otherwise hide why. With
        // words typed, the one action is still to clear them.
        let (title, hint, action) = head::empty_words(
            tab,
            reading,
            error.as_deref(),
            &words,
            missing_place.as_deref(),
        );
        let state = widgets::EmptyState {
            icon: Some(head::empty_icon),
            icon_size: theme::space::L,
            title: &title,
            hint: &hint,
            action: action.map(|(label, tip)| widgets::EmptyAction {
                label,
                keys: None,
                tip,
            }),
        };
        if widgets::empty_state(ui, area, &state) {
            let ctx = ui.ctx().clone();
            if !words.is_empty() {
                self.browser.files.text.remove(&tab);
            } else if let Some(place) = missing_place {
                self.find_place(&ctx, place);
            } else if tab == Tab::Samples {
                self.pick_sample_folder(&ctx);
            }
        }
    }
}
