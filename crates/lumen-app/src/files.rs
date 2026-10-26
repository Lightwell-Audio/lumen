//! Saving and opening projects (M3 slice 3c).
//!
//! A project is a folder: `<Name>/<Name>.lumen`, with `Samples/Imported/`
//! for audio copied in and `Backups/` for earlier saves (ADR-0004). Saving
//! and opening run on background threads, so the interface and the audio
//! never wait for the disk; the result comes back on a channel.
//!
//! The project stores media paths relative to its folder when the file is
//! inside it. Where each file actually is on this machine is kept outside
//! the undo history (`Document::media_files`) and written into the paths at
//! save time.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};

use lumen_project::model::{Id, Project};
use lumen_project::{MediaLocation, SaveOptions, locate_media, media_path_for};

/// Where audio copied into a project goes, inside its folder.
pub const IMPORTED: &str = "Samples/Imported";
/// Where audio Lumen rendered itself (a consolidated clip) goes, inside the
/// project folder.
pub const RENDERED: &str = "Samples/Rendered";
/// Where audio recorded in Lumen goes, inside the project folder.
pub const RECORDED: &str = "Samples/Recorded";
/// The file extension of projects.
pub const EXTENSION: &str = "lumen";

/// Where a project chosen in Save As is written. A file picked inside an
/// existing project folder (one holding another project file or a `Samples`
/// folder) or a folder already named after the project is used as it is;
/// anywhere else, a folder named after the project is created for it.
pub fn project_file_for(chosen: &Path) -> PathBuf {
    let chosen = chosen.with_extension(EXTENSION);
    let (Some(dir), Some(stem)) = (chosen.parent(), chosen.file_stem()) else {
        return chosen;
    };
    let is_project_folder = dir.join("Samples").is_dir()
        || dir.file_name() == Some(stem)
        || std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == EXTENSION) && e.path() != chosen)
        });
    if is_project_folder {
        chosen
    } else {
        dir.join(stem).join(chosen.file_name().unwrap_or(stem))
    }
}

/// What a save needs, taken from the document when the save starts.
pub struct SaveRequest {
    /// The project with transport and view stamped in.
    pub project: Project,
    pub path: PathBuf,
    /// Resolve a user-selected Save As path on the worker, since project
    /// folder detection reads directories.
    pub normalize_project_path: bool,
    /// Where each media file is now.
    pub media_files: HashMap<Id, PathBuf>,
    /// Copy audio from outside the project folder into `Samples/Imported`.
    pub copy_imported: bool,
    /// The folder Lumen writes rendered audio to while a project is open.
    /// Files in it are Lumen's own work, which only the project holds a
    /// reference to: they are copied into the project whatever
    /// `copy_imported` says, and are not recorded as imported.
    pub renders: Option<PathBuf>,
    /// The folder takes are recorded into: handled like `renders`, and copied
    /// into `Samples/Recorded`.
    pub recordings: Option<PathBuf>,
    /// The folder templates are kept in. A project made from a template
    /// plays the template's audio from there: it is copied into the project
    /// whatever `copy_imported` says, into the same kind of folder it was in,
    /// and is not recorded as imported (M9 slice R4).
    pub templates: Option<PathBuf>,
    /// Earlier versions kept in `Backups` (none for autosaves).
    pub keep_backups: usize,
    /// The document state being saved (edits during the save stay unsaved).
    pub state: u64,
    /// A project file this save must not write: the open project's own, when
    /// saving a copy (the check reads the disk, so the worker makes it).
    pub not_over: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Saved {
    pub path: PathBuf,
    /// Media files that moved (copied into the project).
    pub moved: HashMap<Id, PathBuf>,
    pub state: u64,
    /// Files copied into the project, and their size together.
    pub copied: usize,
    pub bytes: u64,
    /// Media the save could not find on this machine: they are left as the
    /// project had them, still missing. Their file names.
    pub skipped: Vec<String>,
}

/// Stops a long save and shows how far it has come (Collect and save). The
/// interface keeps one and gives the worker a clone.
#[derive(Debug, Clone, Default)]
pub struct Control {
    stop: Arc<AtomicBool>,
    /// How far, 0.0 to 1.0, as `f32` bits.
    done: Arc<AtomicU32>,
}

impl Control {
    /// Asks the save to stop at its next piece.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// How far the copying has come, 0.0 to 1.0.
    pub fn progress(&self) -> f32 {
        f32::from_bits(self.done.load(Ordering::Relaxed))
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn set(&self, fraction: f32) {
        self.done.store(fraction.to_bits(), Ordering::Relaxed);
    }
}

/// Why a save did not finish.
#[derive(Debug)]
pub enum Failure {
    /// Stopped on request; nothing in the project was changed.
    Stopped,
    Failed(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Stopped => f.write_str("Stopped"),
            Failure::Failed(why) => f.write_str(why),
        }
    }
}

impl From<String> for Failure {
    fn from(why: String) -> Self {
        Failure::Failed(why)
    }
}

impl From<&str> for Failure {
    fn from(why: &str) -> Self {
        Failure::Failed(why.to_owned())
    }
}

/// Starts saving in the background.
pub fn save(request: SaveRequest) -> Receiver<Result<Saved, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumen-save".into())
        .spawn(move || {
            let _ = tx.send(save_now(request));
        })
        .expect("spawn save thread");
    rx
}

