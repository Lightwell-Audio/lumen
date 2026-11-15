use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    pub scan_at_startup: bool,
    pub folders: Vec<String>,
    pub favorites: Vec<String>,
    pub recents: Vec<String>,
    pub editors: BTreeMap<String, EditorPlace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorPlace {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

impl From<lumen_hosting::EditorBounds> for EditorPlace {
    fn from(b: lumen_hosting::EditorBounds) -> Self {
        Self {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
            dpi: b.dpi,
        }
    }
}

impl From<EditorPlace> for lumen_hosting::EditorBounds {
    fn from(p: EditorPlace) -> Self {
        Self {
            x: p.x,
            y: p.y,
            width: p.width,
            height: p.height,
            dpi: p.dpi,
        }
    }
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            scan_at_startup: true,
            folders: Vec::new(),
            favorites: Vec::new(),
            recents: Vec::new(),
            editors: BTreeMap::new(),
        }
    }
}

impl PluginSettings {
    pub const MAX_RECENTS: usize = 12;

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

    pub fn touch(&mut self, id: &str) {
        self.recents.retain(|r| r != id);
        self.recents.insert(0, id.to_owned());
        self.recents.truncate(Self::MAX_RECENTS);
    }

    pub const MAX_EDITORS: usize = 500;

    pub fn remember_editor(&mut self, class: &str, place: EditorPlace) -> bool {
        if place.width == 0 || place.height == 0 || place.width > 16_384 || place.height > 16_384 {
            return false;
        }
        if self.editors.get(class) == Some(&place) {
            return false;
        }
        if self.editors.len() >= Self::MAX_EDITORS && !self.editors.contains_key(class) {
            return false;
        }
        self.editors.insert(class.to_owned(), place);
        true
    }

    pub const MAX_LISTED: usize = 4_096;

    pub fn bound(&mut self) {
        self.folders.truncate(Self::MAX_LISTED);
        self.favorites.truncate(Self::MAX_LISTED);
        self.recents.truncate(Self::MAX_RECENTS);
        while self.editors.len() > Self::MAX_EDITORS {
            self.editors.pop_last();
        }
    }

    pub fn add_folder(&mut self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        if self.folders.contains(&text) {
            return false;
        }
        self.folders.push(text);
        true
    }

    pub fn remove_folder(&mut self, folder: &str) -> bool {
        let before = self.folders.len();
        self.folders.retain(|f| f != folder);
        self.folders.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_added_can_be_removed_once() {
        let mut s = PluginSettings::default();
        assert!(s.add_folder(Path::new("D:/Plugins")));
        assert!(s.add_folder(Path::new("E:/More")));
        assert!(s.remove_folder("D:/Plugins"));
        assert_eq!(s.folders, ["E:/More"]);
        assert!(!s.remove_folder("D:/Plugins"), "already gone");
    }

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
    fn a_windows_place_is_kept_by_class_and_only_when_it_is_one() {
        let mut s = PluginSettings::default();
        let place = EditorPlace {
            x: -1200,
            y: 80,
            width: 640,
            height: 480,
            dpi: 144,
        };
        assert!(s.remember_editor("vst3:AB", place));
        assert!(!s.remember_editor("vst3:AB", place), "unchanged");
        let moved = EditorPlace { x: 10, ..place };
        assert!(s.remember_editor("vst3:AB", moved));
        assert_eq!(s.editors["vst3:AB"], moved);
        let flat = EditorPlace { height: 0, ..place };
        assert!(!s.remember_editor("vst3:CD", flat));
        assert!(!s.editors.contains_key("vst3:CD"));
        let text = serde_json::to_string(&s).unwrap();
        let back: PluginSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.editors["vst3:AB"], moved);
    }

    #[test]
    fn a_file_with_huge_lists_is_cut_to_what_is_kept() {
        let mut s = PluginSettings {
            folders: (0..10_000).map(|i| format!("/f{i}")).collect(),
            favorites: (0..10_000).map(|i| format!("vst3:{i}")).collect(),
            recents: (0..100).map(|i| format!("vst3:{i}")).collect(),
            ..PluginSettings::default()
        };
        let place = EditorPlace {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            dpi: 96,
        };
        s.editors = (0..2_000).map(|i| (format!("c{i}"), place)).collect();
        s.bound();
        assert_eq!(s.folders.len(), PluginSettings::MAX_LISTED);
        assert_eq!(s.favorites.len(), PluginSettings::MAX_LISTED);
        assert_eq!(s.recents.len(), PluginSettings::MAX_RECENTS);
        assert_eq!(s.editors.len(), PluginSettings::MAX_EDITORS);
        assert_eq!(s.folders[0], "/f0", "the first are kept");
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
