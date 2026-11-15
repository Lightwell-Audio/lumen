use serde::Serialize;

use crate::commands::{ALL, Area};
use crate::preferences::{Page, ROWS};

#[derive(Debug, Serialize)]
pub struct CommandDoc {
    pub id: &'static str,
    pub name: &'static str,
    pub area: &'static str,
    pub keys: Vec<Vec<&'static str>>,
    pub keys_mac: Vec<Vec<&'static str>>,
    pub about: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AreaDoc {
    pub id: &'static str,
    pub name: &'static str,
    pub light: usize,
}

#[derive(Debug, Serialize)]
pub struct PreferenceDoc {
    pub id: &'static str,
    pub name: &'static str,
    pub page: &'static str,
    pub words: &'static str,
}

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
