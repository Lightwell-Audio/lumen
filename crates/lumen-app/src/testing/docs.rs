//! The tables the user documentation is generated from (docs/USER_DOCS.md
//! §5.1): every command with its default keys on both systems, and every
//! Preferences row. `tests/docs_data.rs` writes them to the site's data
//! folder; nothing here is used by the app itself.

use serde::Serialize;

use crate::commands::{ALL, Area};
use crate::preferences::{Page, ROWS};

/// A command as the docs show it.
#[derive(Debug, Serialize)]
pub struct CommandDoc {
    /// Stable: the reference's anchor and the key pages list it by.
    pub id: &'static str,
    pub name: &'static str,
    /// The area's id ("transport"), which picks its colour on the site.
    pub area: &'static str,
    /// The default shortcuts on Windows and Linux, each as its keycaps
    /// (["Ctrl", "E"]); the first is the one shown.
    pub keys: Vec<Vec<&'static str>>,
    /// The same on macOS (["Cmd", "E"]).
    pub keys_mac: Vec<Vec<&'static str>>,
    pub about: &'static str,
}

/// An area of commands: its id, name and the track colour of its light
/// (1 to 12, the design system's `clip-n`).
#[derive(Debug, Serialize)]
pub struct AreaDoc {
    pub id: &'static str,
    pub name: &'static str,
    pub light: usize,
}

/// A Preferences row search finds.
#[derive(Debug, Serialize)]
pub struct PreferenceDoc {
    /// Stable: the reference's anchor ("audio.driver").
    pub id: &'static str,
    pub name: &'static str,
    /// The page's name ("Audio").
    pub page: &'static str,
    pub words: &'static str,
}

/// A Preferences page: its name and what it holds.
#[derive(Debug, Serialize)]
pub struct PageDoc {
    pub name: &'static str,
    pub about: &'static str,
}

const AREAS: [Area; 11] = [
    Area::Transport,
    Area::Edit,
    Area::Clip,
    Area::Track,
    Area::Mixer,
    Area::View,
    Area::Grid,
    Area::Locators,
    Area::Midi,
    Area::File,
    Area::Devices,
];

/// Every command, in the palette's order.
pub fn commands() -> Vec<CommandDoc> {
    ALL.iter()
        .map(|info| CommandDoc {
            id: info.id,
            name: info.name,
            area: info.area.id(),
            keys: info.keys.iter().map(|c| c.caps_on(false)).collect(),
            keys_mac: info.keys.iter().map(|c| c.caps_on(true)).collect(),
            about: info.about,
        })
        .collect()
}

/// Every area, in the palette's order.
pub fn areas() -> Vec<AreaDoc> {
    AREAS
        .iter()
        .map(|area| AreaDoc {
            id: area.id(),
            name: area.name(),
            light: area.light_index() + 1,
        })
        .collect()
}

/// Every Preferences row, page by page.
pub fn preferences() -> Vec<PreferenceDoc> {
    ROWS.iter()
        .map(|row| PreferenceDoc {
            id: row.key,
            name: row.name,
            page: row.page.name(),
            words: row.words,
        })
        .collect()
}

/// The Preferences pages, in the window's order.
pub fn pages() -> Vec<PageDoc> {
    Page::ALL
        .iter()
        .map(|page| PageDoc {
            name: page.name(),
            about: page.tip(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_is_listed_once() {
        let mut ids: Vec<_> = areas().iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AREAS.len());
        for info in ALL {
            assert!(
                AREAS.contains(&info.area),
                "{} has an unlisted area",
                info.id
            );
        }
    }

    #[test]
    fn keys_are_written_for_each_system() {
        let split = commands()
            .into_iter()
            .find(|c| c.id == "midi.split_notes")
            .expect("Split is a command");
        assert_eq!(split.keys, [["Ctrl", "E"]]);
        assert_eq!(split.keys_mac, [["Cmd", "E"]]);
    }
}
