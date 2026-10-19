//! What the user keeps about plugins, in the app settings file
//! (`settings.plugins`): the folders to search besides the standard ones,
//! favorites, recently chosen devices and whether to scan at start. An
//! added section with defaults, so older files read as before (no version
//! change).

use std::collections::{BTreeMap, HashSet};
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
    /// Where each plugin's own window was last, by class id (M6 slice 4f): it
    /// opens there next time, whatever the project.
    pub editors: BTreeMap<String, EditorPlace>,
}

/// A plugin window's place: its outer top-left corner and the size of its
/// content, in physical pixels at `dpi`.
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

    /// The most places kept (a library has hundreds of plugins; a person has
    /// opened some dozens of windows).
    pub const MAX_EDITORS: usize = 500;

    /// Remembers where `class`'s window is. True when that changed what is
    /// kept. A window with no size or an absurd one is not a place.
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

    /// The most folders, favorites and recents a file may hold.
    pub const MAX_LISTED: usize = 4_096;

    /// Cuts what a file holds to what the app keeps: a damaged or crafted
    /// settings file cannot make the lists grow without end.
    pub fn bound(&mut self) {
        self.folders.truncate(Self::MAX_LISTED);
        self.favorites.truncate(Self::MAX_LISTED);
        self.recents.truncate(Self::MAX_RECENTS);
        while self.editors.len() > Self::MAX_EDITORS {
            self.editors.pop_last();
        }
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
        // And it survives the file.
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
