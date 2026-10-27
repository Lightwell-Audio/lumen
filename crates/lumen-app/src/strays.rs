//! Files that belong to no project (M9 slice R4b, ADR-0106): audio in the
//! project's `Samples` folders that no media entry names, and what Lumen's work
//! folders (`Renders`, `Recordings`) still hold of cancelled renders and
//! discarded takes. The pool lists them and moves them to the system's trash
//! when asked; nothing here ever erases a file.
//!
//! A work-folder file is left alone when something may still want it: the
//! open document (undone renders stay in its file map so a redo finds them),
//! an autosave a crash left (it names the file, and recovery needs it), or a
//! take of a session that can still be recovered or is still recording.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lumen_project::model::{Id, Media};

use crate::files::{IMPORTED, RECORDED, RENDERED};
use crate::media::is_audio_file;
use crate::take_worker::{Sidecar, sidecar_path};

/// Which folder a stray file is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The project's `Samples` folders.
    Samples,
    /// Lumen's folder of rendered audio.
    Renders,
    /// Lumen's folder of recorded audio.
    Recordings,
}

/// One file nothing wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stray {
    pub path: PathBuf,
    pub place: Place,
    pub size: u64,
    /// A take a crash interrupted (it has its sidecar, which goes with it).
    pub unfinished: bool,
}

/// What the scan needs to know. All plain data, so the scan runs on a worker.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// The project's folder, when it has been saved.
    pub project_dir: Option<PathBuf>,
    /// The project's media entries, saved paths and all.
    pub media: Vec<Media>,
    /// Where the document says each file is now.
    pub files: Vec<(Id, PathBuf)>,
    pub renders: Option<PathBuf>,
    pub recordings: Option<PathBuf>,
    /// Where autosaves are kept, and this session's name in it.
    pub autosaves: Option<PathBuf>,
    pub session: Option<String>,
}

/// What the scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strays {
    pub files: Vec<Stray>,
}

impl Strays {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|s| s.size).sum()
    }
}

/// Looks through the folders for files nothing wants. Reads the disk: run it
/// on a worker.
pub fn scan(inputs: &Inputs) -> Strays {
    let mut wanted: HashSet<PathBuf> = inputs.files.iter().map(|(_, p)| canonical(p)).collect();
    if let Some(dir) = &inputs.project_dir {
        for media in &inputs.media {
            let resolved = media
                .path
                .split('/')
                .fold(dir.clone(), |path, part| path.join(part));
            wanted.insert(canonical(&resolved));
            if let Some(original) = &media.original_path {
                wanted.insert(canonical(Path::new(original)));
            }
        }
    }
    let autosaves = autosave_texts(inputs.autosaves.as_deref());
    let mut files = Vec::new();

    if let Some(dir) = &inputs.project_dir {
        for folder in [IMPORTED, RECORDED, RENDERED] {
            let folder = folder.split('/').fold(dir.clone(), |p, part| p.join(part));
            for path in audio_in(&folder) {
                if !wanted.contains(&canonical(&path)) {
                    files.push(stray(path, Place::Samples, false));
                }
            }
        }
    }
    for (folder, place) in [
        (&inputs.renders, Place::Renders),
        (&inputs.recordings, Place::Recordings),
    ] {
        let Some(folder) = folder else { continue };
        for path in audio_in(folder) {
            if wanted.contains(&canonical(&path)) {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if name.is_some_and(|name| autosaves.iter().any(|text| text.contains(&name))) {
                continue;
            }
            let sidecar = sidecar_path(&path);
            let unfinished = sidecar.is_file();
            if unfinished && !is_abandoned(&sidecar, inputs) {
                continue;
            }
            files.push(stray(path, place, unfinished));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Strays { files }
}

/// A take's sidecar whose session is gone for good: not this one, and with no
/// autosave left that could recover it. One that cannot be read is not.
fn is_abandoned(sidecar: &Path, inputs: &Inputs) -> bool {
    let Some(sidecar): Option<Sidecar> = std::fs::read(sidecar)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    else {
        return false;
    };
    if sidecar.session.is_empty() || inputs.session.as_deref() == Some(sidecar.session.as_str()) {
        return false;
    }
    let recoverable = inputs
        .autosaves
        .as_ref()
        .is_some_and(|dir| dir.join(format!("{}.lumen", sidecar.session)).is_file());
    !recoverable
}

fn stray(path: PathBuf, place: Place, unfinished: bool) -> Stray {
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    Stray {
        path,
        place,
        size,
        unfinished,
    }
}

/// The audio files directly in `folder`.
fn audio_in(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_audio_file(p))
        .collect()
}

/// The text of each autosave in `dir`: a crashed session's project names the
/// files it plays, so a work-folder file it names is not a stray.
fn autosave_texts(dir: Option<&Path>) -> Vec<String> {
    let Some(dir) = dir else { return Vec::new() };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == crate::files::EXTENSION))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

fn canonical(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => resolved,
    }
}

