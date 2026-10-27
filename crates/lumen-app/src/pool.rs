//! The project's media pool (M9 slice R4b, ADR-0106): one row for each
//! audio file the project holds, with where it lives, whether it is on this
//! machine and how many clips play it. The rows are made from the project and
//! the document's file map alone, so building them touches no disk; whether a
//! file is still there is [`absent`], which a worker runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lumen_project::edit::media;
use lumen_project::model::{Id, Media, Project};

use crate::files::{IMPORTED, RECORDED, RENDERED};

/// Where a media file lives, for the pool's "Where" column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    /// Copied into the project's `Samples/Imported`.
    Imported,
    /// Recorded in Lumen (`Samples/Recorded`, or still in the work folder).
    Recorded,
    /// Rendered by Lumen: a join, bounce or freeze.
    Rendered,
    /// Elsewhere inside the project folder.
    Project,
    /// Outside the project folder: moving the project will not take it along.
    Outside,
}

impl Home {
    pub fn label(self) -> &'static str {
        match self {
            Home::Imported => "Imported",
            Home::Recorded => "Recorded",
            Home::Rendered => "Rendered",
            Home::Project => "Project",
            Home::Outside => "Outside",
        }
    }
}

/// One media file of the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: Id,
    /// The file's name without its folder or extension.
    pub name: String,
    /// The path the project stores for it (relative to the project folder
    /// when inside it), for saying where a missing file was.
    pub path: String,
    pub home: Home,
    /// Not found on this machine.
    pub missing: bool,
    /// How many clips (and frozen tracks) play it; 0 is unused.
    pub uses: usize,
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
    pub size_bytes: u64,
}

impl Row {
    pub fn unused(&self) -> bool {
        self.uses == 0
    }
}

/// Lumen's own work folders, whose files a save copies into the project.
#[derive(Debug, Clone, Default)]
pub struct Folders {
    pub renders: Option<PathBuf>,
    pub recordings: Option<PathBuf>,
}

/// The pool of `project`: its files by name, then by id. `files` is where each
/// one is on this machine; an id with no entry is missing.
pub fn rows(project: &Project, files: &HashMap<Id, PathBuf>, folders: &Folders) -> Vec<Row> {
    let counts = media::counts(project);
    let mut rows: Vec<Row> = project
        .media
        .iter()
        .map(|media| Row {
            id: media.id,
            name: name_of(media),
            path: media.path.clone(),
            home: home_of(media, files.get(&media.id), folders),
            missing: !files.contains_key(&media.id),
            uses: counts.get(&media.id).copied().unwrap_or(0),
            frames: media.frames,
            channels: media.channels,
            sample_rate: media.sample_rate,
            size_bytes: media.size_bytes,
        })
        .collect();
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.0.cmp(&b.id.0))
    });
    rows
}

/// The ids in `files` whose file is no longer on disk. Reads the disk: run it
/// on a worker.
pub fn absent(files: &HashMap<Id, PathBuf>) -> Vec<Id> {
    let mut ids: Vec<Id> = files
        .iter()
        .filter(|(_, path)| !path.is_file())
        .map(|(id, _)| *id)
        .collect();
    ids.sort_by_key(|id| id.0);
    ids
}

