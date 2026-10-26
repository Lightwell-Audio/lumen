//! Reading and writing `.lumen` files.
//!
//! Saving is atomic: the document is written to a temporary file in the same
//! folder, flushed to disk, and only then renamed over the old file. A crash
//! at any moment leaves either the old or the new document, never a mix.
//! Lumen also never writes a document it could not read back: saving
//! validates first.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::product::{PRODUCT_NAME, display_name};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::migrate::{CURRENT_VERSION, FIRST_VERSION, MIGRATIONS, upgrade};
use crate::model::{Media, Project};
use crate::{json, validate::validate};

/// Value of the `format` field that identifies a project file.
pub const FORMAT_ID: &str = "lumen-project";

#[derive(Serialize)]
struct EnvelopeOut<'a> {
    format: &'static str,
    version: u64,
    generator: String,
    project: &'a Project,
}

#[derive(Deserialize)]
struct EnvelopeIn {
    project: Project,
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    /// Not valid JSON: truncated, damaged, or not text.
    Syntax {
        line: usize,
        column: usize,
        message: String,
    },
    NotAProject,
    TooNew {
        version: u64,
        supported: u64,
    },
    Migration {
        from: u64,
        message: String,
    },
    /// Valid JSON that does not describe a consistent project.
    Invalid(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "The project could not be read: {e}"),
            LoadError::Syntax {
                line,
                column,
                message,
            } => write!(
                f,
                "The project file is damaged (line {line}, column {column}): {message}"
            ),
            LoadError::NotAProject => write!(f, "This is not a {PRODUCT_NAME} project file."),
            LoadError::TooNew { version, supported } => write!(
                f,
                "This project was saved by a newer version of {PRODUCT_NAME} \
                 (format {version}; this version reads up to format {supported})."
            ),
            LoadError::Migration { from, message } => {
                write!(
                    f,
                    "The project could not be upgraded from format {from}: {message}"
                )
            }
            LoadError::Invalid(message) => write!(f, "The project file is inconsistent: {message}"),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Debug)]
pub enum SaveError {
    /// The in-memory project is inconsistent; nothing was written.
    Invalid(String),
    Io(io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Invalid(message) => {
                write!(f, "Not saved, the project is inconsistent: {message}")
            }
            SaveError::Io(e) => write!(f, "The project could not be saved: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Serializes `project` as the current format version.
pub fn to_bytes(project: &Project) -> Result<Vec<u8>, SaveError> {
    validate(project).map_err(SaveError::Invalid)?;
    let envelope = EnvelopeOut {
        format: FORMAT_ID,
        version: CURRENT_VERSION,
        generator: display_name(),
        project,
    };
    let pretty =
        serde_json::to_string_pretty(&envelope).map_err(|e| SaveError::Invalid(e.to_string()))?;
    let mut text = json::compact_scalar_arrays(&pretty);
    text.push('\n');
    Ok(text.into_bytes())
}

/// Parses, migrates and validates a document of any supported version.
pub fn from_bytes(bytes: &[u8]) -> Result<Project, LoadError> {
    let document: Value = serde_json::from_slice(bytes).map_err(|e| LoadError::Syntax {
        line: e.line(),
        column: e.column(),
        message: e.to_string(),
    })?;
    if document.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(LoadError::NotAProject);
    }
    let version = document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(LoadError::NotAProject)?;
    if version > CURRENT_VERSION {
        return Err(LoadError::TooNew {
            version,
            supported: CURRENT_VERSION,
        });
    }
    let document = upgrade(document, version, FIRST_VERSION, MIGRATIONS)
        .map_err(|(from, message)| LoadError::Migration { from, message })?;
    let envelope: EnvelopeIn =
        serde_json::from_value(document).map_err(|e| LoadError::Invalid(e.to_string()))?;
    validate(&envelope.project).map_err(LoadError::Invalid)?;
    Ok(envelope.project)
}

pub fn load(path: &Path) -> Result<Project, LoadError> {
    from_bytes(&fs::read(path).map_err(LoadError::Io)?)
}

#[derive(Debug, Clone)]
pub struct SaveOptions {
    /// Previous versions kept in the `Backups` folder next to the project.
    pub keep_backups: usize,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self { keep_backups: 10 }
    }
}

/// Saves atomically, keeping rolling backups of the previous file.
pub fn save(project: &Project, path: &Path, options: &SaveOptions) -> Result<(), SaveError> {
    let bytes = to_bytes(project)?;
    write_atomically(path, &bytes, options, &mut |_| Ok(())).map_err(SaveError::Io)
}

/// Writes any file (an exported MIDI file, M5 slice 7) by the project's
/// commit protocol: a temporary file beside it, flushed, then renamed over
/// the old one, so a crash leaves the old file or the new, never half.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let options = SaveOptions { keep_backups: 0 };
    write_atomically(path, bytes, &options, &mut |_| Ok(()))
}

