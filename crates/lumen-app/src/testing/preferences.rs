//! What the E2E suite reads of the Preferences window (M9 R7).

use crate::App;
use crate::preferences::{Page, ROWS};

impl App {
    /// Whether Preferences is open, and the name of its page.
    pub fn preferences_shown(&self) -> Option<&'static str> {
        self.preferences.open.then(|| self.preferences.page.name())
    }

    /// Whether the History window is open.
    pub fn history_shown(&self) -> bool {
        self.show_history
    }

    /// The row a search result marked, by key, while its outline shows.
    pub fn marked_preference(&self) -> Option<&'static str> {
        self.preferences.marked_row()
    }

    /// Every row search can find: its key and its page's name.
    pub fn preference_rows() -> Vec<(&'static str, &'static str)> {
        ROWS.iter().map(|r| (r.key, r.page.name())).collect()
    }

    /// The plugin folders the user added, as the settings keep them.
    pub fn plugin_folders(&self) -> Vec<String> {
        self.settings.plugins.folders.clone()
    }

    /// How long after a change the autosave waits now.
    pub fn autosave_delay(&self) -> Option<std::time::Duration> {
        self.autosave.as_ref().map(|a| a.delay())
    }

    /// The undo steps the open project keeps.
    pub fn undo_limit(&self) -> usize {
        self.document.undo_limit()
    }

    /// Where a save or open dialog starts now.
    pub fn dialog_start_folder(&self) -> Option<std::path::PathBuf> {
        self.dialog_start()
    }

    /// The cache folder in use this run.
    pub fn cache_dir_in_use(&self) -> std::path::PathBuf {
        self.cache_dir()
    }

    /// The cache's limit, as the settings keep it, in gigabytes.
    pub fn cache_limit_gb(&self) -> u32 {
        self.settings.cache.limit_gb
    }

    /// The bytes the cache holds, once Preferences counted them.
    pub fn cache_size_counted(&self) -> Option<u64> {
        self.cache.size.filter(|_| !self.cache.busy())
    }

    /// Starts counting the cache, then chooses the limit `gb` in the same
    /// frame, so the limit meets a busy worker.
    pub fn choose_cache_limit_while_counting(&mut self, ctx: &eframe::egui::Context, gb: u32) {
        self.count_cache(ctx);
        self.cache_limit_chosen(ctx, gb);
    }

    /// A trim to a lower limit waits for the cache's worker.
    pub fn cache_trim_waiting(&self) -> bool {
        self.cache.trim_waiting()
    }

    /// The names of the pages, in the list's order.
    pub fn preference_pages() -> Vec<&'static str> {
        Page::ALL.map(Page::name).to_vec()
    }
}
