//! Preset files on disk (M6 slice 5f; built-ins M8 slice B0): where a plugin's
//! `.vstpreset` files are kept, finding them, reading one and writing one, all
//! off the UI thread. The bytes are turned into a sound by
//! `lumen_hosting::Preset`; a built-in's preset is Lumen's own JSON file
//! (`.lumenpreset`) holding the device's name and its parameter values. This
//! module only moves them to and from the disk and back to the frame loop,
//! which polls each frame.
//!
//! The standard says where a plugin's presets live: a folder per vendor and
//! plugin in the user's presets folder and in the system's. Saving writes to
//! the user's. The app's own devices keep theirs in a folder of the documents
//! folder named after the product (`product_name!`), one folder per device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use lumen_hosting::{MAX_FILE, Preset};
use lumen_project::model::Id;

/// The extension of a plugin's preset file.
pub const EXTENSION: &str = "vstpreset";
/// The extension of a built-in device's preset file.
pub const BUILTIN_EXTENSION: &str = "lumenpreset";
/// The only `version` a built-in device's preset file may say (M8 slice B0,
/// review L4): a file a newer build wrote is refused with what it says, never
/// half-read.
pub const BUILTIN_VERSION: u64 = 1;

/// The folder of the documents folder where the app's own devices keep their
/// presets: its name is the product's, from `lumen_core`.
fn builtin_root() -> String {
    format!("{} Presets", lumen_core::product::PRODUCT_NAME)
}
/// The deepest a plugin's folder is searched, and the most files listed.
const DEPTH: usize = 4;
const MOST_FILES: usize = 2000;
/// The longest name Lumen gives a preset it saves, in characters: a name that
/// fits the card's field and a file list.
pub const NAME_LIMIT: usize = 30;
/// The longest a file or folder name is made (a plugin's own name may be long).
const LONGEST_NAME: usize = 80;

/// Where preset files are kept: the user's folder (saving writes here) and
/// the system's, which are only read, and Lumen's own devices' root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    pub user: Option<PathBuf>,
    pub system: Vec<PathBuf>,
    /// Where the built-in devices' presets are kept (M8 slice B0).
    pub builtin: Option<PathBuf>,
    /// Chosen by the caller (tests): no window is opened on these.
    pub fixed: bool,
}

impl Places {
    /// This machine's standard folders.
    pub fn standard() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if cfg!(windows) {
            // The Documents folder where Windows keeps it, which a person may
            // have moved (KNOWN_ISSUES row 175); the profile's own Documents
            // when it cannot say.
            let documents =
                documents_folder().or_else(|| var("USERPROFILE").map(|p| p.join("Documents")));
            let profile = documents.as_ref().map(|p| p.join("VST3 Presets"));
            let user = documents
                .as_ref()
                .map(|d| d.join("VST3 Presets"))
                .or(profile.clone());
            // Presets an earlier Lumen saved under the profile's Documents,
            // when that is not the user's folder, are still listed.
            let earlier = profile.filter(|p| Some(p) != user.as_ref() && p.is_dir());
            Self {
                user,
                system: var("PROGRAMDATA")
                    .map(|p| p.join("VST3 Presets"))
                    .into_iter()
                    .chain(earlier)
                    .collect(),
                builtin: documents.map(|d| d.join(builtin_root())),
                fixed: false,
            }
        } else if cfg!(target_os = "macos") {
            Self {
                user: var("HOME").map(|p| p.join("Library/Audio/Presets")),
                system: vec![PathBuf::from("/Library/Audio/Presets")],
                builtin: var("HOME").map(|p| p.join("Documents").join(builtin_root())),
                fixed: false,
            }
        } else {
            Self {
                user: var("HOME").map(|p| p.join(".vst3/presets")),
                system: vec![
                    PathBuf::from("/usr/share/vst3/presets"),
                    PathBuf::from("/usr/local/share/vst3/presets"),
                ],
                builtin: var("HOME").map(|p| p.join("Documents").join(builtin_root())),
                fixed: false,
            }
        }
    }

    /// Folders chosen by the caller (tests).
    pub fn fixed(user: PathBuf, system: Vec<PathBuf>) -> Self {
        Self {
            builtin: Some(user.join(builtin_root())),
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

    /// Where a built-in device called `device` keeps its presets: Lumen's own
    /// folder, one folder per device.
    pub fn builtin_folder(&self, device: &str) -> Option<PathBuf> {
        self.builtin
            .as_ref()
            .map(|root| root.join(safe_name(device, "Device")))
    }

    /// As [`Places::folders`], for a built-in device.
    pub fn builtin_folders(&self, device: &str) -> Vec<(PathBuf, bool)> {
        self.builtin_folder(device)
            .map(|folder| vec![(folder, true)])
            .unwrap_or_default()
    }
}

/// The user's Documents folder as Windows knows it (`FOLDERID_Documents`),
/// which follows a folder the person moved or had redirected.
#[cfg(windows)]
fn documents_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    let mut wide: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: the GUID is a static, the token is null (the current user),
    // and `wide` receives a string the shell allocates, freed below on
    // every path.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Documents,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut wide,
        )
    };
    let path = (result >= 0 && !wide.is_null()).then(|| {
        // SAFETY: on success the shell returns a NUL-terminated string.
        let len = (0..).take_while(|&i| unsafe { *wide.add(i) } != 0).count();
        // SAFETY: `len` characters were just read from it.
        let chars = unsafe { std::slice::from_raw_parts(wide, len) };
        PathBuf::from(std::ffi::OsString::from_wide(chars))
    });
    // SAFETY: the string (or null) came from SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(wide as *const core::ffi::c_void) };
    path.filter(|p| p.is_absolute())
}