/// The commit protocol. `before_commit` runs after the new content is safely
/// on disk and before it replaces the old file (a test seam for failures).
fn write_atomically(
    path: &Path,
    bytes: &[u8],
    options: &SaveOptions,
    before_commit: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let temp = dir.join(format!(".{name}.{}.saving", std::process::id()));
    let result = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        before_commit(&temp)?;
        if options.keep_backups > 0 && path.exists() {
            back_up(path, dir, &name, options.keep_backups)?;
        }
        fs::rename(&temp, path)?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    } else {
        remove_stale_temp_files(dir, &name);
    }
    result
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    // NTFS journals the rename; directory handles cannot be flushed this way.
    Ok(())
}

/// Leftovers of saves that crashed before committing.
fn remove_stale_temp_files(dir: &Path, name: &str) {
    let prefix = format!(".{name}.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.starts_with(&prefix) && file.ends_with(".saving") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Copies the current file to `Backups/<stem> <UTC timestamp>.lumen` and
/// prunes the oldest beyond `keep`.
fn back_up(path: &Path, dir: &Path, name: &str, keep: usize) -> io::Result<()> {
    back_up_at(path, dir, name, keep, &utc_timestamp())
}

/// `back_up` with a given timestamp. Saves within the same millisecond get
/// `_001`, `_002`… after it: `_` sorts after `.`, and the padding keeps the
/// numbers in order, so the newest backup always sorts last.
fn back_up_at(path: &Path, dir: &Path, name: &str, keep: usize, stamp: &str) -> io::Result<()> {
    let backups = dir.join("Backups");
    fs::create_dir_all(&backups)?;
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Continue after the highest suffix already used for this stamp, so a
    // slot freed by pruning is never reused for a newer backup.
    let taken = format!("{stem} {stamp}");
    let mut newest: Option<u32> = None;
    for entry in fs::read_dir(&backups)?.flatten() {
        let file = entry.path();
        let Some(rest) = file
            .file_stem()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(&taken))
        else {
            continue;
        };
        let counter = match rest {
            "" => Some(0),
            _ => rest.strip_prefix('_').and_then(|n| n.parse().ok()),
        };
        newest = newest.max(counter);
    }
    let target = match newest {
        None => backups.join(format!("{taken}.{extension}")),
        Some(n) => backups.join(format!("{taken}_{:03}.{extension}", n + 1)),
    };
    fs::copy(path, &target)?;

    let prefix = format!("{stem} ");
    let mut existing: Vec<PathBuf> = fs::read_dir(&backups)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
        })
        .collect();
    existing.sort(); // timestamps sort lexicographically
    let excess = existing.len().saturating_sub(keep);
    for old in &existing[..excess] {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// `2026-10-05T183012.345Z`: sortable and valid in Windows file names.
pub fn utc_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant), proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}{:02}{:02}.{:03}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60,
        now.subsec_millis()
    )
}

