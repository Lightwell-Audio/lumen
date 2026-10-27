//! Test seams for the Project media tab (M9 slice R4b): what its list and
//! cards hold, which the screen shows only as pixels.

use lumen_project::model::Id;

use crate::App;
use crate::browser::Tab;

impl App {
    /// The missing-audio banner's count, while it shows.
    pub fn missing_media_count(&self) -> Option<usize> {
        self.missing_banner()
    }

    /// Whether the browser shows the Project media tab.
    pub fn media_tab_shown(&self) -> bool {
        self.show_browser && self.browser.tab == Tab::Media
    }

    /// The tab's rows as the list shows them: name, missing, clips that use it.
    pub fn media_rows(&self) -> Vec<(String, bool, usize)> {
        self.media_pool
            .rows()
            .iter()
            .map(|r| (r.name.clone(), r.missing, r.uses))
            .collect()
    }

    /// The id of the media row named `name`.
    pub fn media_id(&self, name: &str) -> Option<Id> {
        self.media_pool
            .rows()
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.id)
    }

    /// How many files the last folder search found that can be relinked.
    pub fn media_found(&self) -> Option<usize> {
        self.media_pool.report.as_ref().map(|r| r.found.len())
    }

    /// Whether a folder search is under way.
    pub fn media_searching(&self) -> bool {
        self.media_pool.search.is_some()
    }

    /// The last thing the tab said.
    pub fn media_note(&self) -> Option<&str> {
        self.media_pool.note.as_deref()
    }

    /// How many files the last scan found that belong to no project, once
    /// a scan has answered.
    pub fn media_strays(&self) -> usize {
        self.media_pool.strays().files.len()
    }

    /// Whether Clear was pressed once and waits for the second press.
    pub fn media_clear_armed(&self) -> bool {
        self.media_pool.armed
    }
}
