//! Reading folders for the browser's file tabs (M9 R7c, ADR-0116): one
//! folder's rows for Places, and the walk under a folder that Samples,
//! Projects and search list. A walk never follows a link or a junction (no
//! loops), stops [`DEPTH`] folders down and at [`MOST`] files, and keeps an
//! index of every folder's own modified time, so a walk of a tree it has
//! seen reads again only the folders that changed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// Folders a walk goes down, at most.
pub(crate) const DEPTH: usize = 32;
/// Files one walk lists, at most.
pub(crate) const MOST: usize = 200_000;
/// Folder names one walk keeps, at most: a tree of millions of folders is
/// bounded like one of millions of files.
pub(crate) const FOLDERS: usize = MOST;
/// Rows sent at a time.
pub(crate) const BATCH: usize = 256;

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum Kind {
    Folder,
    Audio,
    Midi,
    Project,
}

impl Kind {
    /// The kind of a file, by its extension; `None` for one Lumen cannot
    /// open.
    pub(crate) fn of_file(path: &Path) -> Option<Kind> {
        let extension = path.extension()?.to_str()?;
        let is = |list: &[&str]| list.iter().any(|e| extension.eq_ignore_ascii_case(e));
        if is(crate::media::EXTENSIONS) {
            Some(Kind::Audio)
        } else if is(crate::midi_files::EXTENSIONS) {
            Some(Kind::Midi)
        } else if extension.eq_ignore_ascii_case("lumen") {
            Some(Kind::Project)
        } else {
            None
        }
    }

    /// Said after a row's name, for screen readers.
    pub(crate) fn said(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Audio => "audio file",
            Kind::Midi => "MIDI file",
            Kind::Project => "project",
        }
    }
}

/// Which files a read or a walk lists (folders are always listed by a
/// read, never by a walk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// Audio and MIDI files: Samples, and Places.
    Sounds,
    /// Projects.
    Projects,
}

impl Wanted {
    pub(crate) fn takes(self, kind: Kind) -> bool {
        match self {
            Wanted::Sounds => matches!(kind, Kind::Audio | Kind::Midi),
            Wanted::Projects => kind == Kind::Project,
        }
    }
}

/// A row: a folder, or a file Lumen can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub path: PathBuf,
    /// What the row shows: a folder's name, a file's without its extension.
    pub name: String,
    pub kind: Kind,
    /// The name in lower case, made once: what the list sorts and searches
    /// by (a list can hold 200,000 rows).
    key: String,
}

impl Entry {
    pub(crate) fn new(path: PathBuf, kind: Kind) -> Self {
        let name = if kind == Kind::Folder {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        } else {
            path.file_stem()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        Self::named(path, name, kind)
    }

    /// A row with a name of its own (a place, a drive).
    pub(crate) fn named(path: PathBuf, name: String, kind: Kind) -> Self {
        let key = name.to_lowercase();
        Self {
            path,
            name,
            kind,
            key,
        }
    }

    /// The name in lower case.
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// The file's whole name, with its extension.
    pub(crate) fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.name.clone(), |n| n.to_string_lossy().into_owned())
    }
}

/// Folders first, then files, each by name without regard to case.
pub(crate) fn order(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    (a.kind != Kind::Folder)
        .cmp(&(b.kind != Kind::Folder))
        .then_with(|| a.key.cmp(&b.key))
        .then_with(|| a.path.cmp(&b.path))
}

/// Sorts rows by [`order`].
pub(crate) fn sort(entries: &mut [Entry]) {
    entries.sort_by(order);
}

/// How a read or a walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    /// Cancelled: what was found is kept.
    Stopped,
    /// It reached [`MOST`] files or [`DEPTH`] folders.
    Limited,
}

/// Whether `kind` (from a directory entry) is a link or a junction.
fn is_link(kind: &fs::FileType) -> bool {
    kind.is_symlink()
}

