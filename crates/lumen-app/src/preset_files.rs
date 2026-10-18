//! Preset files on disk (M6 slice 5f): where a plugin's `.vstpreset` files
//! are kept, finding them, reading one and writing one, all off the UI
//! thread. The bytes are turned into a sound by `lumen_hosting::Preset`; this
//! module only moves them to and from the disk and back to the frame loop,
//! which polls each frame.
//!
//! The standard says where presets live: a folder per vendor and plugin in
//! the user's presets folder and in the system's. Saving writes to the user's.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use lumen_hosting::{MAX_FILE, Preset};
use lumen_project::model::Id;

/// The extension of a preset file.
pub const EXTENSION: &str = "vstpreset";
/// The deepest a plugin's folder is searched, and the most files listed.
const DEPTH: usize = 4;
const MOST_FILES: usize = 2000;
/// The longest name Lumen gives a preset it saves, in characters: a name that
/// fits the card's field and a file list.
pub const NAME_LIMIT: usize = 30;
/// The longest a file or folder name is made (a plugin's own name may be long).
const LONGEST_NAME: usize = 80;

/// Where preset files are kept: the user's folder (saving writes here) and
/// the system's, which are only read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    pub user: Option<PathBuf>,
    pub system: Vec<PathBuf>,
    /// Chosen by the caller (tests): no window is opened on these.
    pub fixed: bool,
}

impl Places {
    /// This machine's standard folders.
    pub fn standard() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if cfg!(windows) {
            Self {
                user: var("USERPROFILE").map(|p| p.join("Documents").join("VST3 Presets")),
                system: var("PROGRAMDATA")
                    .map(|p| p.join("VST3 Presets"))
                    .into_iter()
                    .collect(),
                fixed: false,
            }
        } else if cfg!(target_os = "macos") {
            Self {
                user: var("HOME").map(|p| p.join("Library/Audio/Presets")),
                system: vec![PathBuf::from("/Library/Audio/Presets")],
                fixed: false,
            }
        } else {
            Self {
                user: var("HOME").map(|p| p.join(".vst3/presets")),
                system: vec![
                    PathBuf::from("/usr/share/vst3/presets"),
                    PathBuf::from("/usr/local/share/vst3/presets"),
                ],
                fixed: false,
            }
        }
    }

    /// Folders chosen by the caller (tests).
    pub fn fixed(user: PathBuf, system: Vec<PathBuf>) -> Self {
        Self {
            user: Some(user),
            system,
            fixed: true,
        }
    }

    /// The folder where `vendor`'s `plugin` keeps its presets in the user's
    /// place, if there is one.
    pub fn user_folder(&self, vendor: &str, plugin: &str) -> Option<PathBuf> {
        self.user
            .as_ref()
            .map(|root| plugin_folder(root, vendor, plugin))
    }

    /// Every folder that may hold the plugin's presets, the user's first.
    pub fn folders(&self, vendor: &str, plugin: &str) -> Vec<(PathBuf, bool)> {
        let mut out = Vec::new();
        if let Some(user) = &self.user {
            out.push((plugin_folder(user, vendor, plugin), true));
        }
        for root in &self.system {
            out.push((plugin_folder(root, vendor, plugin), false));
        }
        out
    }
}

fn plugin_folder(root: &Path, vendor: &str, plugin: &str) -> PathBuf {
    root.join(safe_name(vendor, "Unknown vendor"))
        .join(safe_name(plugin, "Plugin"))
}

/// A name fit to be a file or folder name on any system: no separators or
/// reserved characters, no dots or spaces at the ends, not too long;
/// `fallback` when nothing is left.
pub fn safe_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join(" ");
    let trimmed: String = joined
        .trim_matches(|c: char| c == '.' || c == ' ')
        .chars()
        .take(LONGEST_NAME)
        .collect();
    let trimmed = trimmed.trim_end_matches(['.', ' ']).to_owned();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed
    }
}

/// A preset file found in a plugin's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file's name without its extension.
    pub name: String,
    /// The folder inside the plugin's folder it was found in, when it was in
    /// one ("Bass" for `Bass/Warm.vstpreset`).
    pub group: Option<String>,
    pub path: PathBuf,
    /// In the user's folder (so Save can write over it), not the system's.
    pub user: bool,
}