/// Starts a save in the background that `control` can stop and that reports
/// how far its copying has come: Collect and save.
pub fn collect(request: SaveRequest, control: Control) -> Receiver<Result<Saved, Failure>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumen-collect".into())
        .spawn(move || {
            let _ = tx.send(collect_now(request, &control));
        })
        .expect("spawn collect thread");
    rx
}

/// Saves on the calling thread: copies imported audio if asked, writes the
/// media paths relative to the project folder, then writes the project
/// atomically with a backup of the previous version.
pub fn save_now(request: SaveRequest) -> Result<Saved, String> {
    save_inner(request, None).map_err(|failure| failure.to_string())
}

/// [`save_now`] that `control` can stop. A stopped save has written nothing
/// to the project: the files it copied are whole and stay.
pub fn collect_now(request: SaveRequest, control: &Control) -> Result<Saved, Failure> {
    save_inner(request, Some(control))
}

/// What one media file becomes in a save.
struct Plan {
    /// Where it is in the project's media list.
    index: usize,
    source: PathBuf,
    /// Set when the file is copied into the project.
    copy: Option<Copy>,
}

struct Copy {
    folder: PathBuf,
    target: PathBuf,
    /// Lumen's own (rendered, recorded or from a template), not imported.
    owned: bool,
}

/// Where a file of Lumen's own goes in a project, or none when `source` is
/// not one: audio it rendered or recorded, or took from a template.
fn owned_folder(
    source: &Path,
    renders: Option<&Path>,
    recordings: Option<&Path>,
    templates: Option<&Path>,
) -> Option<&'static str> {
    if recordings.is_some_and(|dir| source.starts_with(dir)) {
        return Some(RECORDED);
    }
    if renders.is_some_and(|dir| source.starts_with(dir)) {
        return Some(RENDERED);
    }
    if templates.is_some_and(|dir| source.starts_with(dir)) {
        // The template keeps its audio in the same three folders.
        let kept_in = source.parent().and_then(Path::file_name);
        return Some(if kept_in.is_some_and(|n| n == "Recorded") {
            RECORDED
        } else if kept_in.is_some_and(|n| n == "Rendered") {
            RENDERED
        } else {
            IMPORTED
        });
    }
    None
}

