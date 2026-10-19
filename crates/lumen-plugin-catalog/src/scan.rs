//! Scanning every module under some folders, a few at a time, and keeping
//! what is learned in a [`Cache`].
//!
//! The plan is made first: discovery lists the modules; each one whose stamp
//! still matches its cache entry is reused without a process; the rest are
//! scanned by a pool of child processes, each with its own deadline. Results
//! arrive as they finish, so the caller can show them at once. Cancel stops
//! handing out work and kills the children running; what finished stays.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lumen_plugin_host::scan::{ScanOutcome, ScanReport, check_host, run_scan};

use crate::cache::{Cache, ClassRecord, Entry, Reason, Result};
use crate::catalog::class_id;
use crate::discover::{self, Module};
use crate::stamp::Stamp;

/// How long one module may take before it is blocked as unresponsive. Slow
/// plugins load in a few seconds; the longest seen was under 9.
pub const DEADLINE: Duration = Duration::from_secs(30);
/// Scan processes at once, at most.
pub const MAX_JOBS: usize = 4;

/// Which modules a scan loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// New modules and ones whose file changed; the rest come from the cache.
    Changed,
    /// Every module, ignoring the cache, blocked ones included.
    All,
}

/// What a running scan reports.
#[derive(Debug)]
pub enum Event<'a> {
    /// The plan: how many modules will be loaded, and how many were reused.
    Planned { total: usize, reused: usize },
    /// One module finished; `done` of `total` have.
    Scanned {
        done: usize,
        total: usize,
        entry: &'a Entry,
    },
}

/// How a scan went.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub discovered: usize,
    /// Modules whose cached answer still stood.
    pub reused: usize,
    /// Modules loaded this time.
    pub scanned: usize,
    /// Of those, how many came out blocked.
    pub blocked: usize,
    /// Cache entries dropped because their module is gone.
    pub removed: usize,
    pub cancelled: bool,
    /// Set when no scan could start at all: the host binary is missing or
    /// unusable. The message says what was tried.
    pub host_missing: Option<String>,
    /// Bundles skipped for having no binary for this platform.
    pub without_binary: usize,
    /// Folders that do not exist.
    pub missing_roots: usize,
    /// More modules were found than one scan takes (`discover::MAX_MODULES`):
    /// the rest were left out.
    pub truncated: bool,
}

type ChildEnv = Box<dyn Fn(&Module) -> Vec<(String, String)> + Send + Sync>;

pub struct Scanner {
    /// The `lumen-plugin-host` executable that does the scanning.
    pub host: PathBuf,
    pub jobs: usize,
    pub deadline: Duration,
    /// Extra environment for each child, chosen per module. A seam for
    /// tests, which pick a fault for the test plugin that way.
    pub child_env: Option<ChildEnv>,
}

impl Scanner {
    pub fn new(host: PathBuf) -> Self {
        let cores = std::thread::available_parallelism().map_or(2, usize::from);
        Self {
            host,
            jobs: cores.clamp(1, MAX_JOBS),
            deadline: DEADLINE,
            child_env: None,
        }
    }