#[cfg(not(windows))]
fn documents_folder() -> Option<PathBuf> {
    None
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

/// A sound read from a file: a plugin's state, or a built-in's values.
#[derive(Debug, Clone, PartialEq)]
pub enum Sound {
    Vst3(Preset),
    Builtin {
        /// The device it was saved for, by its stable name.
        device: String,
        values: BTreeMap<u32, f64>,
    },
}

/// A built-in device's preset file: its name and what its parameters hold,
/// as JSON, so a person can read and edit one.
pub fn builtin_bytes(device: &str, values: &BTreeMap<u32, f64>) -> Vec<u8> {
    let values: BTreeMap<String, f64> = values
        .iter()
        .map(|(id, value)| (id.to_string(), *value))
        .collect();
    let json = serde_json::json!({
        "format": "lumen_builtin",
        "version": BUILTIN_VERSION,
        "device": device,
        "params": values,
    });
    let mut bytes = serde_json::to_vec_pretty(&json).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

/// [`builtin_bytes`] read back, or why it cannot be.
pub fn parse_builtin(bytes: &[u8]) -> Result<(String, BTreeMap<u32, f64>), String> {
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("the file is not readable ({e})"))?;
    if json["format"] != "lumen_builtin" {
        return Err("the file is not a built-in device's preset".to_owned());
    }
    // A file a newer build wrote may mean something else by its values: it is
    // refused, in full, rather than half-read (review L4). A file with no
    // version is one a person wrote by hand, and is read as this version.
    let version = json["version"].as_u64().unwrap_or(BUILTIN_VERSION);
    if version != BUILTIN_VERSION {
        return Err(format!(
            "This preset was saved by a newer version of {} (version {version}; this version \
             reads version {BUILTIN_VERSION}).",
            lumen_core::product::PRODUCT_NAME
        ));
    }
    let device = json["device"]
        .as_str()
        .ok_or("the file does not say which device it is for")?
        .to_owned();
    let mut values = BTreeMap::new();
    let params = json["params"]
        .as_object()
        .ok_or("the file holds no values")?;
    for (id, value) in params {
        let id: u32 = id
            .parse()
            .map_err(|_| format!("\u{201c}{id}\u{201d} is not a parameter"))?;
        let value = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("parameter {id} has no value"))?;
        values.insert(id, value.clamp(0.0, 1.0));
    }
    Ok((device, values))
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
            && path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                x.eq_ignore_ascii_case(EXTENSION) || x.eq_ignore_ascii_case(BUILTIN_EXTENSION)
            })
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
/// "Name.vstpreset", then "Name 2.vstpreset" and on. `extension` picks the
/// kind (a built-in's is `.lumenpreset`). Saving as never writes over a file.
pub fn unused_path(folder: &Path, name: &str, extension: &str) -> PathBuf {
    let base = safe_name(name, "Preset");
    let first = folder.join(format!("{base}.{extension}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| folder.join(format!("{base} {n}.{extension}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// A preset file to write (M8 slice B0: a plugin's `.vstpreset` or a
/// built-in's `.lumenpreset`).
pub struct Written {
    pub folder: PathBuf,
    /// `Some` writes over this file (Save); `None` finds an unused name.
    pub over: Option<PathBuf>,
    pub name: String,
    pub bytes: Vec<u8>,
    /// The extension a new file gets.
    pub extension: &'static str,
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
    /// Whether a folder is there.
    Folder {
        folder: PathBuf,
    },
    Write {
        device: Id,
        folder: PathBuf,
        /// `Some` writes over this file (Save); `None` finds an unused name
        /// for `name` (Save as).
        over: Option<PathBuf>,
        name: String,
        bytes: Vec<u8>,
        /// The extension a new file gets: a plugin's or a built-in's.
        extension: &'static str,
    },
}

/// What the thread found.
#[derive(Debug)]
pub enum Done {
    Listed {
        device: Id,
        entries: Vec<Entry>,
    },
    /// A file read: the sound, or why it cannot be used.
    Read {
        device: Id,
        name: String,
        result: Result<Sound, String>,
    },
    /// Whether the folder asked about exists.
    Folder {
        folder: PathBuf,
        exists: bool,
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
    /// Why the thread could not start; said once by the interface.
    failure: Option<String>,
}

impl Files {
    fn send(&mut self, ctx: &egui::Context, job: Job) {
        if self.worker.is_none() {
            match Self::start(ctx) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    // The save or load the person asked for did not start:
                    // say why rather than wait for an answer that is never
                    // coming.
                    let why = crate::spawn::start_failed("the preset files", &error);
                    crate::trace::log(|| format!("presets: {why}"));
                    self.failure = Some(why);
                    return;
                }
            }
        }
        if let Some((tx, _)) = &self.worker {
            let _ = tx.send(job);
        }
    }

    fn start(ctx: &egui::Context) -> std::io::Result<(Sender<Job>, Receiver<Done>)> {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (done_tx, done_rx) = mpsc::channel::<Done>();
        let ctx = ctx.clone();
        crate::spawn::spawn_named("lumen-preset-files", move || {
            for job in jobs_rx {
                if done_tx.send(run(job)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        })?;
        Ok((jobs_tx, done_rx))
    }

    /// Why a preset action could not start, once.
    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    /// Looks for `device`'s presets in `folders`.
    pub fn list(&mut self, ctx: &egui::Context, device: Id, folders: Vec<(PathBuf, bool)>) {
        self.send(ctx, Job::List { device, folders });
    }

    /// Asks whether `folder` exists (a stat can be slow on a network drive).
    pub fn folder(&mut self, ctx: &egui::Context, folder: PathBuf) {
        self.send(ctx, Job::Folder { folder });
    }

    /// Reads the preset file at `path` for `device`.
    pub fn read(&mut self, ctx: &egui::Context, device: Id, path: PathBuf) {
        self.send(ctx, Job::Read { device, path });
    }

    /// Writes a preset file for `device` into `folder`: over `over` when it
    /// is given, otherwise under an unused name made from `name`, with
    /// `extension` for a new file (a plugin's or a built-in's).
    pub fn write(&mut self, ctx: &egui::Context, device: Id, job: Written) {
        self.send(
            ctx,
            Job::Write {
                device,
                folder: job.folder,
                over: job.over,
                name: job.name,
                bytes: job.bytes,
                extension: job.extension,
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
        Job::Folder { folder } => {
            let exists = folder.exists();
            Done::Folder { folder, exists }
        }
        Job::Write {
            device,
            folder,
            over,
            name,
            bytes,
            extension,
        } => {
            let result = (|| {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let path = over.unwrap_or_else(|| unused_path(&folder, &name, extension));
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

/// Reads and checks one preset file: a plugin's state, or a built-in's
/// values, by the file's extension. A file too large is refused before it is
/// read.
fn read_file(path: &Path) -> Result<Sound, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE as u64 {
        return Err("the file is too large to be a preset".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let builtin = path
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case(BUILTIN_EXTENSION));
    if builtin {
        let (device, values) = parse_builtin(&bytes)?;
        return Ok(Sound::Builtin { device, values });
    }
    Preset::parse(&bytes)
        .map(Sound::Vst3)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    #[cfg(windows)]
    #[test]
    fn the_documents_folder_is_the_one_windows_knows() {
        let documents = documents_folder().expect("Windows names a Documents folder");
        assert!(documents.is_absolute(), "{}", documents.display());
        let places = Places::standard();
        assert_eq!(places.user, Some(documents.join("VST3 Presets")));
    }

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
        assert_eq!(
            unused_path(root, "Lead", EXTENSION),
            root.join("Lead.vstpreset")
        );
        std::fs::write(root.join("Lead.vstpreset"), b"x").unwrap();
        assert_eq!(
            unused_path(root, "Lead", EXTENSION),
            root.join("Lead 2.vstpreset")
        );
        std::fs::write(root.join("Lead 2.vstpreset"), b"x").unwrap();
        assert_eq!(
            unused_path(root, "Lead", EXTENSION),
            root.join("Lead 3.vstpreset")
        );
        // A built-in's own file never takes a plugin's name either.
        assert_eq!(
            unused_path(root, "Lead", BUILTIN_EXTENSION),
            root.join("Lead.lumenpreset")
        );
    }

    #[test]
    fn a_built_in_s_preset_is_json_a_person_can_read_and_comes_back_the_same() {
        let values: BTreeMap<u32, f64> = [(1, 0.5), (7, 1.0)].into_iter().collect();
        let bytes = builtin_bytes("utility", &values);
        let text = String::from_utf8(bytes.clone()).expect("UTF-8");
        assert!(text.contains("\"format\": \"lumen_builtin\""), "{text}");
        assert!(text.contains("\"device\": \"utility\""));
        let (device, back) = parse_builtin(&bytes).expect("reads back");
        assert_eq!(device, "utility");
        assert_eq!(back, values);
        // What is not one is refused, and so is a value out of range.
        assert!(parse_builtin(b"nope").is_err());
        assert!(parse_builtin(br#"{"format":"other"}"#).is_err());
        let bad = br#"{"format":"lumen_builtin","device":"utility","params":{"x":0.5}}"#;
        assert!(parse_builtin(bad).is_err());
        let wild = br#"{"format":"lumen_builtin","device":"utility","params":{"1":7}}"#;
        assert_eq!(parse_builtin(wild).unwrap().1[&1], 1.0, "brought in");
    }

    /// L4: a file a newer build wrote is refused with the whole reason, never
    /// half-read; the version this build writes is the one it reads.
    #[test]
    fn a_preset_a_newer_build_wrote_is_refused_in_full() {
        let values: BTreeMap<u32, f64> = [(1, 0.5)].into_iter().collect();
        let newer = br#"{"format":"lumen_builtin","version":2,"device":"utility",
            "params":{"1":0.5}}"#;
        let why = parse_builtin(newer).unwrap_err();
        assert!(why.contains("newer version"), "{why}");
        assert!(
            why.contains("version 2") && why.contains("version 1"),
            "{why}"
        );
        assert!(why.ends_with('.'), "a full sentence: {why}");
        // What this build writes reads back.
        assert!(parse_builtin(&builtin_bytes("utility", &values)).is_ok());
        // A hand-written file with no version is this version.
        let bare = br#"{"format":"lumen_builtin","device":"utility","params":{"1":0.5}}"#;
        assert_eq!(parse_builtin(bare).unwrap().1, values);
    }

    #[test]
    fn a_built_in_s_preset_is_read_by_its_extension() {
        let dir = TempDir::new("preset-builtin");
        let path = dir.path().join("Bright.lumenpreset");
        let values: BTreeMap<u32, f64> = [(1, 0.5)].into_iter().collect();
        std::fs::write(&path, builtin_bytes("utility", &values)).unwrap();
        assert_eq!(
            read_file(&path),
            Ok(Sound::Builtin {
                device: "utility".to_owned(),
                values
            })
        );
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
