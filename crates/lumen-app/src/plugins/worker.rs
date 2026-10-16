//! The scan thread: reads the cache, scans, saves the cache. It never
//! touches the UI; it sends messages the frame loop polls (M6 slice 1).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::time::Duration;

use eframe::egui;
use lumen_plugin_catalog::cache::{Cache, Entry};
use lumen_plugin_catalog::scan::{Event, Mode, Scanner, Summary};

/// What the thread tells the frame loop.
pub(super) enum Message {
    /// The cache read from disk, and why it was thrown away, if it was.
    Loaded { cache: Cache, note: Option<String> },
    /// The plan: how many modules will be loaded.
    Planned { total: usize },
    /// One module finished.
    Scanned {
        done: usize,
        total: usize,
        entry: Entry,
    },
    /// Over. `cache` is the cache as it now stands; `saved` is how writing
    /// it went.
    Finished {
        summary: Summary,
        cache: Cache,
        saved: Result<(), String>,
    },
}

/// A per-module fault for the test plugin, chosen by the module's file name
/// (end-to-end tests).
pub(crate) type Faults = fn(&str) -> Option<&'static str>;

/// What to do.
pub(super) struct Job {
    /// The scan program; `None` reads the cache and stops.
    pub host: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub mode: Mode,
    pub cache_path: Option<PathBuf>,
    /// The cache to start from; `None` reads it from `cache_path`.
    pub cache: Option<Cache>,
    pub deadline: Option<Duration>,
    pub faults: Option<Faults>,
}

pub(super) fn spawn(job: Job, tx: Sender<Message>, cancel: Arc<AtomicBool>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("lumen-plugin-scan".into())
        .spawn(move || run(job, &tx, &cancel, &ctx))
        .expect("spawn the plugin scan thread");
}

fn run(job: Job, tx: &Sender<Message>, cancel: &AtomicBool, ctx: &egui::Context) {
    let send = |message| {
        let _ = tx.send(message);
        ctx.request_repaint();
    };
    let mut cache = match job.cache {
        Some(cache) => cache,
        None => {
            let (cache, note) = read(job.cache_path.as_deref());
            send(Message::Loaded {
                cache: cache.clone(),
                note,
            });
            cache
        }
    };
    let Some(host) = job.host else {
        return;
    };
    let mut scanner = Scanner::new(host);
    if let Some(deadline) = job.deadline {
        scanner.deadline = deadline;
    }
    if let Some(faults) = job.faults {
        scanner.child_env = Some(Box::new(move |module| {
            let stem = module
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            faults(&stem)
                .map(|fault| vec![("LUMEN_TEST_PLUGIN_FAULT".to_owned(), fault.to_owned())])
                .unwrap_or_default()
        }));
    }
    let summary = scanner.scan(
        &job.roots,
        &mut cache,
        job.mode,
        cancel,
        &mut |event| match event {
            Event::Planned { total, .. } => send(Message::Planned { total }),
            Event::Scanned { done, total, entry } => send(Message::Scanned {
                done,
                total,
                entry: entry.clone(),
            }),
        },
    );
    let saved = write(job.cache_path.as_deref(), &cache);
    send(Message::Finished {
        summary,
        cache,
        saved,
    });
}

/// The cache on disk. A missing file is a first run, not a problem; one
/// that cannot be read or understood is thrown away with a note.
fn read(path: Option<&Path>) -> (Cache, Option<String>) {
    let Some(path) = path else {
        return (Cache::new(), None);
    };
    match std::fs::read(path) {
        Ok(bytes) => {
            let loaded = Cache::load(&bytes);
            (loaded.cache, loaded.discarded)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Cache::new(), None),
        Err(e) => (
            Cache::new(),
            Some(format!("the plugin cache could not be read ({e})")),
        ),
    }
}

fn write(path: Option<&Path>, cache: &Cache) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    lumen_project::write_atomic(path, &cache.to_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lumen-app-plugins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_missing_cache_is_a_first_run_without_a_note() {
        let dir = temp("missing");
        let (cache, note) = read(Some(&dir.join("plugins.json")));
        assert!(cache.entries().is_empty());
        assert_eq!(note, None);
    }

    #[test]
    fn a_damaged_cache_is_thrown_away_with_a_note() {
        let dir = temp("damaged");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plugins.json");
        std::fs::write(&file, b"{ nope").unwrap();
        let (cache, note) = read(Some(&file));
        assert!(cache.entries().is_empty());
        assert!(note.unwrap().contains("damaged"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_is_written_into_a_folder_that_does_not_exist_yet_and_reads_back() {
        let dir = temp("write");
        let file = dir.join("deep/plugins.json");
        let cache = Cache::new();
        write(Some(&file), &cache).unwrap();
        let (back, note) = read(Some(&file));
        assert_eq!((back, note), (cache, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_path_means_nothing_is_read_or_written() {
        assert_eq!(read(None).1, None);
        assert!(write(None, &Cache::new()).is_ok());
    }
}