/// The path to store for `file` in a project kept in `project_dir`:
/// relative with `/` separators when inside the folder, absolute otherwise.
pub fn media_path_for(project_dir: &Path, file: &Path) -> String {
    match file.strip_prefix(project_dir) {
        Ok(relative) => relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => file.to_string_lossy().into_owned(),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MediaLocation {
    Found(PathBuf),
    /// Offer relinking; these are the places that were checked.
    Missing {
        tried: Vec<PathBuf>,
    },
}

/// Finds a media file: the stored path (relative to the project folder),
/// then the last known absolute location.
pub fn locate_media(project_dir: &Path, media: &Media) -> MediaLocation {
    let stored = Path::new(&media.path);
    let mut tried = vec![if stored.is_absolute() {
        stored.to_path_buf()
    } else {
        media
            .path
            .split('/')
            .fold(project_dir.to_path_buf(), |p, part| p.join(part))
    }];
    if let Some(original) = &media.original_path {
        tried.push(PathBuf::from(original));
    }
    match tried.iter().find(|p| p.is_file()) {
        Some(found) => MediaLocation::Found(found.clone()),
        None => MediaLocation::Missing { tried },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TempDir, small_project};

    #[test]
    fn a_failed_commit_leaves_the_old_file_and_no_temp_file() {
        let dir = TempDir::new("failed-commit");
        let path = dir.path().join("Song.lumen");
        save(&small_project(1), &path, &SaveOptions::default()).unwrap();
        let before = fs::read(&path).unwrap();

        let bytes = to_bytes(&small_project(2)).unwrap();
        let result = write_atomically(&path, &bytes, &SaveOptions::default(), &mut |_| {
            Err(io::Error::other("simulated power loss before commit"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before, "old file must be intact");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".saving"))
            .collect();
        assert!(leftovers.is_empty(), "temp file not cleaned up");
    }

    #[test]
    fn leftovers_of_a_crashed_save_are_harmless_and_cleaned() {
        let dir = TempDir::new("stale-temp");
        let path = dir.path().join("Song.lumen");
        save(&small_project(3), &path, &SaveOptions::default()).unwrap();
        let stale = dir.path().join(".Song.lumen.99999.saving");
        fs::write(&stale, b"{ half a docum").unwrap();
        assert_eq!(load(&path).unwrap(), small_project(3));
        save(&small_project(4), &path, &SaveOptions::default()).unwrap();
        assert!(
            !stale.exists(),
            "stale temp file should be removed after a good save"
        );
    }

    #[test]
    fn backups_rotate() {
        let dir = TempDir::new("backups");
        let path = dir.path().join("Song.lumen");
        let options = SaveOptions { keep_backups: 3 };
        for seed in 0..6 {
            save(&small_project(seed), &path, &options).unwrap();
        }
        let mut backups: Vec<PathBuf> = fs::read_dir(dir.path().join("Backups"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        backups.sort();
        assert_eq!(backups.len(), 3);
        // The newest backup is the version before the current one.
        assert_eq!(load(backups.last().unwrap()).unwrap(), small_project(4));
        assert_eq!(load(&path).unwrap(), small_project(5));
    }

    #[test]
    fn backups_in_the_same_millisecond_keep_their_order() {
        let dir = TempDir::new("backups-same-ms");
        let path = dir.path().join("Song.lumen");
        let stamp = "2026-10-06T120000.000Z";
        for seed in 0..12 {
            fs::write(&path, format!("version {seed}")).unwrap();
            back_up_at(&path, dir.path(), "Song.lumen", 3, stamp).unwrap();
        }
        let mut backups: Vec<PathBuf> = fs::read_dir(dir.path().join("Backups"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        backups.sort();
        let kept: Vec<String> = backups
            .iter()
            .map(|p| fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(kept, ["version 9", "version 10", "version 11"]);
    }

    #[test]
    fn timestamps_have_the_documented_shape() {
        let stamp = utc_timestamp();
        assert_eq!(stamp.len(), "2026-10-05T183012.345Z".len(), "{stamp}");
        assert!(stamp.ends_with('Z') && !stamp.contains(':'));
    }
}
