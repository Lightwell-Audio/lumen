//! What sits above the file list (M9 R7c): the search, Samples' first
//! visit, Places' way back up, the count with Refresh and the folder menu,
//! and the designed empty states.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect, Ui, vec2};

use super::At;
use super::row::ICON;
use super::walk::{Kind, Outcome};
use crate::app::panes::{self, Pane};
use crate::browser::{Tab, controls};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::{App, fonts, theme, widgets};

impl App {
    /// The search field and what sits under it: the count and Refresh, the
    /// folder menu (Samples), the breadcrumb (Places).
    pub(super) fn files_controls(&mut self, ui: &mut Ui, ctx: &egui::Context, tab: Tab) {
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
            self.music_card(ui, ctx);
        }
        if tab == Tab::Places && self.browser.files.words(tab).trim().is_empty() {
            self.breadcrumb(ui, ctx);
        }
        self.count_line(ui, ctx, tab);
    }

    /// Samples' first visit: it shows the Music folder until one is chosen.
    fn music_card(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let (mut choose, mut keep) = (false, false);
        let palette = *theme::p();
        egui::Frame::NONE
            .fill(palette.raised)
            .stroke(egui::Stroke::new(theme::stroke::HAIRLINE, palette.border))
            .corner_radius(theme::radius::LARGE)
            .inner_margin(egui::Margin::same(theme::space::S as i8))
            .show(ui, |ui| {
            wrapped(
                ui,
                "Samples lists every sound in your Music folder until you choose the folder you keep samples in.",
            );
            ui.horizontal_wrapped(|ui| {
                choose = widgets::raised::primary_button(ui, "Choose a folder\u{2026}").clicked();
                keep = widgets::raised::button(ui, "Keep Music").clicked();
            });
        });
        if choose {
            self.pick_sample_folder(ctx);
        } else if keep {
            self.keep_music();
        }
        ui.add_space(theme::space::XS);
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
                        At::Folder(dir) => self.crumb_of(dir),
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

    /// "Drums › Kicks": the folder from the place it is in.
    fn crumb_of(&self, dir: &Path) -> String {
        let place = self
            .settings
            .browser
            .places
            .iter()
            .filter(|p| dir.starts_with(&p.path))
            .max_by_key(|p| p.path.len());
        match place {
            Some(place) => {
                let rest = dir.strip_prefix(&place.path).unwrap_or(dir);
                std::iter::once(place.name.clone())
                    .chain(rest.iter().map(|p| p.to_string_lossy().into_owned()))
                    .collect::<Vec<_>>()
                    .join(" \u{203a} ")
            }
            None => dir.display().to_string(),
        }
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
        let found = count_with_folders(folders, shown - folders, tab);
        let reading = listing.reading;
        let said = match (reading, listing.outcome) {
            (true, _) => format!("Reading\u{2026} {found}"),
            (false, Some(Outcome::Stopped)) => format!("{found}, stopped"),
            (false, Some(Outcome::Limited)) => {
                format!("{found}, partial: the folder is very large")
            }
            _ => found,
        };
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

    /// The folders one level inside the sample folders that hold a file
    /// read so far, each once.
    fn inner_sample_folders(&self) -> Vec<PathBuf> {
        let roots = self.sample_folders();
        let mut inner: Vec<PathBuf> = self
            .browser
            .files
            .samples
            .rows
            .iter()
            .filter_map(|e| {
                let root = roots.iter().find(|r| e.path.starts_with(r))?;
                let first = e.path.strip_prefix(root).ok()?.iter().next()?;
                let path = root.join(first);
                (path != e.path).then_some(path)
            })
            .collect();
        inner.sort();
        inner.dedup();
        inner
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
            let inner = self.inner_sample_folders();
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
    pub(super) fn files_empty(&mut self, ui: &mut Ui, area: Rect, tab: Tab) {
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
        let clear = ("Clear the search", "Shows every file again");
        // What could not be read is said before "No files match": a search
        // of a folder that is not there would otherwise hide why. With
        // words typed, the one action is still to clear them.
        let (title, hint, action): (String, String, Option<(&str, &str)>) = if reading {
            (
                "Reading\u{2026}".into(),
                "The files appear as they are found".into(),
                None,
            )
        } else if let Some(place) = missing_place.as_ref().filter(|_| words.is_empty()) {
            (
                format!("{} is not there", place.display()),
                "An unplugged drive, or a folder that moved".into(),
                Some(("Find it\u{2026}", "Choose where the folder is now")),
            )
        } else if let Some(error) = error {
            let action = (!words.is_empty()).then_some(clear);
            ("This folder cannot be read".into(), error, action)
        } else if !words.is_empty() {
            (
                "No files match".into(),
                "Try other words, or clear the search".into(),
                Some(clear),
            )
        } else {
            match tab {
                Tab::Samples => (
                    "No sounds here yet".into(),
                    "Audio and MIDI files in your sample folders show here".into(),
                    Some((
                        "Add a sample folder\u{2026}",
                        "Choose another folder to list",
                    )),
                ),
                Tab::Projects => (
                    "No projects yet".into(),
                    "Projects you save, and the projects folder's, show here".into(),
                    None,
                ),
                _ => (
                    "Nothing here".into(),
                    "This folder holds no folders and no audio or MIDI files".into(),
                    None,
                ),
            }
        };
        let state = widgets::EmptyState {
            icon: Some(|p, at, c| {
                let rect = Rect::from_center_size(at, vec2(theme::space::L, theme::space::L));
                p.extend(icons::shapes(Icon::Folder, rect, c));
            }),
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

/// "2 folders", "2 folders, 3 files", or [`count`] when there are no
/// folders.
fn count_with_folders(folders: usize, files: usize, tab: Tab) -> String {
    let folders_said = || {
        let word = if folders == 1 { "folder" } else { "folders" };
        format!("{} {word}", thousands(folders))
    };
    match (folders, files) {
        (0, _) => count(files, tab),
        (_, 0) => folders_said(),
        _ => format!("{}, {}", folders_said(), count(files, tab)),
    }
}

/// "1,284 files" or "12 projects".
fn count(n: usize, tab: Tab) -> String {
    let what = match (tab, n) {
        (Tab::Projects, 1) => "project",
        (Tab::Projects, _) => "projects",
        (_, 1) => "file",
        _ => "files",
    };
    format!("{} {what}", thousands(n))
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn wrapped(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text),
        )
        .wrap()
        .selectable(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_words() {
        assert_eq!(count(1, Tab::Samples), "1 file");
        assert_eq!(count(1284, Tab::Samples), "1,284 files");
        assert_eq!(count(2, Tab::Projects), "2 projects");
        assert_eq!(count_with_folders(2, 0, Tab::Places), "2 folders");
        assert_eq!(count_with_folders(1, 3, Tab::Places), "1 folder, 3 files");
        assert_eq!(count_with_folders(0, 3, Tab::Places), "3 files");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
