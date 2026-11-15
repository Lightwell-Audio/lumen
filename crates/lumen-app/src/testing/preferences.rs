use crate::App;
use crate::preferences::{Page, ROWS};

impl App {
    pub fn preferences_shown(&self) -> Option<&'static str> {
        self.preferences.open.then(|| self.preferences.page.name())
    }

    pub fn history_shown(&self) -> bool {
        self.show_history
    }

    pub fn marked_preference(&self) -> Option<&'static str> {
        self.preferences.marked_row()
    }

    pub fn preference_rows() -> Vec<(&'static str, &'static str)> {
        ROWS.iter().map(|r| (r.key, r.page.name())).collect()
    }

    pub fn plugin_folders(&self) -> Vec<String> {
        self.settings.plugins.folders.clone()
    }

    pub fn autosave_delay(&self) -> Option<std::time::Duration> {
        self.autosave.as_ref().map(|a| a.delay())
    }

    pub fn undo_limit(&self) -> usize {
        self.document.undo_limit()
    }

    pub fn dialog_start_folder(&self) -> Option<std::path::PathBuf> {
        self.dialog_start()
    }

    pub fn cache_dir_in_use(&self) -> std::path::PathBuf {
        self.cache_dir()
    }

    pub fn cache_limit_gb(&self) -> u32 {
        self.settings.cache.limit_gb
    }

    pub fn cache_size_counted(&self) -> Option<u64> {
        self.cache.size.filter(|_| !self.cache.busy())
    }

    pub fn choose_cache_limit_while_counting(&mut self, ctx: &eframe::egui::Context, gb: u32) {
        self.count_cache(ctx);
        self.cache_limit_chosen(ctx, gb);
    }

    pub fn cache_trim_waiting(&self) -> bool {
        self.cache.trim_waiting()
    }

    pub fn preference_pages() -> Vec<&'static str> {
        Page::ALL.map(Page::name).to_vec()
    }
}