/// Reads one folder: its folders and the files `wanted` takes, in batches
/// of at most [`BATCH`]. Links are listed by what they point to only when
/// it is a file (a linked folder could lead back up).
pub(crate) fn read_folder(
    dir: &Path,
    wanted: Wanted,
    cancel: &AtomicBool,
    batch: &mut dyn FnMut(Vec<Entry>),
) -> Result<Outcome, String> {
    let read = fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut listed = 0;
    for entry in read.flatten() {
        let stop = if cancel.load(Ordering::Relaxed) {
            Some(Outcome::Stopped)
        } else if listed >= MOST {
            Some(Outcome::Limited)
        } else {
            None
        };
        if let Some(outcome) = stop {
            if !rows.is_empty() {
                batch(rows);
            }
            return Ok(outcome);
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let row = if kind.is_dir() && !is_link(&kind) {
            (!hidden(&path)).then_some(Kind::Folder)
        } else {
            Kind::of_file(&path).filter(|k| wanted.takes(*k))
        };
        let Some(row) = row else {
            continue;
        };
        rows.push(Entry::new(path, row));
        listed += 1;
        if rows.len() >= BATCH {
            batch(std::mem::take(&mut rows));
        }
    }
    if !rows.is_empty() {
        batch(rows);
    }
    Ok(Outcome::Done)
}

/// Whether a folder is one people do not keep sounds in: dot-folders and
/// the system's own.
fn hidden(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.starts_with('.') || n.starts_with('$') || n == "System Volume Information"
    })
}

/// One folder as the index knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Folder {
    /// Its own modified time, in nanoseconds: it changes when something is
    /// added to it, removed or renamed (not when something deeper does).
    modified: u128,
    folders: Vec<String>,
    files: Vec<(String, Kind)>,
}

/// What a walk saw of a tree, kept in the cache folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Index {
    version: u32,
    root: PathBuf,
    /// By path relative to the root.
    folders: HashMap<PathBuf, Folder>,
    /// The walk that made it stopped early.
    pub partial: bool,
}

/// The index file's version.
const INDEX_VERSION: u32 = 1;

impl Index {
    /// The index's bytes, for the cache.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// An index read back, when it is one of this version for `root`.
    pub(crate) fn from_bytes(bytes: &[u8], root: &Path) -> Option<Index> {
        let index: Index = serde_json::from_slice(bytes).ok()?;
        (index.version == INDEX_VERSION && index.root == root).then_some(index)
    }
}

fn modified(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

/// What a walk of one root came to.
#[derive(Debug)]
pub(crate) struct Walked {
    /// What the walk saw, for the next walk of the same tree.
    pub index: Index,
    pub outcome: Outcome,
    /// Files listed.
    pub found: usize,
    /// Folders that could not be read, and the first one's error.
    pub unreadable: usize,
    pub first_error: Option<String>,
}

/// Lists at most `room` files `wanted` takes under `root`, in batches,
/// reading again only the folders whose own time differs from `old`'s.
/// Returns the new index and how the walk ended; an index of a walk that did
/// not finish is marked partial, and a folder that was not read whole (cut
/// short, or unreadable) is left out of it, so the next walk reads it. What
/// a walk keeps is bounded: `room` files and [`FOLDERS`] folder names, and
/// only the files `wanted` takes.
pub(crate) fn walk(
    root: &Path,
    wanted: Wanted,
    old: Option<&Index>,
    room: usize,
    cancel: &AtomicBool,
    batch: &mut dyn FnMut(Vec<Entry>),
) -> Walked {
    let mut walked = Walked {
        index: Index {
            version: INDEX_VERSION,
            root: root.to_owned(),
            folders: HashMap::new(),
            partial: false,
        },
        outcome: Outcome::Done,
        found: 0,
        unreadable: 0,
        first_error: None,
    };
    let mut sink = Sink {
        rows: Vec::new(),
        found: 0,
        batch,
    };
    // Folder names kept, in the index and on the stack.
    let mut folders = 0;
    // Depth first, so a batch holds neighbours.
    let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];
    while let Some((relative, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            walked.outcome = Outcome::Stopped;
            break;
        }
        if sink.found >= room {
            walked.outcome = Outcome::Limited;
            break;
        }
        let dir = root.join(&relative);
        let time = match fs::metadata(&dir) {
            Ok(meta) => modified(&meta),
            Err(error) => {
                walked.failed(&dir, &error.to_string());
                continue;
            }
        };
        let folder = match old.and_then(|o| o.folders.get(&relative)) {
            Some(seen) if seen.modified == time && time != 0 => {
                // A folder the index knows: its files go out from there.
                for (name, kind) in &seen.files {
                    if sink.found >= room {
                        walked.outcome = Outcome::Limited;
                        break;
                    }
                    if wanted.takes(*kind) {
                        sink.push(Entry::new(dir.join(name), *kind));
                    }
                }
                seen.clone()
            }
            _ => {
                let rooms = (room - sink.found, FOLDERS.saturating_sub(folders));
                match read_one(&dir, time, wanted, rooms, cancel, &mut |e| sink.push(e)) {
                    Read::Whole(folder) => folder,
                    Read::Cut(outcome) => {
                        walked.outcome = outcome;
                        break;
                    }
                    Read::Failed(error) => {
                        walked.failed(&dir, &error);
                        continue;
                    }
                }
            }
        };
        if walked.outcome != Outcome::Done {
            break;
        }
        folders += folder.folders.len();
        if folders > FOLDERS {
            walked.outcome = Outcome::Limited;
            break;
        }
        if depth + 1 < DEPTH {
            for name in folder.folders.iter().rev() {
                stack.push((relative.join(name), depth + 1));
            }
        } else if !folder.folders.is_empty() {
            walked.outcome = Outcome::Limited;
        }
        walked.index.folders.insert(relative, folder);
    }
    sink.flush();
    walked.found = sink.found;
    walked.index.partial = walked.outcome != Outcome::Done || walked.unreadable > 0;
    walked
}