    /// Scans under `roots`, updating `cache`. `on` is called on this thread.
    pub fn scan(
        &self,
        roots: &[PathBuf],
        cache: &mut Cache,
        mode: Mode,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(Event<'_>),
    ) -> Summary {
        let found = discover::discover(roots);
        let mut summary = Summary {
            discovered: found.modules.len(),
            without_binary: found.without_binary.len(),
            missing_roots: found.missing_roots.len(),
            truncated: found.truncated,
            ..Summary::default()
        };

        // Modules that are gone leave the cache, whether or not this scan
        // finishes: discovery is already complete.
        let present: HashSet<String> = found
            .modules
            .iter()
            .map(|m| m.path.to_string_lossy().into_owned())
            .collect();
        summary.removed = cache.retain(|e| present.contains(&e.path));

        // The plan: a module is reused when its binary's stamp still matches.
        let mut todo: Vec<(Module, Stamp)> = Vec::new();
        for module in found.modules {
            let key = module.path.to_string_lossy();
            let stamp = Stamp::of(&module.binary);
            let reusable = mode == Mode::Changed
                && stamp.is_some_and(|stamp| {
                    cache.entry(&key).is_some_and(|e| {
                        e.stamp == stamp
                            && e.binary == module.binary.to_string_lossy()
                            && !scan_fault(e)
                    })
                });
            if reusable {
                summary.reused += 1;
            } else {
                // A module whose file cannot be read is still scanned: the
                // child will say why, and it gets a zero stamp.
                todo.push((
                    module,
                    stamp.unwrap_or(Stamp {
                        len: 0,
                        modified_ns: 0,
                    }),
                ));
            }
        }
        let total = todo.len();
        on(Event::Planned {
            total,
            reused: summary.reused,
        });
        if total == 0 {
            return summary;
        }

        // A scan program that is out of date answers nothing, which would
        // block every module; find that out before recording anything.
        if let Err(why) = check_host(&self.host) {
            summary.host_missing = Some(why);
            return summary;
        }

        let next = AtomicUsize::new(0);
        let abort = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel::<(usize, ScanOutcome)>();
        let stop = || cancel.load(Ordering::Relaxed) || abort.load(Ordering::Relaxed);
        std::thread::scope(|scope| {
            for _ in 0..self.jobs.clamp(1, total) {
                let tx = tx.clone();
                let (todo, next, stop) = (&todo, &next, &stop);
                scope.spawn(move || {
                    loop {
                        if stop() {
                            return;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((module, _)) = todo.get(index) else {
                            return;
                        };
                        let env = self
                            .child_env
                            .as_ref()
                            .map(|f| f(module))
                            .unwrap_or_default();
                        let env: Vec<(&str, &str)> =
                            env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                        let outcome =
                            run_scan(&self.host, &module.path, self.deadline, &env, cancel);
                        if tx.send((index, outcome)).is_err() {
                            return;
                        }
                    }
                });
            }
            drop(tx);

            let mut done = 0;
            for (index, outcome) in rx {
                let (module, stamp) = &todo[index];
                if let ScanOutcome::Unstarted(why) = &outcome {
                    // Every module would fail the same way.
                    summary.host_missing.get_or_insert(why.clone());
                    abort.store(true, Ordering::Relaxed);
                    continue;
                }
                // A cancelled scan has no answer to keep.
                let Some(result) = result_of(outcome, self.deadline) else {
                    continue;
                };
                let entry = Entry {
                    path: module.path.to_string_lossy().into_owned(),
                    binary: module.binary.to_string_lossy().into_owned(),
                    stamp: *stamp,
                    result,
                };
                done += 1;
                summary.scanned += 1;
                if matches!(entry.result, Result::Blocked { .. }) {
                    summary.blocked += 1;
                }
                on(Event::Scanned {
                    done,
                    total,
                    entry: &entry,
                });
                cache.set(entry);
            }
        });
        summary.cancelled = cancel.load(Ordering::Relaxed);
        summary
    }
}

/// A block that says nothing about the module: the scan program ended
/// cleanly without an answer, which a module cannot do on its own (it would
/// have crashed, hung or failed with a message). It means the scan program
/// was faulty, so such an entry is tried again rather than trusted until the
/// file changes.
fn scan_fault(entry: &Entry) -> bool {
    matches!(
        entry.result,
        Result::Blocked {
            reason: Reason::Malformed { .. },
            ..
        }
    )
}

/// What the cache records for a finished scan; `None` for one that was
/// cancelled or never started, which says nothing about the module.
fn result_of(outcome: ScanOutcome, deadline: Duration) -> Option<Result> {
    let blocked = |reason| Result::Blocked { reason, at: now() };
    Some(match outcome {
        ScanOutcome::Scanned(report) => scanned(report),
        ScanOutcome::Failed(message) => blocked(Reason::Failed { message }),
        ScanOutcome::Crashed { how, detail } => blocked(Reason::Crashed { how, detail }),
        ScanOutcome::TimedOut => blocked(Reason::TimedOut {
            seconds: deadline.as_secs(),
        }),
        ScanOutcome::Malformed(message) => blocked(Reason::Malformed { message }),
        ScanOutcome::Cancelled | ScanOutcome::Unstarted(_) => return None,
    })
}

fn scanned(report: ScanReport) -> Result {
    Result::Scanned {
        vendor: report.vendor,
        url: report.url,
        load_ms: report.load_ms,
        classes: report
            .classes
            .into_iter()
            .map(|c| ClassRecord {
                id: class_id(&c.id),
                name: c.name,
                category: c.category,
                vendor: c.vendor,
                subcategories: c.subcategories,
                version: c.version,
                sdk_version: c.sdk_version,
            })
            .collect(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