fn save_inner(request: SaveRequest, control: Option<&Control>) -> Result<Saved, Failure> {
    let SaveRequest {
        mut project,
        path: chosen_path,
        normalize_project_path,
        media_files,
        copy_imported,
        renders,
        recordings,
        templates,
        keep_backups,
        state,
        not_over,
    } = request;
    let renders = renders.map(|dir| canonical(&dir));
    let recordings = recordings.map(|dir| canonical(&dir));
    let templates = templates.map(|dir| canonical(&dir));
    let path = if normalize_project_path {
        project_file_for(&chosen_path)
    } else {
        chosen_path
    };
    let dir = path
        .parent()
        .ok_or("the project has no folder")?
        .to_path_buf();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let dir = canonical(&dir);
    let file = dir.join(path.file_name().ok_or("the project has no file name")?);
    if not_over.is_some_and(|open| canonical(&open) == canonical(&file)) {
        return Err(Failure::Failed(
            "That is the open project's own file. Choose another name or folder.".into(),
        ));
    }

    // First decide what happens to each file, so the copying knows its size.
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    let mut planned = HashMap::new();
    for (index, media) in project.media.iter().enumerate() {
        let name = || {
            Path::new(&media.path)
                .file_name()
                .map_or_else(|| media.path.clone(), |n| n.to_string_lossy().into_owned())
        };
        let Some(source) = media_files.get(&media.id) else {
            // Missing on this machine: keep what the project said.
            skipped.push(name());
            continue;
        };
        let source = canonical(source);
        let owned = owned_folder(
            &source,
            renders.as_deref(),
            recordings.as_deref(),
            templates.as_deref(),
        );
        let copy = if (copy_imported || owned.is_some())
            && !source.starts_with(&dir)
            && source.is_file()
        {
            let folder = dir.join(owned.unwrap_or(IMPORTED));
            let target = copy_target(&folder, &source, &planned)?;
            planned.insert(target.clone(), size_of(&source));
            Some(Copy {
                folder,
                target,
                owned: owned.is_some(),
            })
        } else {
            if !source.is_file() {
                skipped.push(name());
            }
            None
        };
        plans.push(Plan {
            index,
            source,
            copy,
        });
    }
    let total: u64 = planned
        .iter()
        .filter(|(target, _)| !target.is_file())
        .map(|(_, size)| size)
        .sum();

    // Then copy, whole files only.
    let (mut copied, mut bytes) = (0, 0);
    for plan in &plans {
        let Some(copy) = &plan.copy else { continue };
        if copy.target.is_file() {
            continue;
        }
        std::fs::create_dir_all(&copy.folder)
            .map_err(|e| format!("Could not make {}: {e}", copy.folder.display()))?;
        copy_file(&plan.source, &copy.target, control, &mut bytes, total)?;
        copied += 1;
    }
    if control.is_some_and(Control::stopped) {
        return Err(Failure::Stopped);
    }

    // The project says where each file is now.
    let mut moved = HashMap::new();
    for plan in plans {
        let media = &mut project.media[plan.index];
        let location = match plan.copy {
            Some(copy) => {
                moved.insert(media.id, copy.target.clone());
                if !copy.owned {
                    media.original_path = Some(plan.source.to_string_lossy().into_owned());
                }
                copy.target
            }
            None => plan.source,
        };
        media.path = media_path_for(&dir, &location);
    }
    lumen_project::save(&project, &file, &SaveOptions { keep_backups })
        .map_err(|e| e.to_string())?;
    if let Some(control) = control {
        control.set(1.0);
    }
    Ok(Saved {
        path: file,
        moved,
        state,
        copied,
        bytes,
        skipped,
    })
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

/// How much of a file is read and written at a time, and so how often a stop
/// is noticed and the progress moves.
const PIECE: usize = 1 << 20;

/// Copies `source` to `target` in pieces, under a temporary name beside the
/// target that is renamed when the copy is whole: a stop, an error or a crash
/// never leaves half a file under the real name. `done` counts the bytes
/// copied so far of `total`, for the progress.
fn copy_file(
    source: &Path,
    target: &Path,
    control: Option<&Control>,
    done: &mut u64,
    total: u64,
) -> Result<(), Failure> {
    let mut input =
        File::open(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.{}.copying", std::process::id()));
    let written = (|| -> Result<(), Failure> {
        let mut output = File::create(&temp).map_err(|e| write_failed(target, &e))?;
        let mut piece = vec![0_u8; PIECE];
        loop {
            if control.is_some_and(Control::stopped) {
                return Err(Failure::Stopped);
            }
            let n = input
                .read(&mut piece)
                .map_err(|e| format!("Could not read {}: {e}", source.display()))?;
            if n == 0 {
                break;
            }
            output
                .write_all(&piece[..n])
                .map_err(|e| write_failed(target, &e))?;
            *done += n as u64;
            if let Some(control) = control {
                control.set((*done as f64 / total.max(1) as f64).min(0.99) as f32);
            }
        }
        output.sync_all().map_err(|e| write_failed(target, &e))?;
        Ok(())
    })();
    match written {
        Ok(()) => std::fs::rename(&temp, target).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            Failure::Failed(write_failed(target, &e))
        }),
        Err(failure) => {
            let _ = std::fs::remove_file(&temp);
            Err(failure)
        }
    }
}