/// Where a walk's rows go: out in batches of [`BATCH`] as they are found.
struct Sink<'a> {
    rows: Vec<Entry>,
    found: usize,
    batch: &'a mut dyn FnMut(Vec<Entry>),
}

impl Sink<'_> {
    fn push(&mut self, entry: Entry) {
        self.rows.push(entry);
        self.found += 1;
        if self.rows.len() >= BATCH {
            (self.batch)(std::mem::take(&mut self.rows));
        }
    }

    fn flush(&mut self) {
        if !self.rows.is_empty() {
            (self.batch)(std::mem::take(&mut self.rows));
        }
    }
}

impl Walked {
    fn failed(&mut self, dir: &Path, error: &str) {
        self.unreadable += 1;
        if self.first_error.is_none() {
            self.first_error = Some(format!("{}: {error}", dir.display()));
        }
    }
}

/// One folder read for the index.
enum Read {
    Whole(Folder),
    /// Cut short by a cancel or by the room left: what was read went out,
    /// and the folder is not kept in the index.
    Cut(Outcome),
    Failed(String),
}

/// Reads one folder for the index: the files `wanted` takes, each sent to
/// `found` as it is met, and the folders that are not links. Stops when
/// cancelled, or once it holds `rooms.0` files or `rooms.1` folders (one
/// folder can hold millions of either).
fn read_one(
    dir: &Path,
    modified: u128,
    wanted: Wanted,
    rooms: (usize, usize),
    cancel: &AtomicBool,
    found: &mut dyn FnMut(Entry),
) -> Read {
    let (file_room, folder_room) = rooms;
    let mut folder = Folder {
        modified,
        ..Folder::default()
    };
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) => return Read::Failed(error.to_string()),
    };
    for entry in read {
        if cancel.load(Ordering::Relaxed) {
            return Read::Cut(Outcome::Stopped);
        }
        // An entry that fails to read is skipped; the folder still is whole
        // as far as it can be read.
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && !is_link(&kind) {
            if !hidden(&path) {
                if folder.folders.len() >= folder_room {
                    return Read::Cut(Outcome::Limited);
                }
                folder
                    .folders
                    .push(entry.file_name().to_string_lossy().into_owned());
            }
        } else if kind.is_file()
            && let Some(file) = Kind::of_file(&path).filter(|k| wanted.takes(*k))
        {
            folder
                .files
                .push((entry.file_name().to_string_lossy().into_owned(), file));
            found(Entry::new(path, file));
            if folder.files.len() >= file_room {
                return Read::Cut(Outcome::Limited);
            }
        }
    }
    folder.folders.sort();
    folder.files.sort();
    Read::Whole(folder)
}