/// The preset files under `folder`, sorted by group then name; none when the
/// folder is not there. Stops at [`MOST_FILES`].
pub fn list_folder(folder: &Path, user: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    walk(folder, folder, user, 0, &mut out);
    out.sort_by(|a, b| (&a.group, a.name.to_lowercase()).cmp(&(&b.group, b.name.to_lowercase())));
    out
}

fn walk(root: &Path, dir: &Path, user: bool, depth: usize, out: &mut Vec<Entry>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = read.filter_map(Result::ok).collect();
    items.sort_by_key(std::fs::DirEntry::file_name);
    for item in items {
        if out.len() >= MOST_FILES {
            return;
        }
        let path = item.path();
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth + 1 < DEPTH {
                walk(root, &path, user, depth + 1, out);
            }
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION))
            && let Some(stem) = path.file_stem()
        {
            let group = path
                .parent()
                .filter(|parent| *parent != root)
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(|rel| rel.to_string_lossy().replace('\\', "/"));
            out.push(Entry {
                name: stem.to_string_lossy().into_owned(),
                group,
                path,
                user,
            });
        }
    }
}

/// A path in `folder` for a preset called `name` that no file has yet:
/// "Name.vstpreset", then "Name 2.vstpreset" and on. Saving as never writes
/// over a file.
pub fn unused_path(folder: &Path, name: &str) -> PathBuf {
    let base = safe_name(name, "Preset");
    let first = folder.join(format!("{base}.{EXTENSION}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| folder.join(format!("{base} {n}.{EXTENSION}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// Work for the thread.
enum Job {
    List {
        device: Id,
        folders: Vec<(PathBuf, bool)>,
    },
    Read {
        device: Id,
        path: PathBuf,
    },
    Write {
        device: Id,
        folder: PathBuf,
        /// `Some` writes over this file (Save); `None` finds an unused name
        /// for `name` (Save as).
        over: Option<PathBuf>,
        name: String,
        bytes: Vec<u8>,
    },
}

/// What the thread found.
#[derive(Debug)]
pub enum Done {
    Listed {
        device: Id,
        entries: Vec<Entry>,
    },
    /// A file read: the preset, or why it cannot be used.
    Read {
        device: Id,
        name: String,
        result: Result<Preset, String>,
    },
    /// A file written: where, or why not.
    Wrote {
        device: Id,
        name: String,
        result: Result<PathBuf, String>,
    },
}

/// The thread that lists, reads and writes preset files; made on first use.
#[derive(Default)]
pub struct Files {
    worker: Option<(Sender<Job>, Receiver<Done>)>,
}

impl Files {
    fn send(&mut self, ctx: &egui::Context, job: Job) {
        let (tx, _) = self.worker.get_or_insert_with(|| {
            let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
            let (done_tx, done_rx) = mpsc::channel::<Done>();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name("lumen-preset-files".into())
                .spawn(move || {
                    for job in jobs_rx {
                        if done_tx.send(run(job)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                })
                .expect("spawn preset file thread");
            (jobs_tx, done_rx)
        });
        let _ = tx.send(job);
    }

    /// Looks for `device`'s presets in `folders`.
    pub fn list(&mut self, ctx: &egui::Context, device: Id, folders: Vec<(PathBuf, bool)>) {
        self.send(ctx, Job::List { device, folders });
    }

    /// Reads the preset file at `path` for `device`.
    pub fn read(&mut self, ctx: &egui::Context, device: Id, path: PathBuf) {
        self.send(ctx, Job::Read { device, path });
    }

    /// Writes a preset file for `device` into `folder`: over `over` when it
    /// is given, otherwise under an unused name made from `name`.
    pub fn write(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        folder: PathBuf,
        over: Option<PathBuf>,
        name: String,
        bytes: Vec<u8>,
    ) {
        self.send(
            ctx,
            Job::Write {
                device,
                folder,
                over,
                name,
                bytes,
            },
        );
    }

    /// What the thread has finished since the last call, in order.
    pub fn poll(&mut self) -> Vec<Done> {
        self.worker
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default()
    }
}

fn run(job: Job) -> Done {
    match job {
        Job::List { device, folders } => {
            let mut entries = Vec::new();
            for (folder, user) in folders {
                entries.extend(list_folder(&folder, user));
            }
            Done::Listed { device, entries }
        }
        Job::Read { device, path } => {
            let name = path
                .file_stem()
                .map_or_else(|| "Preset".into(), |s| s.to_string_lossy().into_owned());
            Done::Read {
                device,
                name,
                result: read_file(&path),
            }
        }
        Job::Write {
            device,
            folder,
            over,
            name,
            bytes,
        } => {
            let result = (|| {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let path = over.unwrap_or_else(|| unused_path(&folder, &name));
                lumen_project::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
                Ok(path)
            })();
            Done::Wrote {
                device,
                name,
                result,
            }
        }
    }
}

/// Reads and checks one preset file. A file too large is refused before it
/// is read.
fn read_file(path: &Path) -> Result<Preset, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE as u64 {
        return Err("the file is too large to be a preset".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Preset::parse(&bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    #[test]
    fn names_are_made_safe_for_a_file_system() {
        assert_eq!(safe_name("Warm / Pad: 2", "x"), "Warm Pad 2");
        assert_eq!(safe_name("  ..  ", "Preset"), "Preset");
        assert_eq!(safe_name("a\u{0}b", "x"), "a b");
        assert_eq!(safe_name("trailing. ", "x"), "trailing");
        assert_eq!(
            safe_name(&"w".repeat(200), "x").chars().count(),
            LONGEST_NAME
        );
    }

    #[test]
    fn a_plugins_folder_is_vendor_then_plugin() {
        let places = Places::fixed(PathBuf::from("/u"), vec![PathBuf::from("/s")]);
        assert_eq!(
            places.user_folder("Vendor A", "Synth One"),
            Some(PathBuf::from("/u/Vendor A/Synth One"))
        );
        let all = places.folders("Vendor A", "Synth One");
        assert_eq!(all.len(), 2);
        assert!(all[0].1 && !all[1].1);
    }

    #[test]
    fn files_are_found_in_the_folder_and_below_and_sorted() {
        let dir = TempDir::new("preset-list");
        let root = dir.path();
        std::fs::create_dir_all(root.join("Bass")).unwrap();
        std::fs::write(root.join("Zed.vstpreset"), b"x").unwrap();
        std::fs::write(root.join("alpha.VSTPRESET"), b"x").unwrap();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join("Bass").join("Warm.vstpreset"), b"x").unwrap();
        let found = list_folder(root, true);
        let names: Vec<(Option<&str>, &str)> = found
            .iter()
            .map(|e| (e.group.as_deref(), e.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [(None, "alpha"), (None, "Zed"), (Some("Bass"), "Warm")]
        );
        assert!(found.iter().all(|e| e.user));
        assert!(list_folder(&root.join("missing"), false).is_empty());
    }

    #[test]
    fn save_as_never_writes_over_a_file() {
        let dir = TempDir::new("preset-unused");
        let root = dir.path();
        assert_eq!(unused_path(root, "Lead"), root.join("Lead.vstpreset"));
        std::fs::write(root.join("Lead.vstpreset"), b"x").unwrap();
        assert_eq!(unused_path(root, "Lead"), root.join("Lead 2.vstpreset"));
        std::fs::write(root.join("Lead 2.vstpreset"), b"x").unwrap();
        assert_eq!(unused_path(root, "Lead"), root.join("Lead 3.vstpreset"));
    }

    #[test]
    fn a_file_over_the_limit_is_refused_before_it_is_read() {
        let dir = TempDir::new("preset-big");
        let path = dir.path().join("big.vstpreset");
        std::fs::write(&path, vec![0_u8; MAX_FILE + 1]).unwrap();
        assert!(read_file(&path).unwrap_err().contains("too large"));
        std::fs::write(&path, b"nope").unwrap();
        assert!(read_file(&path).is_err());
    }
}
