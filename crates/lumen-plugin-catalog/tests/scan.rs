//! The scanner against real child processes and the test plugin: a cold
//! scan, the cache, a changed file, a removed one, retry, cancel and a
//! missing host. Each module is a copy of the test plugin, given a fault by
//! its name.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use lumen_plugin_catalog::cache::{Cache, Reason, Result};
use lumen_plugin_catalog::catalog::{Catalog, Kind, builtin_devices};
use lumen_plugin_catalog::scan::{Event, Mode, Scanner, Summary};

/// The built artifacts: the test plugin and the host binary.
fn built() -> &'static (PathBuf, PathBuf) {
    static BUILT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    BUILT.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = Command::new(cargo);
        build.args([
            "build",
            "-p",
            "lumen-test-plugin",
            "-p",
            "lumen-plugin-host",
        ]);
        if !cfg!(debug_assertions) {
            build.arg("--release");
        }
        assert!(build.status().expect("run cargo").success(), "build");
        // target/<profile>/deps/<this test> -> target/<profile>
        let exe = std::env::current_exe().expect("test binary");
        let profile = exe.parent().and_then(Path::parent).expect("profile dir");
        let library = if cfg!(windows) {
            "lumen_test_plugin.dll"
        } else if cfg!(target_os = "macos") {
            "liblumen_test_plugin.dylib"
        } else {
            "liblumen_test_plugin.so"
        };
        let host = if cfg!(windows) {
            "lumen-plugin-host.exe"
        } else {
            "lumen-plugin-host"
        };
        let (library, host) = (profile.join(library), profile.join(host));
        assert!(
            library.is_file() && host.is_file(),
            "artifacts were not built"
        );
        (library, host)
    })
}

/// A folder of modules that is removed when dropped.
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lumen-catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Root(dir)
    }

    /// Adds a copy of the test plugin as the bundle `<name>.vst3`; returns its binary.
    fn add(&self, name: &str) -> PathBuf {
        let (arch, extension) = if cfg!(windows) {
            ("x86_64-win", "vst3")
        } else if cfg!(target_os = "linux") {
            ("x86_64-linux", "so")
        } else {
            ("MacOS", "")
        };
        let file = if extension.is_empty() {
            name.to_string()
        } else {
            format!("{name}.{extension}")
        };
        let binary = self.0.join(format!("{name}.vst3/Contents/{arch}/{file}"));
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::copy(&built().0, &binary).unwrap();
        binary
    }

    fn path(&self, name: &str) -> String {
        self.0
            .join(format!("{name}.vst3"))
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A scanner whose children get a fault from the module's name.
fn scanner(deadline: Duration) -> Scanner {
    let mut scanner = Scanner::new(built().1.clone());
    scanner.deadline = deadline;
    scanner.child_env = Some(Box::new(|module| {
        let name = module
            .path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let fault = match name.as_str() {
            "Crash" => "factory_crash",
            "Hang" => "factory_hang",
            "NoFactory" => "factory_null",
            _ => "",
        };
        vec![(
            lumen_test_plugin::faults::ENV.to_string(),
            fault.to_string(),
        )]
    }));
    scanner
}

fn run(scanner: &Scanner, root: &Root, cache: &mut Cache, mode: Mode) -> Summary {
    scanner.scan(
        std::slice::from_ref(&root.0),
        cache,
        mode,
        &AtomicBool::new(false),
        &mut |_| {},
    )
}

const QUICK: Duration = Duration::from_secs(2);

fn four_modules(root: &Root) {
    for name in ["Good", "Crash", "Hang", "NoFactory"] {
        root.add(name);
    }
}

#[test]
fn a_cold_scan_lists_the_good_module_and_blocks_each_bad_one_with_its_reason() {
    let root = Root::new("cold");
    four_modules(&root);
    let mut cache = Cache::new();
    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);

    assert_eq!(summary.discovered, 4);
    assert_eq!(
        (summary.scanned, summary.blocked, summary.reused),
        (4, 3, 0)
    );
    assert!(!summary.cancelled && summary.host_missing.is_none());

    let reason = |name: &str| match &cache.entry(&root.path(name)).unwrap().result {
        Result::Blocked { reason, .. } => reason.clone(),
        other => panic!("{name} should be blocked: {other:?}"),
    };
    assert!(matches!(reason("Crash"), Reason::Crashed { .. }));
    assert_eq!(reason("Hang"), Reason::TimedOut { seconds: 2 });
    assert!(matches!(reason("NoFactory"), Reason::Failed { .. }));

    let catalog = Catalog::build(&cache, builtin_devices());
    let plugins: Vec<_> = catalog
        .devices()
        .iter()
        .filter(|d| d.module.is_some())
        .map(|d| (d.name.as_str(), d.kind))
        .collect();
    assert_eq!(
        plugins,
        [
            ("Deterministic Test Effect", Kind::Effect),
            ("Deterministic Test Instrument", Kind::Instrument)
        ]
    );
    assert_eq!(catalog.blocked().len(), 3);
}

#[test]
fn an_unchanged_second_scan_loads_nothing() {
    let root = Root::new("cached");
    four_modules(&root);
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);

    // Through the file, as the next start of Lumen would see it. The host
    // binary is made unusable: any attempt to scan would show.
    let mut cache = Cache::load(&cache.to_bytes()).cache;
    let mut nothing = Scanner::new(PathBuf::from("/no/such/host"));
    nothing.deadline = QUICK;
    let summary = run(&nothing, &root, &mut cache, Mode::Changed);
    assert_eq!(
        (summary.reused, summary.scanned, summary.removed),
        (4, 0, 0)
    );
    assert!(summary.host_missing.is_none(), "nothing was started");
}