/// What to tell the user when `target` could not be written.
fn write_failed(target: &Path, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::StorageFull {
        format!(
            "The disk is full: {} could not be copied. Files already copied are whole.",
            target.display()
        )
    } else {
        format!("Could not copy to {}: {error}", target.display())
    }
}

/// A size in words: "340 MB", "1.2 GB".
pub fn describe_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// The absolute, resolved form of `path` (or `path` itself if it does not
/// exist), without the `\\?\` prefix Windows adds, which must not end up in
/// project files.
fn canonical(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => resolved,
    }
}

/// Where `source` goes in `folder`: its own name, or "name 2", "name 3"…
/// when another file of that name (and a different size) is already there.
/// The same name and size is taken to be the same file, already copied.
/// `planned` holds the targets this save has given out but not yet written,
/// with the size of the file each will hold, so two files of one name get
/// two targets before either is copied.
fn copy_target(
    folder: &Path,
    source: &Path,
    planned: &HashMap<PathBuf, u64>,
) -> Result<PathBuf, String> {
    let name = source.file_stem().ok_or("a media file has no name")?;
    let extension = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let size = std::fs::metadata(source).map(|m| m.len()).ok();
    for n in 1.. {
        let candidate = if n == 1 {
            folder.join(format!("{}{extension}", name.to_string_lossy()))
        } else {
            folder.join(format!("{} {n}{extension}", name.to_string_lossy()))
        };
        if let Some(&held) = planned.get(&candidate) {
            if Some(held) == size {
                return Ok(candidate);
            }
            continue;
        }
        match std::fs::metadata(&candidate) {
            Err(_) => return Ok(candidate),
            Ok(meta) if Some(meta.len()) == size => return Ok(candidate),
            Ok(_) => {}
        }
    }
    unreachable!()
}

/// A project read from disk, with where its media files are.
pub struct Opened {
    pub project: Project,
    pub path: PathBuf,
    pub media_files: HashMap<Id, PathBuf>,
    /// Media that could not be found.
    pub missing: Vec<String>,
}

/// Starts opening `path` in the background.
pub fn open(path: PathBuf) -> Receiver<Result<Opened, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumen-open".into())
        .spawn(move || {
            let _ = tx.send(open_now(&path));
        })
        .expect("spawn open thread");
    rx
}

pub fn open_now(path: &Path) -> Result<Opened, String> {
    let project = lumen_project::load(path).map_err(|e| e.to_string())?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut media_files = HashMap::new();
    let mut missing = Vec::new();
    for media in &project.media {
        match locate_media(dir, media) {
            MediaLocation::Found(found) => {
                media_files.insert(media.id, found);
            }
            MediaLocation::Missing { .. } => missing.push(media.path.clone()),
        }
    }
    Ok(Opened {
        project,
        path: path.to_path_buf(),
        media_files,
        missing,
    })
}

