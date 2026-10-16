//! What the user keeps about plugins, in the app settings file
//! (`settings.plugins`): the folders to search besides the standard ones,
//! favorites, recently chosen devices and whether to scan at start. An
//! added section with defaults, so older files read as before (no version
//! change).

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Scan in the background when Lumen starts (the owner's choice).
    pub scan_at_startup: bool,
    /// Folders searched besides the standard ones.
    pub folders: Vec<String>,
    /// Device ids (`vst3:<class id>`, `builtin:synth`), in the order added.
    pub favorites: Vec<String>,
    /// Device ids, most recent first, at most [`Self::MAX_RECENTS`].
    pub recents: Vec<String>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            scan_at_startup: true,
            folders: Vec::new(),
            favorites: Vec::new(),
            recents: Vec::new(),
        }
    }
}

impl PluginSettings {
    pub const MAX_RECENTS: usize = 12;

    /// Adds `id` to the favorites or removes it; true if it is one now.
    pub fn toggle_favorite(&mut self, id: &str) -> bool {
        if let Some(at) = self.favorites.iter().position(|f| f == id) {
            self.favorites.remove(at);
            false
        } else {
            self.favorites.push(id.to_owned());
            true
        }
    }

    pub fn favorite_set(&self) -> HashSet<String> {
        self.favorites.iter().cloned().collect()
    }

    /// Puts `id` first among the recents.
    pub fn touch(&mut self, id: &str) {
        self.recents.retain(|r| r != id);
        self.recents.insert(0, id.to_owned());
        self.recents.truncate(Self::MAX_RECENTS);
    }

    /// Adds a search folder; false if it is already there.
    pub fn add_folder(&mut self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        if self.folders.contains(&text) {
            return false;
        }
        self.folders.push(text);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_toggle_and_keep_their_order() {
        let mut s = PluginSettings::default();
        assert!(s.toggle_favorite("a"));
        assert!(s.toggle_favorite("b"));
        assert_eq!(s.favorites, ["a", "b"]);
        assert!(!s.toggle_favorite("a"));
        assert_eq!(s.favorites, ["b"]);
    }

    #[test]
    fn recents_are_most_recent_first_without_repeats_and_capped() {
        let mut s = PluginSettings::default();
        for i in 0..20 {
            s.touch(&format!("d{i}"));
        }
        s.touch("d15");
        assert_eq!(s.recents.len(), PluginSettings::MAX_RECENTS);
        assert_eq!(s.recents[0], "d15");
        assert_eq!(s.recents.iter().filter(|r| *r == "d15").count(), 1);
        assert_eq!(s.recents[1], "d19");
    }

    #[test]
    fn a_folder_is_added_once() {
        let mut s = PluginSettings::default();
        assert!(s.add_folder(Path::new("/plugins")));
        assert!(!s.add_folder(Path::new("/plugins")));
        assert_eq!(s.folders, ["/plugins"]);
    }

    #[test]
    fn an_older_file_without_the_section_reads_with_defaults() {
        let s: PluginSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, PluginSettings::default());
        assert!(s.scan_at_startup);
        let s: PluginSettings = serde_json::from_str(r#"{"favorites":["x"],"future":1}"#).unwrap();
        assert_eq!(s.favorites, ["x"]);
    }
}
