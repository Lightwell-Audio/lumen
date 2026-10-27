//! Revert to saved (M9 slice R4, ADR-0105). The saved file is read back, so
//! everything is as it was saved, including what a plugin keeps inside its own
//! window, which the history does not hold. Nothing is lost: the unsaved
//! version is first written to `Backups/Reverted` beside the project, with the
//! plugins read as for any save, and a message afterwards offers to open it.
//! The prompt is a quiet window, not a modal dialog; the work is the copy
//! worker's (`app::copies`), so it shows progress and stops on Esc.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, RichText, pos2};

use crate::app::copies::Purpose;
use crate::app::projects::Next;
use crate::widgets::{Dialog, raised};
use crate::{App, files, theme};

/// Where a project's unsaved versions are put away, inside its folder.
const FOLDER: &str = "Backups/Reverted";

/// What the app keeps about a revert.
#[derive(Default)]
pub(crate) struct Revert {
    /// The prompt is showing.
    asking: bool,
    /// The saved file is being read back, and where the unsaved version went.
    pub(crate) reloading: Option<PathBuf>,
    /// Where the last revert put the changes it dropped: the banner offers
    /// them until it is dismissed or another project is open.
    pub(crate) kept: Option<PathBuf>,
}

impl Revert {
    /// Whether the prompt is showing.
    pub(crate) fn asking(&self) -> bool {
        self.asking
    }
}

/// Where the unsaved version of the project in `file` goes: a file named for
/// it and the moment, in the project's own `Backups/Reverted` folder.
pub(crate) fn safety_file(file: &Path, stamp: &str) -> Option<PathBuf> {
    let stem = file.file_stem()?.to_string_lossy();
    let mut folder = file.parent()?.to_path_buf();
    folder.extend(FOLDER.split('/'));
    Some(folder.join(format!("{stem} {stamp}.{}", files::EXTENSION)))
}

impl App {
    /// Revert is possible now, or why not.
    pub(crate) fn revert_unavailable(&self) -> Result<(), &'static str> {
        if self.document.path.is_none() {
            Err("This project has not been saved yet")
        } else if !self.document.is_dirty() {
            Err("There are no unsaved changes")
        } else if self.opening.is_some() {
            Err(crate::app::commands::wait::OPENING)
        } else {
            self.copy_unavailable()
        }
    }

    /// Revert to saved: asks first, in a quiet window.
    pub(crate) fn begin_revert(&mut self) {
        match self.revert_unavailable() {
            Ok(()) => self.revert.asking = true,
            Err(why) => self.say(format!("{why}, so there is nothing to revert")),
        }
    }

    /// The prompt. Call every frame.
    pub(crate) fn revert_window(&mut self, ctx: &egui::Context) {
        if !self.revert.asking {
            return;
        }
        // Saved, or another project open, while the prompt was showing.
        if self.document.path.is_none() || !self.document.is_dirty() {
            self.revert.asking = false;
            return;
        }
        let title = format!("Revert to the saved {}?", self.document.name());
        let mut choice = None;
        Dialog::new("revert", &title, 380.0)
            .at(
                pos2(ctx.content_rect().center().x, 120.0),
                Align2::CENTER_TOP,
            )
            .question()
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Everything changed since the last save is dropped. It is put away first, \
                         in the Backups/Reverted folder beside the project, and a message then \
                         offers to open it.",
                    )
                    .color(theme::p().text_secondary),
                );
                ui.add_space(theme::space::M);
                ui.horizontal(|ui| {
                    if raised::primary_button(ui, "Revert").clicked() {
                        choice = Some(true);
                    }
                    if raised::button(ui, "Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                self.revert.asking = false;
                self.start_revert();
            }
            Some(false) => self.revert.asking = false,
            None => {}
        }
    }

    /// Puts the unsaved version away; the saved file is read back when that
    /// is done (`copies::copy_done`).
    fn start_revert(&mut self) {
        if let Err(why) = self.revert_unavailable() {
            self.say(format!("{why}, so nothing was reverted"));
            return;
        }
        let Some(safety) = self
            .document
            .path
            .as_deref()
            .and_then(|file| safety_file(file, &lumen_project::utc_timestamp()))
        else {
            return;
        };
        self.start_save_for(Purpose::Revert, safety, false);
    }

    /// The put-away copy is written: reads the saved file back, unless the
    /// project changed in the meantime (what changed then is in neither).
    pub(crate) fn reload_saved(&mut self, kept: PathBuf, mark: u64) {
        let Some(file) = self.document.path.clone() else {
            return;
        };
        if self.document.change_mark() != mark {
            self.file_notice = Some(format!(
                "The project changed while your changes were being put away, so it was not \
                 reverted. The earlier changes are in {}. Choose Revert to saved again.",
                kept.display()
            ));
            return;
        }
        self.revert.reloading = Some(kept);
        self.opening = Some(files::open(file));
    }

    /// The message after a revert, with the way to the changes it dropped.
    /// Call every frame, with the other banners.
    pub(crate) fn revert_banner(&mut self, ui: &mut egui::Ui) {
        let Some(kept) = self.revert.kept.clone() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let mut open = false;
        let mut dismiss = false;
        egui::Panel::top("revert banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card(ui, |ui| {
                    ui.label(format!(
                        "Reverted to the saved {}. The changes you had not saved are kept in {FOLDER}.",
                        self.document.name()
                    ));
                    if raised::primary_button(ui, "Open them").clicked() {
                        open = true;
                    }
                    if raised::button(ui, "Dismiss").clicked() {
                        dismiss = true;
                    }
                });
            });
        if open {
            self.revert.kept = None;
            self.request(&ctx, Next::Open(Some(kept)));
        } else if dismiss {
            self.revert.kept = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unsaved_version_goes_in_the_projects_own_reverted_folder() {
        let file = Path::new("Music").join("Song").join("Song.lumen");
        let stamp = "2026-10-26T120000.000Z";
        assert_eq!(
            safety_file(&file, stamp),
            Some(
                Path::new("Music")
                    .join("Song")
                    .join("Backups")
                    .join("Reverted")
                    .join(format!("Song {stamp}.lumen"))
            )
        );
        assert_eq!(safety_file(Path::new(""), stamp), None, "no file, no name");
    }
}
