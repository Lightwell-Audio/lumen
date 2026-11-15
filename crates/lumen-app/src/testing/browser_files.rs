use std::path::{Path, PathBuf};

use crate::browser::files::Place;
use crate::{App, Launch};

impl Launch {
    pub fn with_sample_folder(mut self, dir: &Path) -> Self {
        let saved = &mut self.settings.browser;
        saved.sample_folders = vec![dir.to_string_lossy().into_owned()];
        saved.music_answered = true;
        self
    }

    pub fn also_sample_folder(mut self, dir: &Path) -> Self {
        let saved = &mut self.settings.browser;
        saved
            .sample_folders
            .push(dir.to_string_lossy().into_owned());
        self
    }

    pub fn with_place(mut self, dir: &Path) -> Self {
        let name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.settings.browser.places.push(Place {
            name,
            path: dir.to_string_lossy().into_owned(),
        });
        self
    }

    pub fn with_projects_folder(mut self, dir: &Path) -> Self {
        self.settings.projects.folder = Some(dir.to_string_lossy().into_owned());
        self
    }

    pub fn without_auto_preview(mut self) -> Self {
        self.settings.browser.auto_preview = false;
        self
    }
}

impl App {
    pub fn previewing_file(&self) -> Option<PathBuf> {
        let preview = self.browser.files.preview.as_ref()?;
        preview.playing.as_ref().map(|p| p.path.clone())
    }

    pub fn preview_failure(&self) -> Option<String> {
        let preview = self.browser.files.preview.as_ref()?;
        preview.failed.as_ref().map(|(_, why)| why.clone())
    }

    pub fn preview_started_in(&self) -> Option<std::time::Duration> {
        self.browser.files.preview.as_ref()?.started_in
    }

    pub fn browser_tab_name(&self) -> &'static str {
        self.browser.tab.name()
    }

    pub fn browser_files_reading(&self) -> bool {
        let files = &self.browser.files;
        [&files.samples, &files.projects, &files.places, &files.found]
            .iter()
            .any(|l| l.reading)
    }

    pub fn browser_places(&self) -> Vec<(String, String)> {
        self.settings
            .browser
            .places
            .iter()
            .map(|p| (p.name.clone(), p.path.clone()))
            .collect()
    }

    pub fn browser_sample_folders(&self) -> Vec<String> {
        self.settings.browser.sample_folders.clone()
    }

    pub fn browser_selected_file(&self) -> Option<PathBuf> {
        self.selected_file()
    }

    pub fn auto_preview(&self) -> bool {
        self.settings.browser.auto_preview
    }
}