#[test]
fn a_changed_file_is_scanned_again_and_a_blocked_one_that_did_not_change_is_not() {
    let root = Root::new("changed");
    four_modules(&root);
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    let blocked_at = |cache: &Cache| match cache.entry(&root.path("Crash")).unwrap().result {
        Result::Blocked { at, .. } => at,
        _ => panic!("blocked"),
    };
    let before = blocked_at(&cache);

    // Trailing bytes leave a shared library loadable and change its stamp.
    let binary = root.add("Good");
    let mut bytes = std::fs::read(&binary).unwrap();
    bytes.extend_from_slice(&[0; 16]);
    std::fs::write(&binary, bytes).unwrap();

    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!((summary.scanned, summary.reused), (1, 3));
    assert_eq!(
        blocked_at(&cache),
        before,
        "a blocked module is not retried"
    );
}

#[test]
fn a_module_that_changed_is_retried_even_if_it_was_blocked() {
    let root = Root::new("fixed");
    root.add("Crash");
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!(cache.blocked().count(), 1);

    // The "fixed" plugin: new bytes, and the fault gone.
    let binary = root.add("Crash");
    let mut bytes = std::fs::read(&binary).unwrap();
    bytes.extend_from_slice(&[1; 8]);
    std::fs::write(&binary, bytes).unwrap();
    let mut fixed = scanner(QUICK);
    fixed.child_env = None;
    let summary = run(&fixed, &root, &mut cache, Mode::Changed);
    assert_eq!((summary.scanned, summary.blocked), (1, 0));
    assert_eq!(cache.blocked().count(), 0);
}

#[test]
fn a_removed_module_leaves_the_cache() {
    let root = Root::new("removed");
    four_modules(&root);
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    std::fs::remove_dir_all(root.0.join("Crash.vst3")).unwrap();
    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!(
        (summary.removed, summary.scanned, summary.reused),
        (1, 0, 3)
    );
    assert!(cache.entry(&root.path("Crash")).is_none());
}

#[test]
fn retry_forgets_one_module_and_scans_only_it() {
    let root = Root::new("retry");
    four_modules(&root);
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert!(cache.forget(&root.path("NoFactory")));
    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!(
        (summary.scanned, summary.reused, summary.blocked),
        (1, 3, 1)
    );
}

#[test]
fn scan_all_ignores_the_cache() {
    let root = Root::new("all");
    four_modules(&root);
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::All);
    assert_eq!((summary.scanned, summary.reused), (4, 0));
}

#[test]
fn events_report_the_plan_then_each_result_in_order() {
    let root = Root::new("events");
    four_modules(&root);
    let mut cache = Cache::new();
    let mut log = Vec::new();
    scanner(QUICK).scan(
        std::slice::from_ref(&root.0),
        &mut cache,
        Mode::Changed,
        &AtomicBool::new(false),
        &mut |event| match event {
            Event::Planned { total, reused } => log.push(format!("plan {total} {reused}")),
            Event::Scanned { done, total, entry } => {
                log.push(format!(
                    "done {done}/{total} {}",
                    entry.path.rsplit(['/', '\\']).next().unwrap()
                ));
            }
        },
    );
    assert_eq!(log[0], "plan 4 0");
    let counts: Vec<_> = log[1..]
        .iter()
        .map(|l| l.split(' ').nth(1).unwrap().to_string())
        .collect();
    assert_eq!(counts, ["1/4", "2/4", "3/4", "4/4"]);
}

#[test]
fn cancel_stops_a_scan_with_a_hung_module_quickly_and_keeps_what_finished() {
    let root = Root::new("cancel");
    root.add("Good");
    root.add("Hang");
    let mut cache = Cache::new();
    let cancel = AtomicBool::new(false);
    let mut one = scanner(Duration::from_secs(30));
    one.jobs = 2;
    let started = Instant::now();
    let summary = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            one.scan(
                std::slice::from_ref(&root.0),
                &mut cache,
                Mode::Changed,
                &cancel,
                &mut |_| {},
            )
        });
        std::thread::sleep(Duration::from_millis(1500));
        cancel.store(true, Ordering::Relaxed);
        handle.join().unwrap()
    });
    assert!(summary.cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(
        cache.entry(&root.path("Good")).is_some(),
        "what finished stays"
    );
    assert!(
        cache.entry(&root.path("Hang")).is_none(),
        "an unfinished scan says nothing"
    );
}

#[test]
fn a_missing_host_is_reported_once_and_records_nothing() {
    let root = Root::new("nohost");
    four_modules(&root);
    let mut cache = Cache::new();
    let nothing = Scanner::new(PathBuf::from("/no/such/host"));
    let summary = run(&nothing, &root, &mut cache, Mode::Changed);
    assert!(summary.host_missing.is_some());
    assert_eq!(summary.scanned, 0);
    assert!(cache.entries().is_empty());
}

#[test]
fn an_empty_or_missing_root_scans_nothing_without_complaint() {
    let root = Root::new("empty");
    let mut cache = Cache::new();
    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!(summary, Summary::default());
    let summary = scanner(QUICK).scan(
        &[root.0.join("nowhere")],
        &mut cache,
        Mode::Changed,
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert_eq!(summary.missing_roots, 1);
}
