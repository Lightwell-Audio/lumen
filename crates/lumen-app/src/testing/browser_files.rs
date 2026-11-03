//! Test seams for the browser's file tabs (M9 R7c): what the preview plays
//! and what the tabs keep, which the screen shows only as pixels.

use std::path::{Path, PathBuf};

use crate::browser::files::Place;
use crate::{App, Launch};

impl Launch {
    /// Starts with `dir` as the one sample folder, the first visit's
    /// question answered (a test never walks the machine's Music folder).
    pub fn with_sample_folder(mut self, dir: &Path) -> Self {
        let saved = &mut self.settings.browser;
        saved.sample_folders = vec![dir.to_string_lossy().into_owned()];
        saved.music_answered = true;
        self
    }

    /// Adds `dir` to the sample folders (after [`Self::with_sample_folder`]).
    pub fn also_sample_folder(mut self, dir: &Path) -> Self {
        let saved = &mut self.settings.browser;
        saved
            .sample_folders
            .push(dir.to_string_lossy().into_owned());
        self
    }

    /// Starts with `dir` as a place, named after its folder.
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

    /// Starts with `dir` as the projects folder, which Projects lists.
    pub fn with_projects_folder(mut self, dir: &Path) -> Self {
        self.settings.projects.folder = Some(dir.to_string_lossy().into_owned());
        self
    }

    /// Starts with Auto preview off.
    pub fn without_auto_preview(mut self) -> Self {
        self.settings.browser.auto_preview = false;
        self
    }
}

impl App {
    /// The file the browser's preview is playing.
    pub fn previewing_file(&self) -> Option<PathBuf> {
        let preview = self.browser.files.preview.as_ref()?;
        preview.playing.as_ref().map(|p| p.path.clone())
    }

    /// Why the last preview did not play.
    pub fn preview_failure(&self) -> Option<String> {
        let preview = self.browser.files.preview.as_ref()?;
        preview.failed.as_ref().map(|(_, why)| why.clone())
    }

    /// From the click to the first frame heard, the last preview.
    pub fn preview_started_in(&self) -> Option<std::time::Duration> {
        self.browser.files.preview.as_ref()?.started_in
    }

    /// The browser's open tab, by name ("Samples").
    pub fn browser_tab_name(&self) -> &'static str {
        self.browser.tab.name()
    }

    /// Whether any of the file tabs is still reading.
    pub fn browser_files_reading(&self) -> bool {
        let files = &self.browser.files;
        [&files.samples, &files.projects, &files.places, &files.found]
            .iter()
            .any(|l| l.reading)
    }

    /// The places kept, as (name, path).
    pub fn browser_places(&self) -> Vec<(String, String)> {
        self.settings
            .browser
            .places
            .iter()
            .map(|p| (p.name.clone(), p.path.clone()))
            .collect()
    }

    /// The sample folders kept.
    pub fn browser_sample_folders(&self) -> Vec<String> {
        self.settings.browser.sample_folders.clone()
    }

    /// The chosen row of the open file tab.
    pub fn browser_selected_file(&self) -> Option<PathBuf> {
        self.selected_file()
    }

    /// Whether Auto preview is on.
    pub fn auto_preview(&self) -> bool {
        self.settings.browser.auto_preview
    }
}
