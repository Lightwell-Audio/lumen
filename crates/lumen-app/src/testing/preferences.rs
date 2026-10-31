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

    /// The names of the pages, in the list's order.
    pub fn preference_pages() -> Vec<&'static str> {
        Page::ALL.map(Page::name).to_vec()
    }
}
