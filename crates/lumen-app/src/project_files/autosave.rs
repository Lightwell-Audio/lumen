use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lumen_core::product::PRODUCT_NAME;
use serde::{Deserialize, Serialize};

use crate::files;

pub const DELAY: Duration = Duration::from_secs(60);

pub const INTERVALS: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
];

pub fn default_dir() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join(PRODUCT_NAME)
            .join("Autosave")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support")
            .join(PRODUCT_NAME)
            .join("Autosave")
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?
            .join(PRODUCT_NAME.to_lowercase())
            .join("autosave")
    };
    Some(dir)
}

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    #[serde(default = "legacy_note_version")]
    pub version: u32,
    pub project: Option<String>,
    pub name: String,
    pub written: u64,
}

fn legacy_note_version() -> u32 {
    VERSION
}

fn read_note(path: &Path) -> Option<Note> {
    #[derive(Deserialize)]
    struct Header {
        #[serde(default)]
        version: u32,
        #[serde(default)]
        project: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        written: Option<u64>,
    }
    let text = std::fs::read_to_string(path).ok()?;
    let header: Header = serde_json::from_str(&text).ok()?;
    let version = if header.version == 0 {
        VERSION
    } else {
        header.version
    };
    if version <= VERSION {
        return serde_json::from_str(&text).ok();
    }
    let written = header.written.unwrap_or_else(|| modified_seconds(path));
    let name = header
        .name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            path.file_stem().map_or_else(
                || "Untitled".to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            )
        });
    Some(Note {
        version,
        project: header.project,
        name,
        written,
    })
}

