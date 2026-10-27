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
use lumen_plugin_catalog::scan::Mode;

pub(crate) use settings::{EditorPlace, PluginSettings};
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

impl Source {
    /// The faults the test plugin's hosts are given, by module name (tests).
    pub(crate) fn host_faults(&self) -> Option<Faults> {
        match self {
            Source::Fixed(fixed) => fixed.faults,
            _ => None,
        }
    }

    /// The program that runs a plugin: the one that scans, beside Lumen
    /// unless the caller chose another. None when plugins are off.
    pub(crate) fn host_program(&self) -> Option<PathBuf> {
        match self {
            Source::Off => None,
            Source::System => Some(default_host()),
            Source::Fixed(fixed) => Some(fixed.host.clone()),
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
    /// When the catalog was last built, and how many times: during a scan
    /// it is rebuilt at most every [`REBUILD_EVERY`].
    built_at: Option<std::time::Instant>,
    builds: u32,
    /// A note for the person: the cache was thrown away, or could not be
    /// written. Cleared by [`Plugins::dismiss_note`].
    note: Option<String>,
    finished_at: Option<SystemTime>,
    /// Safe mode: the cache is read, and nothing is scanned (a scan loads every
    /// plugin binary).
    safe: bool,
}

/// The most often the browser's list is rebuilt while a scan runs.
const REBUILD_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

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
            built_at: None,
            builds: 0,
            note: None,
            finished_at: None,
            safe: false,
        }
    }

    /// Starts reading the cache and, when `settings` ask, scanning.
    pub(crate) fn start(
        source: Source,
        settings_path: Option<&std::path::Path>,
        settings: &PluginSettings,
        safe: bool,
        ctx: &egui::Context,
    ) -> Self {
        let mut plugins = Self::off();
        plugins.safe = safe;
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
        plugins.run(
            None,
            (settings.scan_at_startup && !safe).then_some(Mode::Changed),
            settings,
            ctx,
        );
        plugins
    }

    /// Ends safe mode: the scan the start left out runs now, if the settings
    /// ask for one at start.
    pub(crate) fn leave_safe(&mut self, settings: &PluginSettings, ctx: &egui::Context) {
        if !self.safe {
            return;
        }
        self.safe = false;
        if settings.scan_at_startup && !self.is_off() && !self.busy() {
            self.run(Some(self.cache.clone()), Some(Mode::Changed), settings, ctx);
        }
    }

    pub(crate) fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Replaces what is known with `cache`, as if a scan had just found it
    /// (the look audit's browser, `App::pretend_plugins`).
    pub(crate) fn pretend(&mut self, cache: Cache) {
        self.catalog = Catalog::build(&cache, builtin_devices());
        self.cache = cache;
        self.status = Status::Idle;
        self.finished_at = Some(SystemTime::now());
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
        if self.is_off() || self.busy() || self.safe {
            return;
        }
        self.run(Some(self.cache.clone()), Some(mode), settings, ctx);
    }

    /// Forgets what is known about one module and loads it again: Retry on
    /// a blocked plugin, or "rescan this plugin".
    pub(crate) fn retry(&mut self, module: &str, settings: &PluginSettings, ctx: &egui::Context) {
        if self.is_off() || self.busy() || self.safe {
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
        // A scan needs its program; whether it is there is found out on the
        // scan thread (a stat can be slow), which says what is missing.
        let host = scan.map(|_| host);
        self.cancel.store(false, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        if host.is_some() {
            self.status = Status::Scanning { done: 0, total: 0 };
        }
        let started = worker::spawn(
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
        if let Err(e) = started {
            self.rx = None;
            self.status = Status::Failed(format!("The plugin scan could not start: {e}"));
        }
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
        if self.stale && self.rebuild_wait().is_none() {
            self.catalog = Catalog::build(&self.cache, builtin_devices());
            self.stale = false;
            self.built_at = Some(std::time::Instant::now());
            self.builds += 1;
        }
        changed
    }

    /// How long until the list the browser shows is brought up to date with
    /// what the scan has found, when it is not yet: building it is work
    /// that grows with the library, so during a scan it is done a few times
    /// a second and not once for every module.
    pub(crate) fn rebuild_wait(&self) -> Option<std::time::Duration> {
        if !self.stale {
            return None;
        }
        if !matches!(self.status, Status::Scanning { .. }) {
            return None;
        }
        let since = self.built_at?.elapsed();
        REBUILD_EVERY.checked_sub(since).filter(|d| !d.is_zero())
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
            Message::Planned { total } => {
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
                if summary.truncated {
                    self.note = Some(format!(
                        "More than {} plugins were found. Only the first were listed.",
                        lumen_plugin_catalog::discover::MAX_MODULES
                    ));
                }
                if let Err(why) = saved {
                    self.note = Some(format!("The plugin list could not be saved: {why}"));
                }
                self.finished_at = Some(SystemTime::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_plugin_catalog::cache::{Entry, Reason, Result as Scanned};
    use lumen_plugin_catalog::scan::Summary;
    use lumen_plugin_catalog::stamp::Stamp;

    fn blocked(path: &str) -> Entry {
        Entry {
            path: path.into(),
            binary: path.into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Scanned::Blocked {
                reason: Reason::TimedOut { seconds: 1 },
                at: 1,
            },
        }
    }

    #[test]
    fn the_list_is_rebuilt_a_few_times_a_second_during_a_scan_not_once_a_module() {
        let mut p = Plugins::off();
        let (tx, rx) = mpsc::channel();
        p.rx = Some(rx);
        p.status = Status::Scanning {
            done: 0,
            total: 300,
        };
        let before = p.builds;
        for i in 0..300 {
            tx.send(Message::Scanned {
                done: i + 1,
                total: 300,
                entry: blocked(&format!("/m/Bad{i}.vst3")),
            })
            .unwrap();
            // A frame after every module.
            p.poll();
        }
        let built = p.builds - before;
        assert!(built <= 3, "{built} rebuilds for 300 modules in an instant");
        assert!(
            p.rebuild_wait().is_some(),
            "the last modules are not shown yet"
        );
        // The scan ends: everything is shown at once.
        tx.send(Message::Finished {
            summary: Summary::default(),
            cache: p.cache.clone(),
            saved: Ok(()),
        })
        .unwrap();
        p.poll();
        assert_eq!(p.catalog().blocked().len(), 300);
        assert!(p.rebuild_wait().is_none());
    }

    #[test]
    fn a_scan_that_left_modules_out_says_so() {
        let mut p = Plugins::off();
        let (tx, rx) = mpsc::channel();
        p.rx = Some(rx);
        p.status = Status::Scanning { done: 0, total: 1 };
        tx.send(Message::Finished {
            summary: Summary {
                truncated: true,
                ..Summary::default()
            },
            cache: Cache::new(),
            saved: Ok(()),
        })
        .unwrap();
        p.poll();
        assert!(p.note().is_some_and(|n| n.contains("Only the first")));
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
        assert!(matches!(
            Source::from_args(&args(&["lumen"])),
            Source::System
        ));
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
        tx.send(Message::Loaded {
            cache: Cache::new(),
            note: Some("discarded".into()),
        })
        .unwrap();
        tx.send(Message::Planned { total: 2 }).unwrap();
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Scanning { done: 0, total: 2 });
        assert_eq!(p.note(), Some("discarded"));

        tx.send(Message::Scanned {
            done: 1,
            total: 2,
            entry: blocked("/m/Bad.vst3"),
        })
        .unwrap();
        assert!(p.poll());
        assert_eq!(*p.status(), Status::Scanning { done: 1, total: 2 });

        let mut cache = Cache::new();
        cache.set(blocked("/m/Bad.vst3"));
        tx.send(Message::Finished {
            summary: Summary::default(),
            cache,
            saved: Err("disk full".into()),
        })
        .unwrap();
        assert!(p.poll());
        assert_eq!(
            p.catalog().blocked().len(),
            1,
            "built once the scan is over"
        );
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
        let mut p = Plugins::start(source, None, &PluginSettings::default(), false, &ctx);
        // The scan thread looks for the program, not the interface's thread.
        assert!(
            !matches!(p.status(), Status::Failed(_)),
            "found out on the frame that started it"
        );
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while matches!(p.status(), Status::Scanning { .. } | Status::Loading)
            && std::time::Instant::now() < end
        {
            p.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let Status::Failed(why) = p.status() else {
            panic!("failed: {:?}", p.status());
        };
        assert!(
            why.contains("cargo build --release -p lumen-plugin-host"),
            "{why}"
        );
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
