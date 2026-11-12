//! Each card's sound and the menu's state: where a sound came from, the sounds
//! a card was given, what the menu is showing, and a save under way.

use super::*;

/// What the card's field shows.
pub(crate) struct FieldLook {
    /// The sound's name, when one was chosen.
    pub name: Option<String>,
    /// The sound has been changed since it was chosen.
    pub changed: bool,
}

/// Where a chosen sound came from.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Source {
    Program { list: usize, index: usize },
    File { path: PathBuf, user: bool },
}

/// The sound a card was last given by the menu or its arrows.
pub(super) struct Current {
    pub(super) name: String,
    pub(super) source: Source,
    /// The parameter values the project held once the sound had settled; the
    /// sound is changed when they are not what the project holds now.
    pub(super) baseline: Option<BTreeMap<u32, f64>>,
    /// The device's sound revision when a file was loaded: another one means an
    /// undo or redo went across the load.
    pub(super) rev: u32,
}

/// One entry of the menu.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Choice {
    Program {
        list: usize,
        index: usize,
        name: String,
        /// The list's name, when the plugin offers more than one.
        group: Option<String>,
    },
    File {
        path: PathBuf,
        user: bool,
        name: String,
        group: Option<String>,
    },
}

impl Choice {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Program { name, .. } | Self::File { name, .. } => name,
        }
    }

    pub(super) fn group(&self) -> Option<&str> {
        match self {
            Self::Program { group, .. } | Self::File { group, .. } => group.as_deref(),
        }
    }

    pub(super) fn heading(&self) -> String {
        let (base, group) = match self {
            Self::Program { group, .. } => ("Factory", group),
            Self::File {
                user: true, group, ..
            } => ("Your presets", group),
            Self::File { group, .. } => ("System presets", group),
        };
        match group {
            Some(group) => format!("{base} \u{b7} {group}"),
            None => base.to_owned(),
        }
    }

    pub(super) fn is_current(&self, current: &Current) -> bool {
        match (self, &current.source) {
            (Self::Program { list, index, .. }, Source::Program { list: l, index: i }) => {
                list == l && index == i
            }
            (Self::File { path, .. }, Source::File { path: p, .. }) => path == p,
            _ => false,
        }
    }
}

/// A save waiting for the plugin to be read.
pub(super) struct Saving {
    pub(super) device: Id,
    pub(super) name: String,
    /// The file to write over (Save), or none for an unused name (Save as).
    pub(super) over: Option<PathBuf>,
    pub(super) since: Instant,
}

#[derive(Default)]
pub(super) struct MenuState {
    pub(super) open_for: Option<Id>,
    pub(super) search: String,
    /// The name being typed for Save as….
    pub(super) naming: Option<String>,
    pub(super) focus: Focus,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    #[default]
    None,
    Search,
    Name,
}

/// The cards' preset state.
#[derive(Default)]
pub(crate) struct Presets {
    /// Where preset files are kept.
    pub(crate) places: Places,
    pub(super) files: Files,
    /// The files found for each device's plugin.
    pub(super) listed: HashMap<Id, Vec<Entry>>,
    /// Devices whose folders have been asked about.
    pub(super) asked: HashSet<Id>,
    pub(super) current: HashMap<Id, Current>,
    /// A file being read for a device: its name, path and whether it is the
    /// user's.
    pub(super) reading: HashMap<Id, (String, PathBuf, bool)>,
    pub(super) menu: MenuState,
    pub(super) saving: Option<Saving>,
    /// The folders shown to the person, newest last (tests read it; with fixed
    /// places no window opens).
    pub(crate) revealed: Vec<PathBuf>,
}

impl Presets {
    pub(crate) fn new(places: Places) -> Self {
        Self {
            places,
            ..Self::default()
        }
    }

    /// Asks again for the files of the plugin of `device`.
    pub(super) fn relist(&mut self, ctx: &egui::Context, device: Id, vendor: &str, plugin: &str) {
        self.asked.insert(device);
        let folders = self.places.folders(vendor, plugin);
        if folders.is_empty() {
            self.listed.entry(device).or_default();
        } else {
            self.files.list(ctx, device, folders);
        }
    }

    /// Gives `device` a chosen sound and a list of files, as if they had been
    /// found (screenshots).
    pub(crate) fn pretend(
        &mut self,
        device: Id,
        name: &str,
        changed: Option<BTreeMap<u32, f64>>,
        files: &[(Option<&str>, &str, bool)],
    ) {
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::Program { list: 0, index: 0 },
                baseline: changed,
                rev: 0,
            },
        );
        self.asked.insert(device);
        self.listed.insert(
            device,
            files
                .iter()
                .map(|(group, name, user)| Entry {
                    name: (*name).to_owned(),
                    group: group.map(str::to_owned),
                    path: PathBuf::from(format!("/presets/{name}.vstpreset")),
                    user: *user,
                })
                .collect(),
        );
    }

    /// Records that a program was chosen for `device`.
    pub(crate) fn chose_program(
        &mut self,
        device: Id,
        name: &str,
        list: usize,
        index: usize,
        rev: u32,
    ) {
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::Program { list, index },
                baseline: None,
                rev,
            },
        );
    }

    /// A preset the plugin took is the project's now.
    pub(crate) fn loaded(&mut self, device: Id, name: &str, rev: u32) {
        let Some((reading, path, user)) = self.reading.remove(&device) else {
            // Loaded by a seam or a command: a name, no file.
            self.current.remove(&device);
            return;
        };
        let name = if reading == name { name } else { &reading };
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::File { path, user },
                baseline: None,
                rev,
            },
        );
    }

    /// A preset was refused: nothing was chosen.
    pub(crate) fn refused(&mut self, device: Id) {
        self.reading.remove(&device);
    }
}