/// What a clear came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleared {
    /// Files (and sidecars with them) now in the trash.
    pub moved: usize,
    pub bytes: u64,
    /// Files that stayed, with why.
    pub failed: Vec<(PathBuf, String)>,
}

/// Moves `strays` (and the sidecars of unfinished takes) to the system's trash.
/// A file the trash refuses stays where it is.
pub fn clear(strays: &[Stray]) -> Cleared {
    clear_with(strays, |path| {
        trash::delete(path).map_err(|e| e.to_string())
    })
}

/// [`clear`] with the mover given, so tests do not fill the real trash.
pub fn clear_with(strays: &[Stray], mut mover: impl FnMut(&Path) -> Result<(), String>) -> Cleared {
    let mut cleared = Cleared::default();
    for stray in strays {
        // The file first: a sidecar left without its file is harmless, a file
        // left without its sidecar would pass for a finished take.
        match mover(&stray.path) {
            Ok(()) => {
                cleared.moved += 1;
                cleared.bytes += stray.size;
            }
            Err(why) => {
                cleared.failed.push((stray.path.clone(), why));
                continue;
            }
        }
        if stray.unfinished {
            let _ = mover(&sidecar_path(&stray.path));
        }
    }
    cleared
}

#[cfg(test)]
mod tests {
    use lumen_project::testing::TempDir;

    use super::*;

