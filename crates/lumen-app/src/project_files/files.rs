use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};

use lumen_project::model::{Id, Project};
use lumen_project::{MediaLocation, SaveOptions, locate_media, media_path_for};

use crate::spawn::{spawn_named, start_failed};

pub const IMPORTED: &str = "Samples/Imported";
pub const RENDERED: &str = "Samples/Rendered";
pub const RECORDED: &str = "Samples/Recorded";
pub const EXTENSION: &str = "lumen";

pub fn is_project(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

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

pub struct SaveRequest {
    pub project: Project,
    pub path: PathBuf,
    pub normalize_project_path: bool,
    pub media_files: HashMap<Id, PathBuf>,
    pub copy_imported: bool,
    pub renders: Option<PathBuf>,
    pub recordings: Option<PathBuf>,
    pub templates: Option<PathBuf>,
    pub keep_backups: usize,
    pub state: u64,
    pub not_over: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Saved {
    pub path: PathBuf,
    pub moved: HashMap<Id, PathBuf>,
    pub state: u64,
    pub copied: usize,
    pub bytes: u64,
    pub skipped: Vec<String>,
    pub backup_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Control {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicU32>,
}

impl Control {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.done.load(Ordering::Relaxed))
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub(crate) fn set(&self, fraction: f32) {
        self.done.store(fraction.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum Failure {
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

pub fn save(request: SaveRequest) -> Receiver<Result<Saved, String>> {
    let (tx, rx) = mpsc::channel();
    let report = tx.clone();
    if let Err(error) = spawn_named("lumen-save", move || {
        let _ = tx.send(save_now(request));
    }) {
        let _ = report.send(Err(start_failed("the save", &error)));
    }
    rx
}

pub fn collect(request: SaveRequest, control: Control) -> Receiver<Result<Saved, Failure>> {
    let (tx, rx) = mpsc::channel();
    let report = tx.clone();
    if let Err(error) = spawn_named("lumen-collect", move || {
        let _ = tx.send(collect_now(request, &control));
    }) {
        let _ = report.send(Err(Failure::Failed(start_failed("the save", &error))));
    }
    rx
}

pub(crate) fn save_now(request: SaveRequest) -> Result<Saved, String> {
    save_inner(request, None).map_err(|failure| failure.to_string())
}

pub fn collect_now(request: SaveRequest, control: &Control) -> Result<Saved, Failure> {
    save_inner(request, Some(control))
}

struct Plan {
    index: usize,
    source: PathBuf,
    copy: Option<Copy>,
}

struct Copy {
    folder: PathBuf,
    target: PathBuf,
    owned: bool,
}

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
            planned.insert(target.clone(), source.clone());
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
        .map(|(_, source)| size_of(source))
        .sum();

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
    let report = lumen_project::save_with_report(&project, &file, &SaveOptions { keep_backups })
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
        backup_error: report.backup_error,
    })
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

const PIECE: usize = 1 << 20;

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

fn canonical(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => resolved,
    }
}

fn copy_target(
    folder: &Path,
    source: &Path,
    planned: &HashMap<PathBuf, PathBuf>,
) -> Result<PathBuf, String> {
    let name = source.file_stem().ok_or("a media file has no name")?;
    let extension = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1.. {
        let candidate = if n == 1 {
            folder.join(format!("{}{extension}", name.to_string_lossy()))
        } else {
            folder.join(format!("{} {n}{extension}", name.to_string_lossy()))
        };
        if let Some(held) = planned.get(&candidate) {
            if held == source {
                return Ok(candidate);
            }
            continue;
        }
        if !candidate.exists() {
            return Ok(candidate);
        }
        if same_contents(source, &candidate) {
            return Ok(candidate);
        }
    }
    unreachable!()
}

fn same_contents(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let (Ok(meta_a), Ok(meta_b)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
        return false;
    };
    if !meta_b.is_file() || meta_a.len() != meta_b.len() {
        return false;
    }
    let (Ok(mut file_a), Ok(mut file_b)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let (mut piece_a, mut piece_b) = (vec![0u8; PIECE], vec![0u8; PIECE]);
    loop {
        let Ok(read) = file_a.read(&mut piece_a) else {
            return false;
        };
        if read == 0 {
            return true;
        }
        if file_b.read_exact(&mut piece_b[..read]).is_err() || piece_a[..read] != piece_b[..read] {
            return false;
        }
    }
}

pub struct Opened {
    pub project: Project,
    pub path: PathBuf,
    pub media_files: HashMap<Id, PathBuf>,
}

pub fn open(path: PathBuf) -> Receiver<Result<Opened, String>> {
    let (tx, rx) = mpsc::channel();
    let report = tx.clone();
    if let Err(error) = spawn_named("lumen-open", move || {
        let _ = tx.send(open_now(&path));
    }) {
        let _ = report.send(Err(start_failed("opening the project", &error)));
    }
    rx
}

pub fn open_now(path: &Path) -> Result<Opened, String> {
    let project = lumen_project::load(path).map_err(|e| e.to_string())?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut media_files = HashMap::new();
    for media in &project.media {
        match locate_media(dir, media) {
            MediaLocation::Found(found) => {
                media_files.insert(media.id, found);
            }
            MediaLocation::Missing { .. } => {}
        }
    }
    Ok(Opened {
        project,
        path: path.to_path_buf(),
        media_files,
    })
}

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

    fn missing(opened: &Opened) -> Vec<String> {
        opened
            .project
            .media
            .iter()
            .filter(|m| !opened.media_files.contains_key(&m.id))
            .map(|m| m.path.clone())
            .collect()
    }

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
                tempo: None,
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
        assert_eq!(
            std::fs::canonicalize(&saved.path).unwrap(),
            std::fs::canonicalize(tmp.path().join("Song").join("Song.lumen")).unwrap(),
            "Save As resolves the folder on the save worker"
        );
        let named = tmp.path().join("Song").join("Song.lumen");
        std::fs::create_dir_all(named.parent().unwrap()).unwrap();
        assert_eq!(project_file_for(&named), named);
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

        std::fs::remove_dir_all(&outside).unwrap();
        let opened = open_now(&path).unwrap();
        assert!(missing(&opened).is_empty(), "{:?}", missing(&opened));
        assert_eq!(opened.project.media[0].path, "Samples/Imported/kick.wav");
        let original = opened.project.media[0].original_path.as_deref().unwrap();
        assert!(!original.starts_with(r"\\?\"), "{original}");
        assert_eq!(opened.project.tracks, doc.project().tracks);
        assert_eq!(opened.media_files.len(), 2);
    }

    #[test]
    fn a_backup_that_cannot_be_kept_is_reported_and_the_save_stands() {
        let tmp = TempDir::new("files-backup");
        let doc = document_with(&[]);
        let path = tmp.path().join("Song.lumen");
        let first = save_now(request(&doc, path.clone(), false)).unwrap();
        assert_eq!(first.backup_error, None, "there is nothing to keep yet");

        std::fs::write(tmp.path().join("Backups"), b"not a folder").unwrap();
        let mut project = doc.project().clone();
        project.view.left_beat = 12.0;
        let second = save_now(SaveRequest {
            project: project.clone(),
            ..request(&doc, path.clone(), false)
        })
        .unwrap();
        let why = second.backup_error.expect("the failed backup is reported");
        assert!(
            why.contains("the previous version could not be backed up"),
            "{why}"
        );
        assert_eq!(
            open_now(&path).unwrap().project.view,
            project.view,
            "the new content is on disk"
        );
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
        assert!(missing(&opened).is_empty(), "{:?}", missing(&opened));
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
        assert_eq!(saved.copied, 1);
        let folder = path.parent().unwrap().join(RENDERED);
        assert!(folder.join("joined-1.wav").is_file());
        let opened = open_now(&path).unwrap();
        assert!(missing(&opened).is_empty(), "{:?}", missing(&opened));
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
        assert_eq!(saved.copied, 3);
        let project = path.parent().unwrap();
        assert!(project.join(IMPORTED).join("kick.wav").is_file());
        assert!(project.join(RECORDED).join("vox.wav").is_file());
        assert!(project.join(RENDERED).join("bake.wav").is_file());
        let opened = open_now(&path).unwrap();
        assert!(missing(&opened).is_empty(), "{:?}", missing(&opened));
        assert_eq!(opened.project.media[0].path, "Samples/Imported/kick.wav");
        assert_eq!(opened.project.media[1].path, "Samples/Recorded/vox.wav");
        assert_eq!(opened.project.media[2].path, "Samples/Rendered/bake.wav");
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
        assert!(missing(&opened).is_empty());
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
        assert_eq!(missing(&opened).len(), 1);
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
        let again = save_now(request(&doc, path, true)).unwrap();
        assert_eq!(again.copied, 0);
    }

    #[test]
    fn different_files_of_one_name_and_size_never_share_a_copy() {
        let tmp = TempDir::new("files-same-size");
        let a = tmp.path().join("a").join("Vox 1.wav");
        let b = tmp.path().join("b").join("Vox 1.wav");
        wav(&a, 4_800);
        wav(&b, 4_800);
        let mut bytes = std::fs::read(&b).unwrap();
        *bytes.last_mut().unwrap() = 7;
        std::fs::write(&b, &bytes).unwrap();
        let doc = document_with(&[a.clone(), b.clone()]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let saved = save_now(request(&doc, path.clone(), true)).unwrap();
        assert_eq!(saved.copied, 2, "both are copied");
        let ids: Vec<_> = doc.project().media.iter().map(|m| m.id).collect();
        assert_ne!(saved.moved[&ids[0]], saved.moved[&ids[1]]);
        assert_eq!(
            std::fs::read(&saved.moved[&ids[0]]).unwrap(),
            std::fs::read(&a).unwrap()
        );
        assert_eq!(std::fs::read(&saved.moved[&ids[1]]).unwrap(), bytes);

        let c = tmp.path().join("c").join("Vox 1.wav");
        wav(&c, 4_800);
        let mut third = std::fs::read(&c).unwrap();
        *third.last_mut().unwrap() = 9;
        std::fs::write(&c, &third).unwrap();
        let later = document_with(std::slice::from_ref(&c));
        let saved = save_now(request(&later, path, true)).unwrap();
        assert_eq!(saved.copied, 1);
        let id = later.project().media[0].id;
        assert_eq!(saved.moved[&id].file_name().unwrap(), "Vox 1 3.wav");
        assert_eq!(std::fs::read(&saved.moved[&id]).unwrap(), third);
    }

    #[test]
    fn one_file_used_twice_is_copied_once() {
        let tmp = TempDir::new("files-twice");
        let kick = tmp.path().join("Library").join("kick.wav");
        wav(&kick, 4_800);
        let doc = document_with(&[kick.clone(), kick]);
        let path = tmp.path().join("Song").join("Song.lumen");
        let saved = save_now(request(&doc, path, true)).unwrap();
        assert_eq!(saved.copied, 1);
        let ids: Vec<_> = doc.project().media.iter().map(|m| m.id).collect();
        assert_eq!(saved.moved[&ids[0]], saved.moved[&ids[1]]);
    }

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

        std::fs::remove_dir_all(&outside).unwrap();
        let opened = open_now(&path).unwrap();
        assert!(missing(&opened).is_empty(), "{:?}", missing(&opened));
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
        assert_eq!(missing(&opened).len(), 1, "the missing file stays missing");
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
