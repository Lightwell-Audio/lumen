//! The browser's Devices tab (M6 slice 1, ADR-0068): installed plugins and
//! Lumen's own devices, grouped by vendor, narrowed by type, category and
//! words (the owner's choice B with a type filter). `view` is what is being
//! shown, `rows` turns the catalog into rows, `list` paints them, `controls`
//! are the search field, the switch, the chips and the scan line, and
//! `panel` puts them in the pane and applies what was done. The Project media
//! tab (M9 R4b) is `media_tab`; the strip of icon tabs over them all is the
//! owner's L B (M9 R7c).

use serde::{Deserialize, Serialize};

pub(crate) mod controls;
pub(crate) mod files;
mod list;
mod media_tab;
mod panel;
pub(crate) mod rows;
pub(crate) mod view;

/// The browser's tabs, in the strip's order (M9 R7c, the owner's L B).
/// Kept in the settings by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tab {
    #[default]
    Devices,
    /// Every audio and MIDI file in the sample folders (M9 R7c).
    Samples,
    /// Recent projects and the projects folder (M9 R7c).
    Projects,
    /// Folders the person keeps sounds in, and the computer (M9 R7c).
    Places,
    /// The files the project holds (M9 R4b).
    Media,
}

impl Tab {
    /// Every tab, in the strip's order.
    pub(crate) const ALL: [Tab; 5] = [
        Tab::Devices,
        Tab::Samples,
        Tab::Projects,
        Tab::Places,
        Tab::Media,
    ];

    /// Its accessible name, as every row of tabs names its tabs.
    pub(crate) fn access(self) -> &'static str {
        match self {
            Tab::Devices => "Devices tab",
            Tab::Samples => "Samples tab",
            Tab::Projects => "Projects tab",
            Tab::Places => "Places tab",
            Tab::Media => "Project media tab",
        }
    }

    /// Its name: the tooltip and the line under the strip.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Tab::Devices => "Devices",
            Tab::Samples => "Samples",
            Tab::Projects => "Projects",
            Tab::Places => "Places",
            Tab::Media => "Project media",
        }
    }

    /// What it holds, after its name on the line under the strip.
    pub(crate) fn holds(self) -> &'static str {
        match self {
            Tab::Devices => "instruments and effects",
            Tab::Samples => "every audio and MIDI file in your sample folders",
            Tab::Projects => "recent projects and your projects folder",
            Tab::Places => "your folders and this computer",
            Tab::Media => "the audio files this project holds",
        }
    }

    /// The tab's tooltip.
    pub(crate) fn tip(self) -> &'static str {
        match self {
            Tab::Devices => "Devices: instruments and effects",
            Tab::Samples => "Samples: every audio and MIDI file in your sample folders",
            Tab::Projects => "Projects: recent projects and your projects folder",
            Tab::Places => "Places: your folders and this computer",
            Tab::Media => {
                "Project media: relink the files that moved, clear the ones nothing plays"
            }
        }
    }
}

pub(crate) use files::Saved;

/// The browser's own state: what it shows, and one-frame requests from
/// commands.
#[derive(Default)]
pub(crate) struct Browser {
    pub view: view::View,
    pub tab: Tab,
    /// A command asked for the search field's keyboard.
    pub focus_search: bool,
    /// Samples, Projects and Places (M9 R7c).
    pub files: files::Files,
}
