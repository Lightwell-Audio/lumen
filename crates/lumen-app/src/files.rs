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
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use lumen_project::model::{Id, Project};
use lumen_project::{MediaLocation, SaveOptions, locate_media, media_path_for};

/// Where audio copied into a project goes, inside its folder.
pub const IMPORTED: &str = "Samples/Imported";
/// The file extension of projects.
pub const EXTENSION: &str = "lumen";

/// Where a project chosen in Save As is written. A file picked inside an
/// existing project folder (one holding another project file or a `Samples`
/// folder) or a folder already named after the project is used as it is;
/// anywhere else, a folder named after the project is created for it, as
/// Ableton does.
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
    /// Where each media file is now.
    pub media_files: HashMap<Id, PathBuf>,
    /// Copy audio from outside the project folder into `Samples/Imported`.
    pub copy_imported: bool,
    /// Earlier versions kept in `Backups` (none for autosaves).
    pub keep_backups: usize,
    /// The document state being saved (edits during the save stay unsaved).
    pub state: u64,
}

pub struct Saved {
    pub path: PathBuf,
    /// Media files that moved (copied into the project).
    pub moved: HashMap<Id, PathBuf>,
    pub state: u64,
    /// Files copied into the project.
    pub copied: usize,
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

/// Saves on the calling thread: copies imported audio if asked, writes the
/// media paths relative to the project folder, then writes the project
/// atomically with a backup of the previous version.
pub fn save_now(request: SaveRequest) -> Result<Saved, String> {
    let SaveRequest {
        mut project,
        path,
        media_files,
        copy_imported,
        keep_backups,
        state,
    } = request;
    let dir = path
        .parent()
        .ok_or("the project has no folder")?
        .to_path_buf();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let dir = canonical(&dir);
    let mut moved = HashMap::new();
    let mut copied = 0;
    for media in &mut project.media {
        let Some(source) = media_files.get(&media.id) else {
            // Missing on this machine: keep what the project said.
            continue;
        };
        let source = canonical(source);
        let location = if copy_imported && !source.starts_with(&dir) && source.is_file() {
            let target = copy_target(&dir.join(IMPORTED), &source)?;
            if !target.is_file() {
                std::fs::create_dir_all(dir.join(IMPORTED))
                    .and_then(|()| std::fs::copy(&source, &target).map(|_| ()))
                    .map_err(|e| format!("Could not copy {}: {e}", source.display()))?;
                copied += 1;
            }
            moved.insert(media.id, target.clone());
            media.original_path = Some(source.to_string_lossy().into_owned());
            target
        } else {
            source
        };
        media.path = media_path_for(&dir, &location);
    }
    let file = dir.join(path.file_name().ok_or("the project has no file name")?);
    lumen_project::save(&project, &file, &SaveOptions { keep_backups })
        .map_err(|e| e.to_string())?;
    Ok(Saved {
        path: file,
        moved,
        state,
        copied,
    })
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
fn copy_target(folder: &Path, source: &Path) -> Result<PathBuf, String> {
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
            media_files: doc.media_files.clone(),
            copy_imported,
            keep_backups: SaveOptions::default().keep_backups,
            state: doc.state(),
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
