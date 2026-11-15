use serde::{Deserialize, Serialize};

pub(crate) mod controls;
pub(crate) mod files;
pub(crate) mod list;
pub(crate) mod media_tab;
pub(crate) mod panel;
pub(crate) mod rows;
pub(crate) mod view;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tab {
    #[default]
    Devices,
    Samples,
    Projects,
    Places,
    Media,
}

impl From<crate::settings::browser::Tab> for Tab {
    fn from(kept: crate::settings::browser::Tab) -> Self {
        use crate::settings::browser::Tab as Kept;
        match kept {
            Kept::Devices => Tab::Devices,
            Kept::Samples => Tab::Samples,
            Kept::Projects => Tab::Projects,
            Kept::Places => Tab::Places,
            Kept::Media => Tab::Media,
        }
    }
}

impl From<Tab> for crate::settings::browser::Tab {
    fn from(tab: Tab) -> Self {
        match tab {
            Tab::Devices => Self::Devices,
            Tab::Samples => Self::Samples,
            Tab::Projects => Self::Projects,
            Tab::Places => Self::Places,
            Tab::Media => Self::Media,
        }
    }
}

impl Tab {
    pub(crate) const ALL: [Tab; 5] = [
        Tab::Devices,
        Tab::Samples,
        Tab::Projects,
        Tab::Places,
        Tab::Media,
    ];

    pub(crate) fn access(self) -> &'static str {
        match self {
            Tab::Devices => "Devices tab",
            Tab::Samples => "Samples tab",
            Tab::Projects => "Projects tab",
            Tab::Places => "Places tab",
            Tab::Media => "Project media tab",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tab::Devices => "Devices",
            Tab::Samples => "Samples",
            Tab::Projects => "Projects",
            Tab::Places => "Places",
            Tab::Media => "Project media",
        }
    }

    pub(crate) fn holds(self) -> &'static str {
        match self {
            Tab::Devices => "instruments and effects",
            Tab::Samples => "every audio and MIDI file in your sample folders",
            Tab::Projects => "recent projects and your projects folder",
            Tab::Places => "your folders and this computer",
            Tab::Media => "the audio files this project holds",
        }
    }

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

#[derive(Default)]
pub(crate) struct Browser {
    pub view: view::View,
    pub tab: Tab,
    pub focus_search: bool,
    pub files: files::Files,
}

#[cfg(test)]
mod tests {
    use super::Tab;

    #[test]
    fn every_tab_is_kept_in_the_settings_and_read_back() {
        for tab in Tab::ALL {
            let kept: crate::settings::browser::Tab = tab.into();
            assert_eq!(Tab::from(kept), tab);
        }
    }
}