fn modified_seconds(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or_else(now_seconds, |since| since.as_secs())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leftover {
    pub session: String,
    pub file: PathBuf,
    pub note: Note,
}

pub struct Autosave {
    dir: Option<PathBuf>,
    session: String,
    pending_since: Option<Instant>,
    written: Option<u64>,
    writing: Option<Receiver<Written>>,
    lock: Option<File>,
    interval: Duration,
    fixed: bool,
    hurried: Option<Duration>,
    finished: Option<bool>,
    failed: Option<String>,
}

struct Written {
    saved: Result<files::Saved, String>,
    lock: Option<File>,
}

impl Autosave {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            dir,
            session: format!("{}-{}", started.as_millis(), std::process::id()),
            pending_since: None,
            written: None,
            writing: None,
            lock: None,
            interval: DELAY,
            fixed: false,
            hurried: None,
            finished: None,
            failed: None,
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.interval = delay;
        self.fixed = true;
        self
    }

    pub fn set_interval(&mut self, interval: Duration) {
        if !self.fixed {
            self.interval = interval;
        }
    }

    pub fn hurry(&mut self, delay: Duration) {
        self.hurried = Some(delay);
    }

    pub fn unhurry(&mut self) {
        self.hurried = None;
    }

    pub fn delay(&self) -> Duration {
        self.hurried
            .map_or(self.interval, |hurried| hurried.min(self.interval))
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    pub(crate) fn has_saved(&self, state: u64) -> bool {
        self.written == Some(state) && self.writing.is_none()
    }

    pub(crate) fn is_writing(&self) -> bool {
        self.writing.is_some()
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn file(&self) -> Option<PathBuf> {
        Some(
            self.dir
                .as_ref()?
                .join(format!("{}.{}", self.session, files::EXTENSION)),
        )
    }

    fn note_file(&self) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("{}.json", self.session)))
    }

    fn lock_file(&self) -> Option<PathBuf> {
        Some(lock_file(self.dir.as_ref()?, &self.session))
    }

    pub fn adopt(&mut self, leftover: &Leftover) {
        self.session = leftover.session.clone();
        self.written = None;
        self.lock = None;
    }

    pub fn tick(
        &mut self,
        dirty: bool,
        state: u64,
        now: Instant,
        save: impl FnOnce(PathBuf) -> files::SaveRequest,
        note: impl FnOnce() -> Note,
    ) {
        if let Some(rx) = &self.writing
            && let Ok(result) = rx.try_recv()
        {
            self.writing = None;
            if result.lock.is_some() {
                self.lock = result.lock;
            }
            self.finished = Some(result.saved.is_ok());
            match result.saved {
                Ok(saved) => self.written = Some(saved.state),
                Err(e) => crate::trace::log(|| format!("autosave failed: {e}")),
            }
        }
        if !dirty {
            self.pending_since = None;
            if self.written.is_some() {
                let files: Vec<PathBuf> = [self.file(), self.note_file()]
                    .into_iter()
                    .flatten()
                    .collect();
                let (lock, lock_file) = (self.lock.take(), self.lock_file());
                self.written = None;
                let spawned = crate::spawn::spawn_named("lumen-autosave-clear", move || {
                    for file in files {
                        let _ = std::fs::remove_file(file);
                    }
                    release(lock, lock_file);
                });
                if let Err(error) = spawned {
                    crate::trace::log(|| {
                        format!(
                            "autosave: {}",
                            crate::spawn::start_failed("the autosave clean-up", &error)
                        )
                    });
                }
            }
            return;
        }
        if self.written == Some(state) {
            self.pending_since = None;
            return;
        }
        let since = *self.pending_since.get_or_insert(now);
        if now.duration_since(since) < self.delay() || self.writing.is_some() {
            return;
        }
        let (Some(file), Some(note_file)) = (self.file(), self.note_file()) else {
            return;
        };
        self.pending_since = None;
        let note = serde_json::to_string_pretty(&note()).ok();
        let request = save(file);
        let lock_file = self.lock.is_none().then(|| self.lock_file()).flatten();
        let (tx, rx) = std::sync::mpsc::channel();
        let report = tx.clone();
        let spawned = crate::spawn::spawn_named("lumen-autosave", move || {
            let _ = std::fs::create_dir_all(note_file.parent().unwrap_or(Path::new(".")));
            let lock = lock_file.and_then(|path| take_lock(&path));
            if let Some(text) = note {
                let _ = std::fs::write(&note_file, text);
            }
            let saved = files::save_now(request);
            let _ = tx.send(Written { saved, lock });
        });
        if let Err(error) = spawned {
            crate::trace::log(|| {
                format!(
                    "autosave: {}",
                    crate::spawn::start_failed("the autosave", &error)
                )
            });
            self.failed = Some(error.to_string());
            let _ = report.send(Written {
                saved: Err(crate::spawn::start_failed("the autosave", &error)),
                lock: None,
            });
        }
        self.writing = Some(rx);
    }

    pub fn take_failure(&mut self) -> Option<String> {
        self.failed.take()
    }

    pub fn take_finished(&mut self) -> Option<bool> {
        self.finished.take()
    }

    pub fn clear(&mut self) {
        for file in [self.file(), self.note_file()].into_iter().flatten() {
            let _ = std::fs::remove_file(file);
        }
        release(self.lock.take(), self.lock_file());
        self.written = None;
        self.pending_since = None;
    }

    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        self.pending_since
            .map(|since| self.delay().saturating_sub(now.duration_since(since)))
    }
}

pub fn leftovers(dir: &Path, own_session: Option<&str>) -> Vec<Leftover> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Leftover> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "json" {
                return None;
            }
            let session = path.file_stem()?.to_string_lossy().into_owned();
            if Some(session.as_str()) == own_session || is_running(dir, &session) {
                return None;
            }
            let file = path.with_extension(files::EXTENSION);
            if !file.is_file() {
                return None;
            }
            let note = read_note(&path)?;
            Some(Leftover {
                session,
                file,
                note,
            })
        })
        .collect();
    found.sort_by_key(|l| std::cmp::Reverse(l.note.written));
    found
}

pub fn discard(leftover: &Leftover) {
    let _ = std::fs::remove_file(&leftover.file);
    let _ = std::fs::remove_file(leftover.file.with_extension("json"));
    let _ = std::fs::remove_file(leftover.file.with_extension("lock"));
}

fn lock_file(dir: &Path, session: &str) -> PathBuf {
    dir.join(format!("{session}.lock"))
}

fn take_lock(path: &Path) -> Option<File> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

fn release(lock: Option<File>, path: Option<PathBuf>) {
    if let (Some(lock), Some(path)) = (lock, path) {
        drop(lock);
        let _ = std::fs::remove_file(path);
    }
}

