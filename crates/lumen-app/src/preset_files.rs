use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use lumen_hosting::{MAX_FILE, Preset};
use lumen_project::model::Id;

pub const EXTENSION: &str = "vstpreset";
pub const BUILTIN_EXTENSION: &str = "lumenpreset";
pub const BUILTIN_VERSION: u64 = 1;

fn builtin_root() -> String {
    format!("{} Presets", lumen_core::product::PRODUCT_NAME)
}
const DEPTH: usize = 4;
const MOST_FILES: usize = 2000;
pub const NAME_LIMIT: usize = 30;
const LONGEST_NAME: usize = 80;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    pub user: Option<PathBuf>,
    pub system: Vec<PathBuf>,
    pub builtin: Option<PathBuf>,
    pub fixed: bool,
}

impl Places {
    pub fn standard() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if cfg!(windows) {
            let documents =
                documents_folder().or_else(|| var("USERPROFILE").map(|p| p.join("Documents")));
            let profile = documents.as_ref().map(|p| p.join("VST3 Presets"));
            let user = documents
                .as_ref()
                .map(|d| d.join("VST3 Presets"))
                .or(profile.clone());
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

    pub fn fixed(user: PathBuf, system: Vec<PathBuf>) -> Self {
        Self {
            builtin: Some(user.join(builtin_root())),
            user: Some(user),
            system,
            fixed: true,
        }
    }

    pub fn user_folder(&self, vendor: &str, plugin: &str) -> Option<PathBuf> {
        self.user
            .as_ref()
            .map(|root| plugin_folder(root, vendor, plugin))
    }

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

    pub fn builtin_folder(&self, device: &str) -> Option<PathBuf> {
        self.builtin
            .as_ref()
            .map(|root| root.join(safe_name(device, "Device")))
    }

    pub fn builtin_folders(&self, device: &str) -> Vec<(PathBuf, bool)> {
        self.builtin_folder(device)
            .map(|folder| vec![(folder, true)])
            .unwrap_or_default()
    }
}

#[cfg(windows)]
fn documents_folder() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    let mut wide: windows_sys::core::PWSTR = std::ptr::null_mut();
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Documents,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut wide,
        )
    };
    let path = (result >= 0 && !wide.is_null()).then(|| {
        let len = (0..).take_while(|&i| unsafe { *wide.add(i) } != 0).count();
        let chars = unsafe { std::slice::from_raw_parts(wide, len) };
        PathBuf::from(std::ffi::OsString::from_wide(chars))
    });
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub group: Option<String>,
    pub path: PathBuf,
    pub user: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sound {
    Vst3(Preset),
    Builtin {
        device: String,
        values: BTreeMap<u32, f64>,
    },
}

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

pub fn parse_builtin(bytes: &[u8]) -> Result<(String, BTreeMap<u32, f64>), String> {
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("the file is not readable ({e})"))?;
    if json["format"] != "lumen_builtin" {
        return Err("the file is not a built-in device's preset".to_owned());
    }
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

pub struct Written {
    pub folder: PathBuf,
    pub over: Option<PathBuf>,
    pub name: String,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

enum Job {
    List {
        device: Id,
        folders: Vec<(PathBuf, bool)>,
    },
    Read {
        device: Id,
        path: PathBuf,
    },
    Folder {
        folder: PathBuf,
    },
    Write {
        device: Id,
        folder: PathBuf,
        over: Option<PathBuf>,
        name: String,
        bytes: Vec<u8>,
        extension: &'static str,
    },
}

#[derive(Debug)]
pub enum Done {
    Listed {
        device: Id,
        entries: Vec<Entry>,
    },
    Read {
        device: Id,
        name: String,
        result: Result<Sound, String>,
    },
    Folder {
        folder: PathBuf,
        exists: bool,
    },
    Wrote {
        device: Id,
        name: String,
        result: Result<PathBuf, String>,
    },
}

#[derive(Default)]
pub struct Files {
    worker: Option<(Sender<Job>, Receiver<Done>)>,
    failure: Option<String>,
}

impl Files {
    fn send(&mut self, ctx: &egui::Context, job: Job) {
        if self.worker.is_none() {
            match Self::start(ctx) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
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

    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    pub fn list(&mut self, ctx: &egui::Context, device: Id, folders: Vec<(PathBuf, bool)>) {
        self.send(ctx, Job::List { device, folders });
    }

    pub fn folder(&mut self, ctx: &egui::Context, folder: PathBuf) {
        self.send(ctx, Job::Folder { folder });
    }

    pub fn read(&mut self, ctx: &egui::Context, device: Id, path: PathBuf) {
        self.send(ctx, Job::Read { device, path });
    }

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
        assert!(parse_builtin(b"nope").is_err());
        assert!(parse_builtin(br#"{"format":"other"}"#).is_err());
        let bad = br#"{"format":"lumen_builtin","device":"utility","params":{"x":0.5}}"#;
        assert!(parse_builtin(bad).is_err());
        let wild = br#"{"format":"lumen_builtin","device":"utility","params":{"1":7}}"#;
        assert_eq!(parse_builtin(wild).unwrap().1[&1], 1.0, "brought in");
    }

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
        assert!(parse_builtin(&builtin_bytes("utility", &values)).is_ok());
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