/// What Computer lists: the person's Desktop, Downloads and Music, and the
/// drives (on Windows) or the file system's root and its volumes.
pub(crate) fn computer() -> Vec<(String, PathBuf)> {
    let mut places = Vec::new();
    if let Some(home) = home() {
        for name in ["Desktop", "Downloads", "Music"] {
            let path = home.join(name);
            if path.is_dir() {
                places.push((name.to_owned(), path));
            }
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", char::from(letter)));
        if drive_present(letter) {
            places.push((format!("{}:", char::from(letter)), root));
        }
    }
    #[cfg(not(windows))]
    {
        places.push(("Computer".to_owned(), PathBuf::from("/")));
        #[cfg(target_os = "macos")]
        if let Ok(read) = fs::read_dir("/Volumes") {
            for entry in read.flatten() {
                places.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
        }
    }
    places
}

/// Whether the drive with `letter` is there, asked of the system rather
/// than the drive (a network drive that is gone could block a read).
#[cfg(windows)]
fn drive_present(letter: u8) -> bool {
    // SAFETY: takes no arguments and only reads the system's drive mask.
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
    mask & (1 << (letter - b'A')) != 0
}

/// The person's home folder.
pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The system's Music folder: where Samples looks until a folder is chosen.
pub(crate) fn music() -> Option<PathBuf> {
    home().map(|h| h.join("Music"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-walk-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(Entry::file_name).collect()
    }

    fn walk_all(root: &Path, old: Option<&Index>) -> (Vec<Entry>, Index, Outcome) {
        let (all, walked) = walk_room(root, old, MOST);
        (all, walked.index, walked.outcome)
    }

    fn walk_room(root: &Path, old: Option<&Index>, room: usize) -> (Vec<Entry>, Walked) {
        let mut all = Vec::new();
        let walked = walk(
            root,
            Wanted::Sounds,
            old,
            room,
            &AtomicBool::new(false),
            &mut |b| {
                assert!(!b.is_empty() && b.len() <= BATCH);
                all.extend(b);
            },
        );
        sort(&mut all);
        (all, walked)
    }

    #[test]
    fn a_folder_lists_folders_first_then_the_files_lumen_opens() {
        let dir = scratch("read");
        touch(&dir.join("b kick.wav"));
        touch(&dir.join("A Snare.FLAC"));
        touch(&dir.join("notes.txt"));
        touch(&dir.join("groove.mid"));
        touch(&dir.join("Song.lumen"));
        touch(&dir.join("Loops/x.wav"));
        touch(&dir.join(".hidden/y.wav"));
        let mut rows = Vec::new();
        let outcome = read_folder(&dir, Wanted::Sounds, &AtomicBool::new(false), &mut |b| {
            rows.extend(b)
        })
        .unwrap();
        sort(&mut rows);
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(
            names(&rows),
            ["Loops", "A Snare.FLAC", "b kick.wav", "groove.mid"]
        );
        assert_eq!(rows[1].name, "A Snare");
        assert_eq!(rows[1].kind, Kind::Audio);
        assert_eq!(rows[3].kind, Kind::Midi);
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let dir = scratch("missing").join("gone");
        let result = read_folder(&dir, Wanted::Sounds, &AtomicBool::new(false), &mut |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn a_walk_finds_files_deep_down_and_a_cancelled_one_stops() {
        let dir = scratch("walk");
        touch(&dir.join("a/b/c/deep kick.wav"));
        touch(&dir.join("top.ogg"));
        touch(&dir.join("x/Song.lumen"));
        let (all, index, outcome) = walk_all(&dir, None);
        assert_eq!(outcome, Outcome::Done);
        assert!(!index.partial);
        assert_eq!(names(&all), ["deep kick.wav", "top.ogg"]);
        let cancel = AtomicBool::new(true);
        let walked = walk(&dir, Wanted::Sounds, None, MOST, &cancel, &mut |_| {});
        assert_eq!(walked.outcome, Outcome::Stopped);
        assert!(walked.index.partial);
        // Projects are walked for on their own.
        let mut projects = Vec::new();
        walk(
            &dir,
            Wanted::Projects,
            None,
            MOST,
            &AtomicBool::new(false),
            &mut |b| projects.extend(b),
        );
        assert_eq!(names(&projects), ["Song.lumen"]);
    }

    #[test]
    fn a_warm_index_still_finds_a_file_added_three_levels_down() {
        let dir = scratch("fresh");
        touch(&dir.join("one/two/three/first.wav"));
        let (_, index, _) = walk_all(&dir, None);
        let bytes = index.to_bytes();
        let index = Index::from_bytes(&bytes, &dir).expect("read back");
        // A new file changes its own folder's time, which the walk checks.
        std::thread::sleep(std::time::Duration::from_millis(20));
        touch(&dir.join("one/two/three/second.wav"));
        let (all, _, _) = walk_all(&dir, Some(&index));
        assert_eq!(names(&all), ["first.wav", "second.wav"]);
        // An index of another tree is not used.
        assert!(Index::from_bytes(&bytes, &dir.join("one")).is_none());
    }

    #[test]
    fn one_folder_of_many_files_comes_in_batches_and_stops_at_the_limit() {
        let dir = scratch("crowd");
        for n in 0..(2 * BATCH + 10) {
            touch(&dir.join(format!("hit {n:04}.wav")));
        }
        touch(&dir.join("deeper/more.wav"));
        let (all, walked) = walk_room(&dir, None, BATCH + 5);
        assert_eq!(walked.outcome, Outcome::Limited);
        assert_eq!(all.len(), BATCH + 5);
        assert_eq!(walked.found, BATCH + 5);
        // A folder cut short is not kept: the next walk reads it whole.
        assert!(walked.index.partial);
        assert!(walked.index.folders.is_empty());
        let (all, walked) = walk_room(&dir, None, MOST);
        assert_eq!(walked.outcome, Outcome::Done);
        assert_eq!(all.len(), 2 * BATCH + 11);
    }

    #[test]
    fn a_cancel_stops_inside_a_folder_and_keeps_nothing_of_it() {
        let dir = scratch("cancel-in");
        for n in 0..50 {
            touch(&dir.join(format!("{n}.wav")));
        }
        let cancel = AtomicBool::new(false);
        let mut entries = 0;
        let mut sent = 0;
        let read = read_one(
            &dir,
            1,
            Wanted::Sounds,
            (MOST, FOLDERS),
            &AtomicBool::new(false),
            &mut |_| sent += 1,
        );
        assert!(matches!(read, Read::Whole(ref f) if f.files.len() == 50));
        assert_eq!(sent, 50);
        cancel.store(true, Ordering::Relaxed);
        let walked = walk(&dir, Wanted::Sounds, None, MOST, &cancel, &mut |b| {
            entries += b.len();
        });
        assert_eq!(walked.outcome, Outcome::Stopped);
        assert_eq!(entries, 0);
        assert!(walked.index.folders.is_empty());
        let cut = read_one(
            &dir,
            1,
            Wanted::Sounds,
            (MOST, FOLDERS),
            &cancel,
            &mut |_| {},
        );
        assert!(matches!(cut, Read::Cut(Outcome::Stopped)));
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_said_and_not_kept() {
        let dir = scratch("unreadable");
        let (_, walked) = walk_room(&dir.join("gone"), None, MOST);
        assert_eq!(walked.unreadable, 1);
        assert!(walked.first_error.is_some());
        assert!(walked.index.partial);
        assert!(walked.index.folders.is_empty());
        assert!(matches!(
            read_one(
                &dir.join("gone"),
                1,
                Wanted::Sounds,
                (MOST, FOLDERS),
                &AtomicBool::new(false),
                &mut |_| {}
            ),
            Read::Failed(_)
        ));
    }

    #[test]
    fn a_read_keeps_only_what_it_lists_and_stops_at_its_folder_room() {
        let dir = scratch("bounded");
        for n in 0..20 {
            touch(&dir.join(format!("{n}.wav")));
            fs::create_dir_all(dir.join(format!("sub {n}"))).unwrap();
        }
        touch(&dir.join("Song.lumen"));
        let no = AtomicBool::new(false);
        // Projects keep no sounds: a folder of a million samples costs a
        // projects walk nothing to hold.
        let mut sent = Vec::new();
        let read = read_one(&dir, 1, Wanted::Projects, (MOST, FOLDERS), &no, &mut |e| {
            sent.push(e)
        });
        assert!(matches!(read, Read::Whole(ref f) if f.files.len() == 1 && f.folders.len() == 20));
        assert_eq!(names(&sent), ["Song.lumen"]);
        // A folder of more folders than the walk has room for is cut short.
        let read = read_one(&dir, 1, Wanted::Projects, (MOST, 5), &no, &mut |_| {});
        assert!(matches!(read, Read::Cut(Outcome::Limited)));
    }

    #[test]
    fn two_files_of_one_name_in_different_folders_are_both_found() {
        let dir = scratch("twins");
        touch(&dir.join("Kicks/Kick.wav"));
        touch(&dir.join("Old/Kick.wav"));
        let (all, _, _) = walk_all(&dir, None);
        assert_eq!(all.len(), 2);
        assert_ne!(all[0].path, all[1].path);
    }

    #[cfg(unix)]
    #[test]
    fn a_walk_does_not_follow_links() {
        let dir = scratch("links");
        touch(&dir.join("a/kick.wav"));
        std::os::unix::fs::symlink(&dir, dir.join("a/loop")).unwrap();
        let (all, _, outcome) = walk_all(&dir, None);
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(names(&all), ["kick.wav"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_walk_does_not_follow_junctions() {
        let dir = scratch("junctions");
        touch(&dir.join("a/kick.wav"));
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.join("a").join("loop"))
            .arg(&dir)
            .output()
            .is_ok_and(|o| o.status.success());
        if !made {
            return;
        }
        let (all, _, outcome) = walk_all(&dir, None);
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(names(&all), ["kick.wav"]);
    }
}
