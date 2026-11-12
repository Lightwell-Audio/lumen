//! The folders the person's projects and templates are kept in.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use super::dialogs::FileDialog;
use crate::App;

/// The folder the person's own projects and templates are kept in:
/// `Documents/Lumen`, with `Projects` and `Templates` inside it. One named
/// folder, so a project and a template are told apart by where they are
/// (owner, MT-PROJ-1, 2026-10-26).
pub(crate) fn user_folder() -> Option<PathBuf> {
    documents_folder().map(|d| d.join(PRODUCT_NAME))
}

/// The folder inside the user folder that holds the projects.
pub(crate) const PROJECTS: &str = "Projects";
/// The folder inside the user folder that holds the templates.
pub(crate) const TEMPLATES: &str = "Templates";

/// Where file dialogs start when the project has no folder yet.
pub(crate) fn documents_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents"))
}

impl App {
    /// Where a save or open dialog starts: beside the folder the project is
    /// in, or in the user folder's Projects (`Documents/Lumen/Projects`).
    pub(crate) fn dialog_start(&self) -> Option<PathBuf> {
        self.document
            .path
            .as_deref()
            .and_then(|p| p.parent()?.parent().map(PathBuf::from))
            .or_else(|| self.projects_folder())
            .or_else(documents_folder)
    }

    /// The projects folder: the one chosen in Preferences, or the user
    /// folder's Projects (`Documents/Lumen/Projects`).
    pub(crate) fn projects_folder(&self) -> Option<PathBuf> {
        self.settings
            .projects
            .folder
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.user_dir.as_ref().map(|d| d.join(PROJECTS)))
    }

    /// Asks for the projects folder (Preferences, Files).
    pub(crate) fn pick_projects_folder(&mut self, ctx: &egui::Context) {
        let owner = self.owner;
        let start = self.projects_folder();
        let show = move || {
            let mut dialog = rfd::FileDialog::new().set_title("Choose the projects folder");
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::ProjectsFolder(dialog.pick_folder())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::ProjectsFolder(paths.into_iter().next())
        });
    }

    /// The projects folder was chosen (`None`: the standard one). Projects
    /// already saved stay where they are.
    pub(crate) fn projects_folder_chosen(&mut self, folder: Option<PathBuf>) {
        let folder = folder.map(|f| f.to_string_lossy().into_owned());
        if folder != self.settings.projects.folder {
            self.settings.projects.folder = folder;
            self.settings_changed = Some(Instant::now());
        }
    }
}
