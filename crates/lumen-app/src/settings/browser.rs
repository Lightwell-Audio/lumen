//! What the browser keeps in the settings (M9 R7c): an added section with
//! defaults, so older files read as before (no version change). The project
//! file does not change. Plain data: the browser converts its own types to
//! and from these (M9 review M3), so the file does not change when the
//! interface does.

use serde::{Deserialize, Serialize};

/// The browser tab that was open, kept by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tab {
    #[default]
    Devices,
    Samples,
    Projects,
    Places,
    Media,
}

/// What `place_folder` holds while Places shows Computer.
pub(crate) const COMPUTER: &str = ":computer";

/// The level previews start at.
pub(crate) const PREVIEW_DB: f32 = -6.0;

/// A folder the person added to Places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Place {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Browser {
    /// The tab that was open.
    pub tab: Tab,
    /// The sample folders; none means the system's Music folder.
    pub sample_folders: Vec<String>,
    /// The first visit's question (Choose a folder… or Keep Music) was
    /// answered.
    pub music_answered: bool,
    pub places: Vec<Place>,
    /// The folder Places showed: a path, [`COMPUTER`], or `None` for the top.
    pub place_folder: Option<String>,
    /// Choosing a file plays it (the owner's P A).
    pub auto_preview: bool,
    /// The preview's level, in dB.
    pub preview_db: f32,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            tab: Tab::default(),
            sample_folders: Vec::new(),
            music_answered: false,
            places: Vec::new(),
            place_folder: None,
            auto_preview: true,
            preview_db: PREVIEW_DB,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_settings_file_reads_with_the_defaults() {
        let saved: Browser = serde_json::from_str("{}").unwrap();
        assert_eq!(saved, Browser::default());
        assert!(saved.auto_preview);
        assert_eq!(saved.preview_db, PREVIEW_DB);
        let saved: Browser = serde_json::from_str(r#"{"tab": "samples"}"#).unwrap();
        assert_eq!(saved.tab, Tab::Samples);
    }

    #[test]
    fn places_and_folders_read_back() {
        let saved = Browser {
            tab: Tab::Places,
            sample_folders: vec!["D:/Samples".into()],
            music_answered: true,
            places: vec![Place {
                name: "Drums".into(),
                path: "D:/Drums".into(),
            }],
            place_folder: Some(COMPUTER.into()),
            auto_preview: false,
            preview_db: -12.0,
        };
        let text = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Browser>(&text).unwrap(), saved);
    }
}
