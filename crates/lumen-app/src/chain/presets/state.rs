use super::*;

pub(crate) struct FieldLook {
    pub name: Option<String>,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Source {
    Program { list: usize, index: usize },
    Factory { id: &'static str },
    File { path: PathBuf, user: bool },
}

pub(super) struct Current {
    pub(super) name: String,
    pub(super) source: Source,
    pub(super) baseline: Option<BTreeMap<u32, f64>>,
    pub(super) rev: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Choice {
    Program {
        list: usize,
        index: usize,
        name: String,
        group: Option<String>,
    },
    Factory {
        preset: lumen_devices::Preset,
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
            Self::Factory { preset } => preset.name(),
        }
    }

    pub(super) fn group(&self) -> Option<&str> {
        match self {
            Self::Program { group, .. } | Self::File { group, .. } => group.as_deref(),
            Self::Factory { .. } => None,
        }
    }

    pub(super) fn heading(&self) -> String {
        let (base, group) = match self {
            Self::Program { group, .. } => ("Factory", group),
            Self::Factory { .. } => ("Factory", &None),
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
            (Self::Factory { preset }, Source::Factory { id }) => preset.id() == *id,
            (Self::File { path, .. }, Source::File { path: p, .. }) => path == p,
            _ => false,
        }
    }
}

pub(super) struct Saving {
    pub(super) device: Id,
    pub(super) name: String,
    pub(super) over: Option<PathBuf>,
    pub(super) since: Instant,
}

#[derive(Default)]
pub(super) struct MenuState {
    pub(super) open_for: Option<Id>,
    pub(super) search: String,
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

#[derive(Default)]
pub(crate) struct Presets {
    pub(crate) places: Places,
    pub(super) files: Files,
    pub(super) listed: HashMap<Id, Vec<Entry>>,
    pub(super) asked: HashSet<Id>,
    pub(super) current: HashMap<Id, Current>,
    pub(super) reading: HashMap<Id, (String, PathBuf, bool)>,
    pub(super) menu: MenuState,
    pub(super) saving: Option<Saving>,
    pub(crate) revealed: Vec<PathBuf>,
}

impl Presets {
    pub(crate) fn new(places: Places) -> Self {
        Self {
            places,
            ..Self::default()
        }
    }

    pub(super) fn relist(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        folders: Vec<(PathBuf, bool)>,
    ) {
        self.asked.insert(device);
        if folders.is_empty() {
            self.listed.entry(device).or_default();
        } else {
            self.files.list(ctx, device, folders);
        }
    }

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

    pub(crate) fn chose_factory(&mut self, device: Id, preset: lumen_devices::Preset, rev: u32) {
        self.current.insert(
            device,
            Current {
                name: preset.name().to_owned(),
                source: Source::Factory { id: preset.id() },
                baseline: None,
                rev,
            },
        );
    }

    pub(crate) fn loaded(&mut self, device: Id, name: &str, rev: u32) {
        let Some((reading, path, user)) = self.reading.remove(&device) else {
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

    pub(crate) fn refused(&mut self, device: Id) {
        self.reading.remove(&device);
    }
}