fn is_running(dir: &Path, session: &str) -> bool {
    let Ok(file) = File::open(lock_file(dir, session)) else {
        return false;
    };
    matches!(
        file.try_lock_shared(),
        Err(std::fs::TryLockError::WouldBlock)
    )
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn ago(written: u64) -> String {
    let seconds = now_seconds().saturating_sub(written);
    match seconds {
        0..=59 => "less than a minute ago".into(),
        60..=119 => "a minute ago".into(),
        120..=3599 => format!("{} minutes ago", seconds / 60),
        3600..=7199 => "an hour ago".into(),
        7200..=86_399 => format!("{} hours ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Imported};
    use lumen_core::time::Ticks;
    use lumen_project::testing::TempDir;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn frozen_note(version: u32) -> Note {
        Note {
            version,
            project: Some("C:/Users/Ada/Documents/Lumen/Projects/Night Drive.lumen".into()),
            name: "Night Drive".into(),
            written: 1_762_310_400,
        }
    }

    #[test]
    fn a_legacy_note_without_a_version_still_says_which_project_it_copied() {
        let note = read_note(&fixture("autosave-note-legacy.json")).expect("the legacy note reads");
        assert_eq!(note, frozen_note(VERSION));
        assert_eq!(
            note.version, VERSION,
            "an unversioned note is version 1's shape"
        );
    }

    #[test]
    fn a_version_1_note_says_which_project_it_copied() {
        let note = read_note(&fixture("autosave-note-v1.json")).expect("the note reads");
        assert_eq!(note, frozen_note(VERSION));
    }

    #[test]
    fn a_leftover_is_offered_from_either_note_shape() {
        for (name, session) in [
            ("autosave-note-legacy.json", "legacy"),
            ("autosave-note-v1.json", "version-1"),
        ] {
            let tmp = TempDir::new(&format!("autosave-{session}"));
            std::fs::write(
                tmp.path().join(format!("{session}.{}", files::EXTENSION)),
                "{}",
            )
            .unwrap();
            std::fs::copy(fixture(name), tmp.path().join(format!("{session}.json"))).unwrap();
            let found = leftovers(tmp.path(), None);
            assert_eq!(found.len(), 1, "{name}");
            assert_eq!(found[0].note, frozen_note(VERSION), "{name}");
        }
    }

    #[test]
    fn a_note_a_newer_version_wrote_still_offers_its_copy() {
        let tmp = TempDir::new("autosave-newer-note");
        let copy = tmp.path().join(format!("future.{}", files::EXTENSION));
        std::fs::write(&copy, "{}").unwrap();
        std::fs::write(
            tmp.path().join("future.json"),
            r#"{"version":2,"project":null,"name":"Night Drive","written":300,"a new field":true}"#,
        )
        .unwrap();
        let found = leftovers(tmp.path(), None);
        assert_eq!(
            found.len(),
            1,
            "the copy is the person's work: still offered"
        );
        assert_eq!(found[0].file, copy, "the copy itself, not the note");
        assert_eq!(found[0].note.version, 2);
        assert_eq!(
            (found[0].note.name.as_str(), found[0].note.written),
            ("Night Drive", 300)
        );
        assert!(copy.exists(), "and its copy is kept");
    }

    #[test]
    fn a_note_a_newer_version_wrote_whose_fields_changed_is_still_offered() {
        let tmp = TempDir::new("autosave-changed-note");
        let copy = tmp.path().join(format!("future.{}", files::EXTENSION));
        std::fs::write(&copy, "{}").unwrap();
        std::fs::write(
            tmp.path().join("future.json"),
            r#"{"version":3,"title":{"text":"Night Drive"}}"#,
        )
        .unwrap();
        let found = leftovers(tmp.path(), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, copy);
        assert_eq!(found[0].note.name, "future", "named by its session");
        assert!(found[0].note.written > 0, "aged from the note file");
    }

    #[test]
    fn an_autosave_note_declares_its_version() {
        let tmp = TempDir::new("autosave-versioned-note");
        let doc = edited_document();
        let autosave = autosaved(tmp.path(), &doc);
        let path = tmp.path().join(format!("{}.json", autosave.session()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 1"), "{text}");
        drop(autosave);
        let found = leftovers(tmp.path(), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].note.version, VERSION);
    }

    #[test]
    fn the_interval_is_the_preference_shortened_while_recording() {
        let mut autosave = Autosave::new(None);
        assert_eq!(autosave.delay(), DELAY);
        autosave.set_interval(Duration::from_secs(30));
        assert_eq!(autosave.delay(), Duration::from_secs(30));
        autosave.hurry(Duration::from_secs(10));
        assert_eq!(autosave.delay(), Duration::from_secs(10));
        autosave.set_interval(Duration::from_secs(120));
        assert_eq!(autosave.delay(), Duration::from_secs(10));
        autosave.unhurry();
        assert_eq!(autosave.delay(), Duration::from_secs(120));
    }

    #[test]
    fn a_fixed_delay_wins_over_the_preference_and_recording_never_lengthens_it() {
        let mut autosave = Autosave::new(None).with_delay(Duration::from_millis(50));
        autosave.set_interval(Duration::from_secs(30));
        assert_eq!(autosave.delay(), Duration::from_millis(50));
        autosave.hurry(Duration::from_secs(10));
        assert_eq!(autosave.delay(), Duration::from_millis(50));
        autosave.unhurry();
        assert_eq!(autosave.delay(), Duration::from_millis(50));
    }

    fn request(doc: &Document, file: PathBuf) -> files::SaveRequest {
        files::SaveRequest {
            project: doc.project().clone(),
            path: file,
            normalize_project_path: false,
            media_files: doc.media_files.clone(),
            copy_imported: false,
            renders: None,
            recordings: None,
            templates: None,
            keep_backups: 0,
            state: doc.state(),
            not_over: None,
        }
    }

    fn edited_document() -> Document {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![Imported {
            media,
            path: PathBuf::from("C:/audio/kick.wav"),
            name: "kick".into(),
            sample_rate: 48_000,
            channels: 2,
            frames: 48_000,
            size_bytes: 192_044,
            tempo: None,
            at: Ticks::ZERO,
            into: crate::document::DropInto::NewTracks {
                before: None,
                group: None,
            },
        }]);
        doc
    }

    fn wait_for_write(autosave: &mut Autosave, doc: &Document, now: Instant) {
        let deadline = lumen_testkit::Deadline::new();
        loop {
            autosave.tick(
                true,
                doc.state(),
                now,
                |f| request(doc, f),
                || unreachable!(),
            );
            if autosave.written == Some(doc.state()) {
                return;
            }
            deadline.check(format_args!(
                "the autosave was never written: still writing {}, last write ended {:?} \
                 (Some(false): it failed)",
                autosave.writing.is_some(),
                autosave.finished
            ));
            std::thread::sleep(lumen_testkit::POLL);
        }
    }

    #[test]
    fn a_crashed_session_leaves_a_recoverable_copy() {
        let tmp = TempDir::new("autosave-crash");
        let doc = edited_document();
        let mut autosave = Autosave::new(Some(tmp.path().to_path_buf()));
        let start = Instant::now();
        let note = || Note {
            version: VERSION,
            project: None,
            name: "Untitled".into(),
            written: now_seconds(),
        };
        autosave.tick(true, doc.state(), start, |f| request(&doc, f), note);
        assert!(leftovers(tmp.path(), None).is_empty());
        autosave.tick(true, doc.state(), start + DELAY, |f| request(&doc, f), note);
        wait_for_write(&mut autosave, &doc, start + DELAY);

        drop(autosave);
        let found = leftovers(tmp.path(), Some("another-session"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].note.name, "Untitled");
        let recovered = files::open_now(&found[0].file).unwrap();
        assert_eq!(recovered.project.tracks, doc.project().tracks);
        assert_eq!(recovered.project.media, doc.project().media);
        discard(&found[0]);
        assert!(leftovers(tmp.path(), None).is_empty());
    }

    #[test]
    fn saving_or_a_normal_close_leaves_nothing() {
        let tmp = TempDir::new("autosave-clean");
        let doc = edited_document();
        let mut autosave = Autosave::new(Some(tmp.path().to_path_buf()));
        let start = Instant::now();
        let note = || Note {
            version: VERSION,
            project: None,
            name: "Untitled".into(),
            written: now_seconds(),
        };
        autosave.tick(true, doc.state(), start, |f| request(&doc, f), note);
        autosave.tick(true, doc.state(), start + DELAY, |f| request(&doc, f), note);
        wait_for_write(&mut autosave, &doc, start + DELAY);
        let files = |dir: &Path| std::fs::read_dir(dir).unwrap().count();
        assert_eq!(files(tmp.path()), 3);
        autosave.tick(
            false,
            doc.state(),
            start + DELAY,
            |_| unreachable!(),
            || unreachable!(),
        );
        lumen_testkit::wait_until("the copy to be removed", || files(tmp.path()) == 0);
        autosave.clear();
        assert!(leftovers(tmp.path(), None).is_empty());
    }

    fn autosaved(dir: &Path, doc: &Document) -> Autosave {
        let mut autosave = Autosave::new(Some(dir.to_path_buf()));
        let start = Instant::now();
        let note = || Note {
            version: VERSION,
            project: None,
            name: "Untitled".into(),
            written: now_seconds(),
        };
        autosave.tick(true, doc.state(), start, |f| request(doc, f), note);
        autosave.tick(true, doc.state(), start + DELAY, |f| request(doc, f), note);
        wait_for_write(&mut autosave, doc, start + DELAY);
        autosave
    }

    #[test]
    fn a_running_session_is_not_offered_to_another() {
        let tmp = TempDir::new("autosave-running");
        let doc = edited_document();
        let running = autosaved(tmp.path(), &doc);
        assert!(leftovers(tmp.path(), Some("another-session")).is_empty());
        drop(running);
        let found = leftovers(tmp.path(), Some("another-session"));
        assert_eq!(found.len(), 1);
        discard(&found[0]);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_normal_close_releases_the_lock_and_its_file() {
        let tmp = TempDir::new("autosave-release");
        let doc = edited_document();
        let mut autosave = autosaved(tmp.path(), &doc);
        assert!(autosave.lock.is_some(), "locked with the first autosave");
        autosave.clear();
        assert!(autosave.lock.is_none());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_recovered_session_is_locked_again_by_its_next_autosave() {
        let tmp = TempDir::new("autosave-adopt");
        let doc = edited_document();
        drop(autosaved(tmp.path(), &doc));
        let leftover = leftovers(tmp.path(), None).remove(0);
        let mut recovering = autosaved(tmp.path(), &doc);
        recovering.clear();
        recovering.adopt(&leftover);
        let start = Instant::now();
        let note = || Note {
            version: VERSION,
            project: None,
            name: "Untitled".into(),
            written: now_seconds(),
        };
        recovering.tick(true, doc.state(), start, |f| request(&doc, f), note);
        recovering.tick(true, doc.state(), start + DELAY, |f| request(&doc, f), note);
        wait_for_write(&mut recovering, &doc, start + DELAY);
        assert!(leftovers(tmp.path(), Some("a-third-session")).is_empty());
    }

    #[test]
    fn the_newest_leftover_comes_first() {
        let tmp = TempDir::new("autosave-order");
        for (session, written) in [("a", 100), ("b", 300), ("c", 200)] {
            std::fs::write(tmp.path().join(format!("{session}.lumen")), "{}").unwrap();
            let note = Note {
                version: VERSION,
                project: Some(format!("C:/p/{session}.lumen")),
                name: session.into(),
                written,
            };
            std::fs::write(
                tmp.path().join(format!("{session}.json")),
                serde_json::to_string(&note).unwrap(),
            )
            .unwrap();
        }
        let found = leftovers(tmp.path(), Some("c"));
        let names: Vec<&str> = found.iter().map(|l| l.note.name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
    }

    #[test]
    fn ages_read_naturally() {
        let now = now_seconds();
        assert_eq!(ago(now), "less than a minute ago");
        assert_eq!(ago(now - 90), "a minute ago");
        assert_eq!(ago(now - 600), "10 minutes ago");
        assert_eq!(ago(now - 3 * 3600), "3 hours ago");
    }
}