    fn touch(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0_u8; bytes]).unwrap();
    }

    /// The one step the other tests leave to a mover that only records. Run by
    /// hand (MT-PROJ-2 step 5): the file is then in the system's trash.
    #[test]
    #[ignore = "moves a file to the real system trash"]
    fn the_real_trash_takes_the_file() {
        let tmp = TempDir::new("strays-real-trash");
        let path = tmp.path().join("lumen-mt-proj-2-trash-check.wav");
        touch(&path, 100);
        let stray = Stray {
            path: path.clone(),
            place: Place::Samples,
            size: 100,
            unfinished: false,
        };
        let cleared = clear(&[stray]);
        assert_eq!(
            (cleared.moved, cleared.bytes),
            (1, 100),
            "{:?}",
            cleared.failed
        );
        assert!(!path.exists(), "gone from where it was");
    }

    fn media(id: u64, path: &str) -> Media {
        Media {
            id: Id(id),
            path: path.into(),
            original_path: None,
            frames: 1,
            channels: 1,
            sample_rate: 48_000,
            size_bytes: 1,
        }
    }

    fn names(strays: &Strays) -> Vec<String> {
        strays
            .files
            .iter()
            .map(|s| s.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_sample_no_entry_names_is_a_stray() {
        let dir = TempDir::new("stray-samples");
        touch(&dir.path().join("Samples/Imported/kept.wav"), 10);
        touch(&dir.path().join("Samples/Imported/orphan.wav"), 20);
        touch(&dir.path().join("Samples/Recorded/take.wav"), 30);
        touch(&dir.path().join("Samples/Imported/notes.txt"), 5);
        let inputs = Inputs {
            project_dir: Some(dir.path().to_path_buf()),
            media: vec![
                media(1, "Samples/Imported/kept.wav"),
                media(2, "Samples/Recorded/take.wav"),
            ],
            ..Inputs::default()
        };
        let strays = scan(&inputs);
        assert_eq!(names(&strays), ["orphan.wav"]);
        assert_eq!(strays.bytes(), 20);
        assert_eq!(strays.files[0].place, Place::Samples);
    }

    #[test]
    fn an_entry_nothing_plays_still_keeps_its_file() {
        // Remove unused drops the entry first; only then is the file a stray.
        let dir = TempDir::new("stray-unused");
        touch(&dir.path().join("Samples/Imported/idle.wav"), 10);
        let mut inputs = Inputs {
            project_dir: Some(dir.path().to_path_buf()),
            media: vec![media(1, "Samples/Imported/idle.wav")],
            ..Inputs::default()
        };
        assert!(scan(&inputs).files.is_empty());
        inputs.media.clear();
        assert_eq!(names(&scan(&inputs)), ["idle.wav"]);
    }

    #[test]
    fn the_documents_files_are_never_strays_even_undone_ones() {
        let dir = TempDir::new("stray-doc");
        let renders = dir.path().join("Renders");
        touch(&renders.join("live.wav"), 10);
        touch(&renders.join("undone.wav"), 10);
        touch(&renders.join("cancelled.wav"), 10);
        let inputs = Inputs {
            renders: Some(renders.clone()),
            // The file map keeps the undone render, so a redo finds it.
            files: vec![
                (Id(1), renders.join("live.wav")),
                (Id(2), renders.join("undone.wav")),
            ],
            ..Inputs::default()
        };
        let strays = scan(&inputs);
        assert_eq!(names(&strays), ["cancelled.wav"]);
        assert_eq!(strays.files[0].place, Place::Renders);
    }

    #[test]
    fn a_file_a_crashed_autosave_names_is_kept_for_its_recovery() {
        let dir = TempDir::new("stray-autosave");
        let renders = dir.path().join("Renders");
        let autosaves = dir.path().join("Autosave");
        touch(&renders.join("needed.wav"), 10);
        touch(&renders.join("free.wav"), 10);
        std::fs::create_dir_all(&autosaves).unwrap();
        std::fs::write(
            autosaves.join("s1.lumen"),
            format!("{{\"path\": \"{}\"}}", renders.join("needed.wav").display()),
        )
        .unwrap();
        let inputs = Inputs {
            renders: Some(renders),
            autosaves: Some(autosaves),
            ..Inputs::default()
        };
        assert_eq!(names(&scan(&inputs)), ["free.wav"]);
    }

    fn take(recordings: &Path, name: &str, session: &str) {
        touch(&recordings.join(name), 10);
        let sidecar = Sidecar {
            session: session.into(),
            track: 1,
            start: 0,
            rate: 48_000,
            channels: 1,
            wraps: Vec::new(),
            silence: 0,
            looped: None,
        };
        std::fs::write(
            sidecar_path(&recordings.join(name)),
            serde_json::to_vec(&sidecar).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn an_unfinished_take_is_a_stray_only_when_no_session_can_recover_it() {
        let dir = TempDir::new("stray-takes");
        let recordings = dir.path().join("Recordings");
        let autosaves = dir.path().join("Autosave");
        std::fs::create_dir_all(&autosaves).unwrap();
        take(&recordings, "mine.wav", "now");
        take(&recordings, "recoverable.wav", "crashed");
        take(&recordings, "abandoned.wav", "discarded");
        std::fs::write(autosaves.join("crashed.lumen"), "{}").unwrap();
        let inputs = Inputs {
            recordings: Some(recordings),
            autosaves: Some(autosaves),
            session: Some("now".into()),
            ..Inputs::default()
        };
        let strays = scan(&inputs);
        assert_eq!(names(&strays), ["abandoned.wav"]);
        assert!(strays.files[0].unfinished);
        assert_eq!(strays.files[0].place, Place::Recordings);
    }

    #[test]
    fn a_sidecar_that_cannot_be_read_keeps_its_take() {
        let dir = TempDir::new("stray-bad-sidecar");
        let recordings = dir.path().join("Recordings");
        touch(&recordings.join("odd.wav"), 10);
        std::fs::write(sidecar_path(&recordings.join("odd.wav")), "{ not json").unwrap();
        let inputs = Inputs {
            recordings: Some(recordings),
            ..Inputs::default()
        };
        assert!(scan(&inputs).files.is_empty());
    }

    #[test]
    fn clearing_moves_the_file_and_its_sidecar_and_keeps_what_the_trash_refuses() {
        let strays = vec![
            Stray {
                path: PathBuf::from("a.wav"),
                place: Place::Recordings,
                size: 5,
                unfinished: true,
            },
            Stray {
                path: PathBuf::from("b.wav"),
                place: Place::Renders,
                size: 7,
                unfinished: false,
            },
        ];
        let mut asked = Vec::new();
        let cleared = clear_with(&strays, |path| {
            asked.push(path.to_path_buf());
            if path == Path::new("b.wav") {
                Err("in use".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(cleared.moved, 1);
        assert_eq!(cleared.bytes, 5);
        assert_eq!(
            cleared.failed,
            [(PathBuf::from("b.wav"), "in use".to_owned())]
        );
        assert_eq!(
            asked,
            [
                PathBuf::from("a.wav"),
                sidecar_path(Path::new("a.wav")),
                PathBuf::from("b.wav")
            ]
        );
    }
}