fn name_of(media: &Media) -> String {
    Path::new(&media.path).file_stem().map_or_else(
        || media.path.clone(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

fn home_of(media: &Media, file: Option<&PathBuf>, folders: &Folders) -> Home {
    // A file still in a work folder is Lumen's own and a save will copy it in.
    if let Some(file) = file {
        if folders
            .recordings
            .as_deref()
            .is_some_and(|d| file.starts_with(d))
        {
            return Home::Recorded;
        }
        if folders
            .renders
            .as_deref()
            .is_some_and(|d| file.starts_with(d))
        {
            return Home::Rendered;
        }
    }
    let inside = |folder: &str| {
        media
            .path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    if inside(RECORDED) {
        Home::Recorded
    } else if inside(RENDERED) {
        Home::Rendered
    } else if inside(IMPORTED) {
        Home::Imported
    } else if Path::new(&media.path).is_absolute() {
        Home::Outside
    } else {
        Home::Project
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, ClipContent, Color, Frozen, Track, TrackKind};
    use lumen_project::testing::{TempDir, project_of};

    use super::*;

    fn media(id: u64, path: &str) -> Media {
        Media {
            id: Id(id),
            path: path.into(),
            original_path: None,
            frames: 48_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 1000 + id,
        }
    }

    fn playing(clip: u64, media: u64) -> Arc<Clip> {
        Arc::new(Clip {
            id: Id(clip),
            name: format!("C{clip}"),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Ticks::ZERO,
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            content: ClipContent::Audio {
                media: Id(media),
                offset: 0,
                gain_db: 0.0,
            },
        })
    }

    fn project() -> Project {
        let mut track = Track::new(Id(1), "T", TrackKind::Audio, Color(0));
        track.clips = vec![playing(10, 1), playing(11, 1), playing(12, 2)];
        let mut p = project_of("pool", vec![track]);
        p.media = vec![
            media(1, "Samples/Imported/Kick.wav"),
            media(2, "samples/loose/snare.wav"),
            media(3, "Samples/Recorded/Take 1.wav"),
            media(4, "Samples/Rendered/join.wav"),
            media(5, &outside()),
        ];
        p
    }

    /// A path that is absolute on every platform (`C:/…` is relative on Unix).
    fn outside() -> String {
        let path = std::env::temp_dir().join("Music").join("pad.wav");
        path.to_string_lossy().replace('\\', "/")
    }

    fn present(p: &Project) -> HashMap<Id, PathBuf> {
        p.media
            .iter()
            .map(|m| (m.id, PathBuf::from(&m.path)))
            .collect()
    }

    #[test]
    fn rows_are_sorted_by_name_and_count_their_clips() {
        let p = project();
        let rows = rows(&p, &present(&p), &Folders::default());
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["join", "Kick", "pad", "snare", "Take 1"]);
        let kick = rows.iter().find(|r| r.name == "Kick").unwrap();
        assert_eq!(kick.uses, 2);
        assert!(!kick.unused());
        let pad = rows.iter().find(|r| r.name == "pad").unwrap();
        assert!(pad.unused());
    }

    #[test]
    fn where_a_file_lives_comes_from_its_path() {
        let p = project();
        let rows = rows(&p, &present(&p), &Folders::default());
        let home = |name: &str| rows.iter().find(|r| r.name == name).unwrap().home;
        assert_eq!(home("Kick"), Home::Imported);
        assert_eq!(home("snare"), Home::Project);
        assert_eq!(home("Take 1"), Home::Recorded);
        assert_eq!(home("join"), Home::Rendered);
        assert_eq!(home("pad"), Home::Outside);
    }

    #[test]
    fn a_file_still_in_a_work_folder_is_lumens_own() {
        let mut p = project();
        p.media = vec![
            media(6, "/work/Recordings/a.wav"),
            media(7, "/work/Renders/b.wav"),
        ];
        let files = present(&p);
        let folders = Folders {
            renders: Some(PathBuf::from("/work/Renders")),
            recordings: Some(PathBuf::from("/work/Recordings")),
        };
        let rows = rows(&p, &files, &folders);
        assert_eq!(rows[0].home, Home::Recorded);
        assert_eq!(rows[1].home, Home::Rendered);
    }

    #[test]
    fn an_id_with_no_file_is_missing() {
        let p = project();
        let mut files = present(&p);
        files.remove(&Id(2));
        let rows = rows(&p, &files, &Folders::default());
        let missing: Vec<_> = rows.iter().filter(|r| r.missing).map(|r| r.id).collect();
        assert_eq!(missing, [Id(2)]);
    }

    #[test]
    fn a_frozen_track_counts_as_a_use() {
        let mut p = project();
        let mut frozen = Track::new(Id(2), "F", TrackKind::Audio, Color(0));
        frozen.frozen = Some(Frozen {
            id: Id(20),
            media: Id(4),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            mono: false,
        });
        p.tracks.push(Arc::new(frozen));
        let rows = rows(&p, &present(&p), &Folders::default());
        assert_eq!(rows.iter().find(|r| r.id == Id(4)).unwrap().uses, 1);
    }

    #[test]
    fn absent_finds_files_that_left_the_disk() {
        let dir = TempDir::new("pool-absent");
        let here = dir.path().join("here.wav");
        std::fs::write(&here, b"x").unwrap();
        let files = HashMap::from([(Id(1), here), (Id(2), dir.path().join("gone.wav"))]);
        assert_eq!(absent(&files), [Id(2)]);
    }
}
