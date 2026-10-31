//! The Files page's folders (M9 R7): where projects start, and the cache of
//! converted audio and peaks with its limit, its size and Clear. A folder
//! choice goes through the system's folder dialog; nothing is moved or
//! copied.

use eframe::egui::{self, Ui};
use lumen_core::product::PRODUCT_NAME;

use crate::files::describe_size;
use crate::preferences::Marks;
use crate::widgets::{hint, section};
use crate::{App, settings, theme, widgets};

impl App {
    /// Where New, Open and Save as start.
    pub(super) fn projects_folder_row(&mut self, ui: &mut Ui, marks: &Marks) {
        section(ui, "Projects");
        let shown = self
            .projects_folder()
            .map_or_else(|| "Documents".to_owned(), |p| p.display().to_string());
        let chosen = self.settings.projects.folder.is_some();
        marks.row(ui, "files.projects_folder", |ui| {
            let asked = path_line(ui, &shown, "Projects folder", chosen);
            match asked {
                Some(Asked::Choose) => self.pick_projects_folder(ui.ctx()),
                Some(Asked::Standard) => self.projects_folder_chosen(None),
                None => {}
            }
        });
        hint(
            ui,
            "New, Open and Save as start here. Projects already saved stay where they are.",
        );
    }

    /// The cache: its folder, its limit, how full it is, and Clear.
    pub(super) fn cache_rows(&mut self, ui: &mut Ui, marks: &Marks) {
        section(ui, "Cache");
        let ctx = ui.ctx().clone();
        if self.cache.size.is_none() && !self.cache.busy() {
            self.count_cache(&ctx);
        }
        let shown = self.chosen_cache_dir().display().to_string();
        let chosen = self.settings.cache.folder.is_some();
        marks.row(ui, "files.cache_folder", |ui| {
            match path_line(ui, &shown, "Cache folder", chosen) {
                Some(Asked::Choose) => self.pick_cache_folder(&ctx),
                Some(Asked::Standard) => self.cache_folder_chosen(None),
                None => {}
            }
        });
        if self.chosen_cache_dir() != self.cache.dir {
            hint(
                ui,
                &format!(
                    "Used from the next start. Until then the cache stays in {}.",
                    self.cache.dir.display()
                ),
            );
        } else {
            hint(
                ui,
                "Converted audio and waveforms, made again whenever they are needed.",
            );
        }
        marks.row(ui, "files.cache_limit", |ui| {
            widgets::row(ui, "Limit", |ui| {
                let labels = settings::Cache::LIMITS_GB.map(|gb| format!("{gb} GB"));
                let names = settings::Cache::LIMITS_GB.map(|gb| format!("Cache limit: {gb} GB"));
                let limits: Vec<_> = settings::Cache::LIMITS_GB
                    .iter()
                    .zip(labels.iter().zip(&names))
                    .map(|(gb, (label, name))| widgets::Choice::new(*gb, label).named(name))
                    .collect();
                let chosen = self.settings.cache.limit_gb;
                if let Some(gb) =
                    widgets::segmented(ui, ui.id().with("cache limit"), chosen, &limits)
                {
                    self.cache_limit_chosen(&ctx, gb);
                }
            });
        });
        marks.row(ui, "files.cache_clear", |ui| {
            widgets::row(ui, "In use", |ui| {
                let busy = self.cache.busy();
                let size = match self.cache.size {
                    Some(bytes) => describe_size(bytes),
                    None => "Counting\u{2026}".to_owned(),
                };
                ui.label(size);
                let clear = ui
                    .add_enabled_ui(!busy, |ui| widgets::raised::button(ui, "Clear cache"))
                    .inner
                    .on_hover_text(
                        "Removes the converted audio and waveforms. What plays now is kept; \
                         the rest is made again when needed",
                    )
                    .on_disabled_hover_text("Busy counting or clearing the cache");
                if clear.clicked() {
                    self.clear_cache(&ctx, false);
                }
            });
        });
        self.previous_cache_row(ui, &ctx);
    }

    /// The folder left behind by a change of folder, offered for Clear once.
    fn previous_cache_row(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let Some(previous) = self.previous_cache_dir() else {
            return;
        };
        if self.cache.previous_size == Some(0) {
            // Nothing of the cache's is left there: nothing to offer.
            self.keep_previous_cache();
            return;
        }
        ui.add_space(theme::space::XS);
        let held = self
            .cache
            .previous_size
            .map_or_else(String::new, |b| format!(" It holds {}.", describe_size(b)));
        widgets::warning(
            ui,
            &format!(
                "The cache used to be in {}.{held} {PRODUCT_NAME} leaves it as it is unless                  you clear it.",
                previous.display()
            ),
            0.0,
        );
        ui.horizontal(|ui| {
            let busy = self.cache.busy();
            if ui
                .add_enabled_ui(!busy, |ui| widgets::raised::button(ui, "Clear old cache"))
                .inner
                .on_hover_text(format!(
                    "Removes the cache files {PRODUCT_NAME} made in the old folder, and nothing else"
                ))
                .clicked()
            {
                self.clear_cache(ctx, true);
            }
            if widgets::raised::button(ui, "Keep it")
                .on_hover_text("Leaves the old folder as it is and stops asking")
                .clicked()
            {
                self.keep_previous_cache();
            }
        });
    }
}

/// What a folder's line asked for.
enum Asked {
    Choose,
    Standard,
}

/// A folder's line: its path, cut short before the buttons, Choose… and,
/// when a folder was chosen, Standard to go back to the standard one.
fn path_line(ui: &mut Ui, path: &str, name: &str, chosen: bool) -> Option<Asked> {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, theme::size::CONTROL),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_size(egui::vec2(width, theme::size::CONTROL));
            let mut asked = None;
            let choose = widgets::raised::button(ui, "Choose\u{2026}")
                .on_hover_text(format!("Choose the {}", name.to_lowercase()));
            let label = format!("Choose the {}", name.to_lowercase());
            choose
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            if choose.clicked() {
                asked = Some(Asked::Choose);
            }
            if chosen {
                let standard = widgets::raised::button(ui, "Standard")
                    .on_hover_text("Go back to the standard folder");
                let label = format!("{name}: use the standard folder");
                standard.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                });
                if standard.clicked() {
                    asked = Some(Asked::Standard);
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let response = ui
                    .add(egui::Label::new(path).truncate())
                    .on_hover_text(path);
                let label = format!("{name}: {path}");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label)
                });
            });
            asked
        },
    )
    .inner
}