/// Adds `path` to the front of `recent`, without duplicates, keeping ten.
pub fn remember(recent: &mut Vec<String>, path: &Path) {
    let path = path.to_string_lossy().into_owned();
    recent.retain(|p| *p != path);
    recent.insert(0, path);
    recent.truncate(10);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Imported};
    use lumen_core::time::Ticks;
    use lumen_project::testing::TempDir;

    fn wav(path: &Path, frames: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        let data = (frames * 4) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        bytes.resize(bytes.len() + frames * 4, 0);
        std::fs::write(path, bytes).unwrap();
    }

    fn document_with(files: &[PathBuf]) -> Document {
        let mut doc = Document::new(crate::empty_project());
        let imported = files
            .iter()
            .map(|path| Imported {
                media: doc.fresh_id(),
                path: path.clone(),
                name: path.file_stem().unwrap().to_string_lossy().into_owned(),
                sample_rate: 48_000,
                channels: 2,
                frames: 4_800,
                size_bytes: std::fs::metadata(path).map_or(0, |m| m.len()),
                at: Ticks::ZERO,
                into: crate::document::DropInto::NewTracks {
                    before: None,
                    group: None,
                },
            })
            .collect();
        doc.import(imported);
        doc
    }

    fn request(doc: &Document, path: PathBuf, copy_imported: bool) -> SaveRequest {
        SaveRequest {
            project: doc.project().clone(),
            path,
            normalize_project_path: false,
            media_files: doc.media_files.clone(),
            copy_imported,
            renders: None,
            recordings: None,
            keep_backups: SaveOptions::default().keep_backups,
            state: doc.state(),
            not_over: None,
            templates: None,
        }
    }

    #[test]
    fn save_as_creates_a_project_folder_unless_already_in_one() {
        let tmp = TempDir::new("files-folder");
        let loose = tmp.path().join("Song.lumen");
        assert_eq!(
            project_file_for(&loose),
            tmp.path().join("Song").join("Song.lumen")
        );
        let doc = document_with(&[]);
        let mut request = request(&doc, loose, false);
        request.normalize_project_path = true;
        let saved = save_now(request).unwrap();
        // The worker names the file by its canonical path, which differs
        // from the temp folder's spelling (Windows short names, macOS's
        // /private/var): the same file is what counts.
        assert_eq!(
            std::fs::canonicalize(&saved.path).unwrap(),
            std::fs::canonicalize(tmp.path().join("Song").join("Song.lumen")).unwrap(),
            "Save As resolves the folder on the save worker"
        );
        // Inside a folder named after the project: used as is.
        let named = tmp.path().join("Song").join("Song.lumen");
        std::fs::create_dir_all(named.parent().unwrap()).unwrap();
        assert_eq!(project_file_for(&named), named);
        // Next to another project's Samples folder: used as is.
        let other = tmp.path().join("Album");
        std::fs::create_dir_all(other.join("Samples")).unwrap();
        assert_eq!(
            project_file_for(&other.join("Take 2")),
            other.join("Take 2.lumen")
        );
    }

    #[test]
    fn saving_copies_audio_in_and_reopening_finds_it() {
        let tmp = TempDir::new("files-copy");
        let outside = tmp.path().join("Downloads");
        let (kick, snare) = (outside.join("kick.wav"), outside.join("snare.wav"));
        wav(&kick, 4_800);
        wav(&snare, 4_800);
        let doc = document_with(&[kick.clone(), snare.clone()]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let saved = save_now(request(&doc, path.clone(), true)).unwrap();
        assert_eq!(saved.copied, 2);
        let imported = path.parent().unwrap().join(IMPORTED);
        assert!(imported.join("kick.wav").is_file() && imported.join("snare.wav").is_file());

        // The originals can go: the project has its own copies.
        std::fs::remove_dir_all(&outside).unwrap();
        let opened = open_now(&path).unwrap();
        assert!(opened.missing.is_empty(), "{:?}", opened.missing);
        assert_eq!(opened.project.media[0].path, "Samples/Imported/kick.wav");
        let original = opened.project.media[0].original_path.as_deref().unwrap();
        assert!(!original.starts_with(r"\\?\"), "{original}");
        assert_eq!(opened.project.tracks, doc.project().tracks);
        assert_eq!(opened.media_files.len(), 2);
    }

    #[test]
    fn recorded_audio_is_always_copied_into_samples_recorded() {
        let tmp = TempDir::new("files-recorded");
        let recordings = tmp.path().join("Recordings");
        let take = recordings.join("Vox 1.wav");
        wav(&take, 4_800);
        let doc = document_with(&[take]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let mut asked = request(&doc, path.clone(), false);
        asked.recordings = Some(recordings);
        let saved = save_now(asked).unwrap();
        assert_eq!(saved.copied, 1);
        assert!(
            path.parent()
                .unwrap()
                .join(RECORDED)
                .join("Vox 1.wav")
                .is_file()
        );
        let opened = open_now(&path).unwrap();
        assert!(opened.missing.is_empty(), "{:?}", opened.missing);
        assert_eq!(opened.project.media[0].path, "Samples/Recorded/Vox 1.wav");
        assert_eq!(opened.project.media[0].original_path, None);
    }

    #[test]
    fn rendered_audio_is_always_copied_in_and_is_not_an_import() {
        let tmp = TempDir::new("files-rendered");
        let renders = tmp.path().join("Renders");
        let baked = renders.join("joined-1.wav");
        let library = tmp.path().join("Library").join("kick.wav");
        wav(&baked, 4_800);
        wav(&library, 4_800);
        let doc = document_with(&[baked.clone(), library.clone()]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let mut asked = request(&doc, path.clone(), false);
        asked.renders = Some(renders);
        let saved = save_now(asked).unwrap();
        // The render moves in; the library file, with copying off, stays.
        assert_eq!(saved.copied, 1);
        let folder = path.parent().unwrap().join(RENDERED);
        assert!(folder.join("joined-1.wav").is_file());
        let opened = open_now(&path).unwrap();
        assert!(opened.missing.is_empty(), "{:?}", opened.missing);
        assert_eq!(
            opened.project.media[0].path,
            "Samples/Rendered/joined-1.wav"
        );
        assert_eq!(opened.project.media[0].original_path, None);
        assert!(Path::new(&opened.project.media[1].path).is_absolute());
    }

    #[test]
    fn a_templates_audio_is_always_copied_in_to_the_kind_of_folder_it_was_in() {
        let tmp = TempDir::new("files-template");
        let templates = tmp.path().join("Templates");
        let kept = templates.join("Beat").join("Samples");
        let kick = kept.join("Imported").join("kick.wav");
        let vox = kept.join("Recorded").join("vox.wav");
        let bake = kept.join("Rendered").join("bake.wav");
        let library = tmp.path().join("Library").join("snare.wav");
        for file in [&kick, &vox, &bake, &library] {
            wav(file, 4_800);
        }
        let doc = document_with(&[kick, vox, bake, library]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let mut asked = request(&doc, path.clone(), false);
        asked.templates = Some(templates);
        let saved = save_now(asked).unwrap();
        // The template's three files move in; the library file, with copying
        // off, stays where it is.
        assert_eq!(saved.copied, 3);
        let project = path.parent().unwrap();
        assert!(project.join(IMPORTED).join("kick.wav").is_file());
        assert!(project.join(RECORDED).join("vox.wav").is_file());
        assert!(project.join(RENDERED).join("bake.wav").is_file());
        let opened = open_now(&path).unwrap();
        assert!(opened.missing.is_empty(), "{:?}", opened.missing);
        assert_eq!(opened.project.media[0].path, "Samples/Imported/kick.wav");
        assert_eq!(opened.project.media[1].path, "Samples/Recorded/vox.wav");
        assert_eq!(opened.project.media[2].path, "Samples/Rendered/bake.wav");
        // Copied from a template, none of it was imported by the person.
        for media in &opened.project.media[..3] {
            assert_eq!(media.original_path, None, "{}", media.path);
        }
        assert!(Path::new(&opened.project.media[3].path).is_absolute());
    }

    #[test]
    fn without_copying_paths_point_at_the_originals() {
        let tmp = TempDir::new("files-nocopy");
        let kick = tmp.path().join("Library").join("kick.wav");
        wav(&kick, 4_800);
        let doc = document_with(std::slice::from_ref(&kick));
        let path = tmp.path().join("Song").join("Song.lumen");
        let saved = save_now(request(&doc, path.clone(), false)).unwrap();
        assert_eq!(saved.copied, 0);
        let opened = open_now(&path).unwrap();
        assert!(Path::new(&opened.project.media[0].path).is_absolute());
        assert!(opened.missing.is_empty());
    }

    #[test]
    fn missing_media_is_reported_not_fatal() {
        let tmp = TempDir::new("files-missing");
        let kick = tmp.path().join("kick.wav");
        wav(&kick, 4_800);
        let doc = document_with(std::slice::from_ref(&kick));
        let path = tmp.path().join("Song").join("Song.lumen");
        save_now(request(&doc, path.clone(), false)).unwrap();
        std::fs::remove_file(&kick).unwrap();
        let opened = open_now(&path).unwrap();
        assert_eq!(opened.missing.len(), 1);
        assert_eq!(
            opened.project.tracks.len(),
            1,
            "the clip stays, marked missing"
        );
    }

    #[test]
    fn a_name_clash_gets_a_number_and_the_same_file_is_reused() {
        let tmp = TempDir::new("files-clash");
        let a = tmp.path().join("a").join("kick.wav");
        let b = tmp.path().join("b").join("kick.wav");
        wav(&a, 4_800);
        wav(&b, 9_600);
        let doc = document_with(&[a, b]);
        let path = tmp.path().join("Song").join("Song.lumen");
        save_now(request(&doc, path.clone(), true)).unwrap();
        let imported = path.parent().unwrap().join(IMPORTED);
        assert!(imported.join("kick.wav").is_file() && imported.join("kick 2.wav").is_file());
        // Saving again copies nothing new.
        let again = save_now(request(&doc, path, true)).unwrap();
        assert_eq!(again.copied, 0);
    }

    /// What is left in `folder` that is not a finished file: temporary copies.
    fn leftovers(folder: &Path) -> Vec<String> {
        std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.ends_with(".copying"))
            .collect()
    }

    #[test]
    fn collecting_copies_every_outside_file_and_reports_how_much() {
        let tmp = TempDir::new("files-collect");
        let outside = tmp.path().join("Downloads");
        let (kick, snare) = (outside.join("kick.wav"), outside.join("snare.wav"));
        wav(&kick, 4_800);
        wav(&snare, 9_600);
        let doc = document_with(&[kick.clone(), snare.clone()]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let control = Control::default();
        let saved = collect_now(request(&doc, path.clone(), true), &control).unwrap();
        assert_eq!(saved.copied, 2);
        let wanted =
            std::fs::metadata(&kick).unwrap().len() + std::fs::metadata(&snare).unwrap().len();
        assert_eq!(saved.bytes, wanted);
        assert!(saved.skipped.is_empty(), "{:?}", saved.skipped);
        assert_eq!(control.progress(), 1.0, "a finished collect reads as whole");
        let imported = path.parent().unwrap().join(IMPORTED);
        assert!(imported.join("kick.wav").is_file() && imported.join("snare.wav").is_file());
        assert!(
            leftovers(&imported).is_empty(),
            "{:?}",
            leftovers(&imported)
        );

        // The project stands on its own: the originals can go.
        std::fs::remove_dir_all(&outside).unwrap();
        let opened = open_now(&path).unwrap();
        assert!(opened.missing.is_empty(), "{:?}", opened.missing);
        assert_eq!(opened.media_files.len(), 2);
    }

    #[test]
    fn a_stopped_collect_writes_no_project_and_leaves_no_half_file() {
        let tmp = TempDir::new("files-collect-stop");
        let kick = tmp.path().join("Downloads").join("kick.wav");
        wav(&kick, 4_800);
        let doc = document_with(std::slice::from_ref(&kick));
        let path = tmp.path().join("Song").join("Song.lumen");
        let control = Control::default();
        control.stop();
        let failure = collect_now(request(&doc, path.clone(), true), &control).unwrap_err();
        assert!(matches!(failure, Failure::Stopped), "{failure:?}");
        let folder = path.parent().unwrap().join(IMPORTED);
        assert!(
            !folder.join("kick.wav").exists(),
            "no file under the real name"
        );
        assert!(leftovers(&folder).is_empty(), "{:?}", leftovers(&folder));
        assert!(
            !path.exists(),
            "the project file is written after the last copy"
        );
        assert!(kick.is_file(), "the original is untouched");
    }

    #[test]
    fn media_that_cannot_be_found_is_listed_and_left_as_it_was() {
        let tmp = TempDir::new("files-collect-missing");
        let (here, gone) = (
            tmp.path().join("Downloads").join("kick.wav"),
            tmp.path().join("Downloads").join("snare.wav"),
        );
        wav(&here, 4_800);
        wav(&gone, 4_800);
        let doc = document_with(&[here, gone.clone()]);
        std::fs::remove_file(&gone).unwrap();
        let path = tmp.path().join("Song").join("Song.lumen");
        let saved = collect_now(request(&doc, path.clone(), true), &Control::default()).unwrap();
        assert_eq!(saved.copied, 1);
        assert_eq!(saved.skipped, vec!["snare.wav".to_owned()]);
        let opened = open_now(&path).unwrap();
        assert_eq!(opened.missing.len(), 1, "the missing file stays missing");
        assert_eq!(opened.project.tracks.len(), 2, "its clip is kept");
    }

    #[test]
    fn a_copy_is_never_written_over_the_open_projects_own_file() {
        let tmp = TempDir::new("files-not-over");
        let kick = tmp.path().join("Downloads").join("kick.wav");
        wav(&kick, 4_800);
        let doc = document_with(std::slice::from_ref(&kick));
        let path = tmp.path().join("Song").join("Song.lumen");
        save_now(request(&doc, path.clone(), false)).unwrap();
        let before = std::fs::read(&path).unwrap();

        let mut again = request(&doc, path.clone(), true);
        again.not_over = Some(path.clone());
        let failure = collect_now(again, &Control::default()).unwrap_err();
        assert!(
            failure.to_string().contains("open project's own file"),
            "{failure}"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "the file is as it was"
        );
        assert!(
            !path.parent().unwrap().join(IMPORTED).exists(),
            "nothing was copied before the refusal"
        );

        // Another place is fine.
        let elsewhere = tmp.path().join("Copy").join("Song copy.lumen");
        let mut copy = request(&doc, elsewhere.clone(), true);
        copy.not_over = Some(path);
        let saved = collect_now(copy, &Control::default()).unwrap();
        assert_eq!(saved.copied, 1);
        assert!(elsewhere.is_file());
    }

    #[test]
    fn a_copy_that_fails_leaves_no_temporary_file() {
        let tmp = TempDir::new("files-copy-fails");
        let source = tmp.path().join("kick.wav");
        wav(&source, 4_800);
        // A folder where the file should go: the final rename cannot succeed.
        let target = tmp.path().join("Samples").join("kick.wav");
        std::fs::create_dir_all(&target).unwrap();
        let mut done = 0;
        let failure = copy_file(&source, &target, None, &mut done, 1).unwrap_err();
        assert!(matches!(failure, Failure::Failed(_)), "{failure:?}");
        assert!(leftovers(target.parent().unwrap()).is_empty());
    }

    #[test]
    fn sizes_are_told_in_words() {
        assert_eq!(describe_size(0), "0 bytes");
        assert_eq!(describe_size(999), "999 bytes");
        assert_eq!(describe_size(1_000), "1.0 KB");
        assert_eq!(describe_size(340_000_000), "340 MB");
        assert_eq!(describe_size(1_234_000_000), "1.2 GB");
        assert_eq!(describe_size(250_000_000_000), "250 GB");
    }

    #[test]
    fn view_and_loop_survive_save_and_open() {
        let tmp = TempDir::new("files-view");
        let doc = document_with(&[]);
        let mut project = doc.project().clone();
        project.view.left_beat = 12.0;
        project.view.browser_open = false;
        project.transport.loop_enabled = true;
        let path = tmp.path().join("Song").join("Song.lumen");
        save_now(SaveRequest {
            project: project.clone(),
            ..request(&doc, path.clone(), true)
        })
        .unwrap();
        let opened = open_now(&path).unwrap();
        assert_eq!(opened.project.view, project.view);
        assert_eq!(opened.project.transport, project.transport);
    }

    #[test]
    fn recent_projects_are_unique_newest_first_and_capped() {
        let mut recent = Vec::new();
        for i in 0..12 {
            remember(&mut recent, Path::new(&format!("C:/p/{i}.lumen")));
        }
        remember(&mut recent, Path::new("C:/p/5.lumen"));
        assert_eq!(recent.len(), 10);
        assert_eq!(recent[0], "C:/p/5.lumen");
        assert_eq!(recent.iter().filter(|p| *p == "C:/p/5.lumen").count(), 1);
    }
}
