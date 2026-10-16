//! Plugins in the app (M6 slice 1, ADR-0068): which are installed, found by
//! scanning in child processes, never in Lumen. This module owns the scan
//! thread's lifetime and the catalog the browser shows; the thread itself is
//! `worker`, the user's choices are `settings`, and nothing here blocks a
//! frame: the loop calls [`Plugins::poll`] and the thread's messages arrive
//! as they are sent.

mod settings;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, SystemTime};

use eframe::egui;
use lumen_plugin_catalog::cache::Cache;
use lumen_plugin_catalog::catalog::{Catalog, builtin_devices};
use lumen_plugin_catalog::discover::standard_roots;
use lumen_plugin_catalog::scan::{Mode, Summary};

pub(crate) use settings::PluginSettings;
pub(crate) use worker::Faults;
use worker::{Job, Message};

/// Where plugins and the scan program come from.
#[derive(Clone)]
pub(crate) enum Source {
    /// This machine: the standard folders and the program beside Lumen.
    System,
    /// No plugins at all (`--plugins off`; most tests).
    Off,
    /// Chosen by the caller (`--plugin-roots`, tests).
    Fixed(Fixed),
}

/// A caller's choice of folders, scan program and cache file.
#[derive(Clone)]
pub(crate) struct Fixed {
    pub roots: Vec<PathBuf>,
    pub host: PathBuf,
    pub cache: Option<PathBuf>,
    pub deadline: Option<Duration>,
    pub faults: Option<Faults>,
}

impl Source {
    /// Reads `--plugins off` and `--plugin-roots`, `--plugin-host`,
    /// `--plugin-cache` (folders separated by `;`).
    pub(crate) fn from_args(args: &[String]) -> Source {
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1).cloned())
        };
        if value("--plugins").is_some_and(|v| v == "off") {
            return Source::Off;
        }
        match value("--plugin-roots") {
            Some(roots) => Source::Fixed(Fixed {
                roots: roots
                    .split(';')
                    .filter(|r| !r.is_empty())
                    .map(PathBuf::from)
                    .collect(),
                host: value("--plugin-host")
                    .map(PathBuf::from)
                    .unwrap_or_else(default_host),
                cache: value("--plugin-cache").map(PathBuf::from),
                deadline: None,
                faults: None,
            }),
            None => Source::System,
        }
    }
}

