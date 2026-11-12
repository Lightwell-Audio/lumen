//! Writing the project: a save, a copy, a collect or a template.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;
use lumen_project::model::Project;

use super::dialogs::{FileDialog, made};
use crate::App;
use crate::app::copies::{Purpose, Waiting};
use crate::files;
use crate::trace;

/// How long a save waits for the plugins' states to be read. Each read has
/// its own deadline in the host; this is the whole wait.
pub(super) const PLUGIN_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

impl App {
    /// The project as it should be saved: the document with the view
    /// stamped in (it is not an undo step).
    pub(crate) fn stamped(&self) -> Project {
        let mut project = self.document.project().clone();
        project.view = lumen_project::model::ViewState {
            left_beat: self.nav.view.left as f32,
            px_per_beat: self.nav.view.zoom as f32,
            browser_open: self.show_browser,
            detail_open: self.show_detail,
            browser_size: self.browser_size,
            detail_size: self.detail_size,
            grid: self.grid,
            ..project.view.clone()
        };
        self.arrangement.save_into(&mut project.view);
        project.view.folded_devices = self.chain.folded.iter().copied().collect();
        project.view.shown_params = self.chain.shown.clone();
        // What the running plugins report goes in with the view: it is not an
        // edit (M6 slice 2c, the owner's C2).
        self.hosting.stamp(&mut project);
        project
    }

    /// Save (Ctrl+S): to the project's file, or ask where (Save As).
    pub(crate) fn save(&mut self, ctx: &egui::Context, ask: bool) {
        if self.saving.is_some()
            || self.save_waiting.is_some()
            || self.collecting.is_some()
            || self.dialog.is_some()
        {
            return;
        }
        match self.document.path.clone() {
            Some(path) if !ask => self.start_save(path),
            _ => {
                let name = format!("{}.{}", self.document.name(), files::EXTENSION);
                let start = self.dialog_start();
                let owner = self.owner;
                let show = move || {
                    let mut dialog = rfd::FileDialog::new()
                        .set_title("Save project")
                        .add_filter(format!("{PRODUCT_NAME} project"), &[files::EXTENSION])
                        .set_file_name(name);
                    if let Some(dir) = start {
                        dialog = dialog.set_directory(made(dir));
                    }
                    if let Some(owner) = &owner {
                        dialog = dialog.set_parent(owner);
                    }
                    FileDialog::SaveAs(dialog.save_file())
                };
                self.save_dialog = true;
                self.file_dialog(ctx, show, |paths| {
                    FileDialog::SaveAs(paths.into_iter().next())
                });
            }
        }
    }

    pub(crate) fn start_save(&mut self, path: PathBuf) {
        self.start_save_for(Purpose::Save, path, false);
    }

    pub(crate) fn start_save_as(&mut self, chosen: PathBuf) {
        self.start_save_for(Purpose::Save, chosen, true);
    }

    /// Starts a save, a copy or a collect.
    pub(crate) fn start_save_for(&mut self, purpose: Purpose, path: PathBuf, normalize: bool) {
        // Every running plugin is read first, and the save is written when
        // the answers are in (or after a few seconds, with what there is).
        if self.hosting.request_captures() {
            trace::log(|| {
                format!(
                    "files: reading the plugins before saving {}",
                    path.display()
                )
            });
            self.save_waiting = Some(Waiting {
                purpose,
                path,
                normalize,
                since: Instant::now(),
            });
            return;
        }
        self.write(purpose, path, normalize);
    }

    pub(super) fn write(&mut self, purpose: Purpose, path: PathBuf, normalize: bool) {
        match purpose {
            Purpose::Save => self.write_save(path, normalize),
            Purpose::Copy | Purpose::Collect | Purpose::Template | Purpose::Revert => {
                self.write_copy(purpose, path, normalize)
            }
        }
    }

    /// Writes the save, with the plugins' states as they were last read.
    fn write_save(&mut self, path: PathBuf, normalize_project_path: bool) {
        trace::log(|| format!("files: saving {}", path.display()));
        self.document.begin_save();
        self.record.audio.notes.save_started();
        self.saving = Some(files::save(files::SaveRequest {
            project: self.stamped(),
            path,
            normalize_project_path,
            media_files: self.document.media_files.clone(),
            copy_imported: self.settings.projects.copy_imported,
            renders: Some(self.renders_dir()),
            recordings: Some(self.recordings_dir()),
            templates: Some(self.templates_dir()),
            keep_backups: lumen_project::SaveOptions::default().keep_backups,
            state: self.document.state(),
            not_over: None,
        }));
    }
}
