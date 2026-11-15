use lumen_project::model::Id;

use crate::App;
use crate::browser::Tab;

impl App {
    pub fn missing_media_count(&self) -> Option<usize> {
        self.missing_banner()
    }

    pub fn media_tab_shown(&self) -> bool {
        self.show_browser && self.browser.tab == Tab::Media
    }

    pub fn media_rows(&self) -> Vec<(String, bool, usize)> {
        self.media_pool
            .rows()
            .iter()
            .map(|r| (r.name.clone(), r.missing, r.uses))
            .collect()
    }

    pub fn media_id(&self, name: &str) -> Option<Id> {
        self.media_pool
            .rows()
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.id)
    }

    pub fn media_found(&self) -> Option<usize> {
        self.media_pool.report.as_ref().map(|r| r.found.len())
    }

    pub fn media_searching(&self) -> bool {
        self.media_pool.search.is_some()
    }

    pub fn media_note(&self) -> Option<&str> {
        self.media_pool.note.as_deref()
    }

    pub fn media_strays(&self) -> usize {
        self.media_pool.strays().files.len()
    }

    pub fn media_clear_armed(&self) -> bool {
        self.media_pool.armed
    }
}