/// The scan program sits beside the Lumen executable.
pub(crate) fn default_host() -> PathBuf {
    let name = if cfg!(windows) {
        "lumen-plugin-host.exe"
    } else {
        "lumen-plugin-host"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// What the browser says about scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    /// Plugins are switched off for this run.
    Off,
    /// Reading the cache.
    Loading,
    /// Nothing is happening.
    Idle,
    /// Scanning: `done` of `total` modules loaded so far.
    Scanning { done: usize, total: usize },
    /// A scan could not start; the message says why and what to do.
    Failed(String),
}

pub(crate) struct Plugins {
    source: Source,
    cache_path: Option<PathBuf>,
    cache: Cache,
    catalog: Catalog,
    status: Status,
    rx: Option<Receiver<Message>>,
    cancel: Arc<AtomicBool>,
    /// The cache changed since the catalog was built.
    stale: bool,
    /// A note for the person: the cache was thrown away, or could not be
    /// written. Cleared by [`Plugins::dismiss_note`].
    note: Option<String>,
    last: Option<Summary>,
    finished_at: Option<SystemTime>,
}

/// Said when the scan program is not where it should be.
pub(crate) fn host_missing(host: &std::path::Path) -> String {
    format!(
        "The plugin scanner was not found at {}. Build it with: cargo build --release -p lumen-plugin-host",
        host.display()
    )
}

impl Plugins {
    pub(crate) fn off() -> Self {
        Self {
            source: Source::Off,
            cache_path: None,
            cache: Cache::new(),
            catalog: Catalog::build(&Cache::new(), builtin_devices()),
            status: Status::Off,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            stale: false,
            note: None,
            last: None,
            finished_at: None,
        }
    }

    /// Starts reading the cache and, when `settings` ask, scanning.
    pub(crate) fn start(
        source: Source,
        settings_path: Option<&std::path::Path>,
        settings: &PluginSettings,
        ctx: &egui::Context,
    ) -> Self {
        let mut plugins = Self::off();
        let cache_path = match &source {
            Source::Off => return plugins,
            Source::System => settings_path
                .and_then(|p| p.parent())
                .map(|dir| dir.join("plugin-cache.json")),
            Source::Fixed(fixed) => fixed.cache.clone(),
        };
        plugins.source = source;
        plugins.cache_path = cache_path;
        plugins.status = Status::Loading;
        plugins.run(None, settings.scan_at_startup.then_some(Mode::Changed), settings, ctx);
        plugins
    }

    pub(crate) fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub(crate) fn dismiss_note(&mut self) {
        self.note = None;
    }

    pub(crate) fn last(&self) -> Option<&Summary> {
        self.last.as_ref()
    }

    pub(crate) fn finished_at(&self) -> Option<SystemTime> {
        self.finished_at
    }

    pub(crate) fn is_off(&self) -> bool {
        matches!(self.source, Source::Off)
    }

    /// A scan (or the first read of the cache) is under way.
    pub(crate) fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Looks for new and changed plugins; with [`Mode::All`], loads every
    /// one again. Does nothing while a scan runs.
    pub(crate) fn rescan(&mut self, mode: Mode, settings: &PluginSettings, ctx: &egui::Context) {
        if self.is_off() || self.busy() {
            return;
        }
        self.run(Some(self.cache.clone()), Some(mode), settings, ctx);
    }

    /// Forgets what is known about one module and loads it again: Retry on
    /// a blocked plugin, or "rescan this plugin".
    pub(crate) fn retry(
        &mut self,
        module: &str,
        settings: &PluginSettings,
        ctx: &egui::Context,
    ) {
        if self.is_off() || self.busy() {
            return;
        }
        self.cache.forget(module);
        self.stale = true;
        self.run(Some(self.cache.clone()), Some(Mode::Changed), settings, ctx);
    }

    /// Stops a scan: the children are killed and what finished is kept.
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn run(
        &mut self,
        cache: Option<Cache>,
        scan: Option<Mode>,
        settings: &PluginSettings,
        ctx: &egui::Context,
    ) {
        let (roots, host, deadline, faults) = match &self.source {
            Source::Off => return,
            Source::System => (standard_roots(), default_host(), None, None),
            Source::Fixed(f) => (f.roots.clone(), f.host.clone(), f.deadline, f.faults),
        };
        let mut roots = roots;
        roots.extend(settings.folders.iter().map(PathBuf::from));
        // A scan needs its program; without it the cache is still read, and
        // the browser says what is missing.
        let host = scan.and_then(|_| {
            if host.is_file() {
                Some(host)
            } else {
                self.status = Status::Failed(host_missing(&host));
                None
            }
        });
        if scan.is_some() && host.is_none() && cache.is_some() {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        if host.is_some() {
            self.status = Status::Scanning { done: 0, total: 0 };
        }
        worker::spawn(
            Job {
                host,
                roots,
                mode: scan.unwrap_or(Mode::Changed),
                cache_path: self.cache_path.clone(),
                cache,
                deadline,
                faults,
            },
            tx,
            Arc::clone(&self.cancel),
            ctx.clone(),
        );
    }

    /// Takes what the thread has sent; true if anything the browser shows
    /// changed.
    pub(crate) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(message) => {
                    self.apply(message);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    if matches!(self.status, Status::Loading | Status::Scanning { .. }) {
                        self.status = Status::Idle;
                    }
                    changed = true;
                }
            }
        }
        if self.stale {
            self.catalog = Catalog::build(&self.cache, builtin_devices());
            self.stale = false;
        }
        changed
    }

    fn apply(&mut self, message: Message) {
        match message {
            Message::Loaded { cache, note } => {
                self.cache = cache;
                self.stale = true;
                self.note = note.or(self.note.take());
                if self.status == Status::Loading {
                    self.status = Status::Idle;
                }
            }
            Message::Planned { total, reused: _ } => {
                self.status = Status::Scanning { done: 0, total };
            }
            Message::Scanned { done, total, entry } => {
                self.cache.set(entry);
                self.stale = true;
                self.status = Status::Scanning { done, total };
            }
            Message::Finished {
                summary,
                cache,
                saved,
            } => {
                self.cache = cache;
                self.stale = true;
                self.rx = None;
                self.status = match &summary.host_missing {
                    Some(why) => Status::Failed(why.clone()),
                    None => Status::Idle,
                };
                if let Err(why) = saved {
                    self.note = Some(format!("The plugin list could not be saved: {why}"));
                }
                self.finished_at = Some(SystemTime::now());
                self.last = Some(summary);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_plugin_catalog::cache::{Entry, Reason, Result as Scanned};
    use lumen_plugin_catalog::stamp::Stamp;

    fn blocked(path: &str) -> Entry {
        Entry {
            path: path.into(),
            binary: path.into(),
            stamp: Stamp { len: 1, modified_ns: 1 },
            result: Scanned::Blocked {
                reason: Reason::TimedOut { seconds: 1 },
                at: 1,
            },
        }
    }

    #[test]
    fn plugins_are_off_unless_asked_for() {
        let off = Plugins::off();
        assert!(off.is_off() && !off.busy());
        assert_eq!(*off.status(), Status::Off);
        // Only Lumen's own devices.
        assert_eq!(off.catalog().devices().len(), builtin_devices().len());
    }

    #[test]
    fn arguments_choose_off_fixed_or_the_system() {
        let args = |s: &[&str]| s.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert!(matches!(Source::from_args(&args(&["lumen"])), Source::System));
        assert!(matches!(
            Source::from_args(&args(&["lumen", "--plugins", "off"])),
            Source::Off
        ));
        let fixed = Source::from_args(&args(&[
            "lumen",
            "--plugin-roots",
            "/a;/b;",
            "--plugin-host",
            "/h",
            "--plugin-cache",
            "/c.json",
        ]));
        let Source::Fixed(f) = fixed else {
            panic!("fixed");
        };
        assert_eq!(f.roots, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(f.host, PathBuf::from("/h"));
        assert_eq!(f.cache, Some(PathBuf::from("/c.json")));
    }

    #[test]
    fn messages_move_the_status_and_rebuild_the_catalog_when_polled() {
        let mut p = Plugins::off();
        let (tx, rx) = mpsc::channel();
        p.rx = Some(rx);
        p.status = Status::Loading;
        tx.send(Message::Loaded { cache: Cache::new(), note: Some("discarded".into()) }).unwrap();
        tx.send(Message::Planned { total: 2, reused: 1 }).unwrap();
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Scanning { done: 0, total: 2 });
        assert_eq!(p.note(), Some("discarded"));

        tx.send(Message::Scanned { done: 1, total: 2, entry: blocked("/m/Bad.vst3") }).unwrap();
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Scanning { done: 1, total: 2 });
        assert_eq!(p.catalog().blocked().len(), 1);

        let mut cache = Cache::new();
        cache.set(blocked("/m/Bad.vst3"));
        tx.send(Message::Finished { summary: Summary::default(), cache, saved: Err("disk full".into()) }).unwrap();
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Idle);
        assert!(!p.busy());
        assert!(p.note().unwrap().contains("disk full"));
        assert!(p.finished_at().is_some());
    }

    #[test]
    fn a_missing_scan_program_is_a_failed_status_with_the_command_to_build_it() {
        let ctx = egui::Context::default();
        let source = Source::Fixed(Fixed {
            roots: vec![],
            host: PathBuf::from("/no/such/host"),
            cache: None,
            deadline: None,
            faults: None,
        });
        let p = Plugins::start(source, None, &PluginSettings::default(), &ctx);
        let Status::Failed(why) = p.status() else {
            panic!("failed: {:?}", p.status());
        };
        assert!(why.contains("cargo build --release -p lumen-plugin-host"), "{why}");
    }

    #[test]
    fn a_thread_that_ends_without_a_word_does_not_leave_the_status_scanning() {
        let mut p = Plugins::off();
        let (tx, rx) = mpsc::channel::<Message>();
        p.rx = Some(rx);
        p.status = Status::Scanning { done: 0, total: 1 };
        drop(tx);
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Idle);
        assert!(!p.busy());
    }
}
