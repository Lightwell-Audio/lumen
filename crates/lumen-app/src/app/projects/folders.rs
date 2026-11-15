use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use super::dialogs::FileDialog;
use crate::App;

pub(crate) fn user_folder() -> Option<PathBuf> {
    documents_folder().map(|d| d.join(PRODUCT_NAME))
}

pub(crate) const PROJECTS: &str = "Projects";
pub(crate) const TEMPLATES: &str = "Templates";

pub(crate) fn documents_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents"))
}

impl App {
    pub(crate) fn dialog_start(&self) -> Option<PathBuf> {
        self.document
            .path
            .as_deref()
            .and_then(|p| p.parent()?.parent().map(PathBuf::from))
            .or_else(|| self.projects_folder())
            .or_else(documents_folder)
    }

    pub(crate) fn projects_folder(&self) -> Option<PathBuf> {
        self.settings
            .projects
            .folder
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.user_dir.as_ref().map(|d| d.join(PROJECTS)))
    }

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

    pub(crate) fn projects_folder_chosen(&mut self, folder: Option<PathBuf>) {
        let folder = folder.map(|f| f.to_string_lossy().into_owned());
        if folder != self.settings.projects.folder {
            self.settings.projects.folder = folder;
            self.settings_changed = Some(Instant::now());
        }
    }
}
