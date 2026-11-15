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

pub const DEADLINE: Duration = Duration::from_secs(30);
pub const MAX_JOBS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Changed,
    All,
}

#[derive(Debug)]
pub enum Event<'a> {
    Planned { total: usize, reused: usize },
    Scanned {
        done: usize,
        total: usize,
        entry: &'a Entry,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub discovered: usize,
    pub reused: usize,
    pub scanned: usize,
    pub blocked: usize,
    pub removed: usize,
    pub cancelled: bool,
    pub host_missing: Option<String>,
    pub without_binary: usize,
    pub missing_roots: usize,
    pub truncated: bool,
}

type ChildEnv = Box<dyn Fn(&Module) -> Vec<(String, String)> + Send + Sync>;

pub struct Scanner {
    pub host: PathBuf,
    pub jobs: usize,
    pub deadline: Duration,
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

        let present: HashSet<String> = found
            .modules
            .iter()
            .map(|m| m.path.to_string_lossy().into_owned())
            .collect();
        summary.removed = cache.retain(|e| present.contains(&e.path));

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

        if let Err(why) = check_host(&self.host) {
            summary.host_missing = Some(why);
            return summary;
        }

        let next = AtomicUsize::new(0);
        let abort = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel::<(usize, ScanOutcome)>();
        let stop = || cancel.load(Ordering::Relaxed) || abort.load(Ordering::Relaxed);
        let mut workers = 0;
        let mut unstarted = None;
        std::thread::scope(|scope| {
            for worker in 0..self.jobs.clamp(1, total) {
                let tx = tx.clone();
                let (todo, next, stop) = (&todo, &next, &stop);
                let started = std::thread::Builder::new()
                    .name(format!("lumen-scan-{worker}"))
                    .spawn_scoped(scope, move || {
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
                match started {
                    Ok(_) => workers += 1,
                    Err(error) => {
                        unstarted.get_or_insert_with(|| error.to_string());
                    }
                }
            }
            drop(tx);
            if workers == 0 {
                return;
            }

            let mut done = 0;
            for (index, outcome) in rx {
                let (module, stamp) = &todo[index];
                if let ScanOutcome::Unstarted(why) = &outcome {
                    summary.host_missing.get_or_insert(why.clone());
                    abort.store(true, Ordering::Relaxed);
                    continue;
                }
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
        if workers == 0 {
            summary.host_missing = Some(no_workers(unstarted.as_deref()));
        }
        summary.cancelled = cancel.load(Ordering::Relaxed);
        summary
    }
}

fn no_workers(why: Option<&str>) -> String {
    match why {
        Some(why) => format!("could not start a thread to scan with: {why}"),
        None => "could not start a thread to scan with".to_string(),
    }
}

fn scan_fault(entry: &Entry) -> bool {
    matches!(
        entry.result,
        Result::Blocked {
            reason: Reason::Malformed { .. },
            ..
        }
    )
}

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

#[cfg(test)]
mod tests {
    use super::no_workers;

    #[test]
    fn a_pool_that_could_not_start_says_why() {
        let why = no_workers(Some("the system is out of threads"));
        assert!(why.contains("thread"), "{why}");
        assert!(why.contains("out of threads"), "{why}");
        assert!(no_workers(None).contains("thread"));
    }
}
