//! The scanner against real child processes and the test plugin: a cold
//! scan, the cache, a changed file, a removed one, retry, cancel and a
//! missing host. Each module is a copy of the test plugin, given a fault by
//! its name.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
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
        let binary = self.place(name);
        std::fs::copy(&built().0, &binary).unwrap();
        self.warm(name);
        binary
    }

    /// Adds the bundle `<name>.vst3`, or replaces its binary, with a copy of
    /// the test plugin that has `extra` bytes after the library: it still
    /// loads, and its stamp differs from an unchanged copy's.
    fn add_changed(&self, name: &str, extra: &[u8]) -> PathBuf {
        let binary = self.place(name);
        let mut bytes = std::fs::read(&built().0).unwrap();
        bytes.extend_from_slice(extra);
        std::fs::write(&binary, bytes).unwrap();
        self.warm(name);
        binary
    }

    /// Loads the new plugin once, here and alone, on macOS. macOS checks
    /// every new file the first time it is loaded, one file after another at
    /// about 0.1 s each. Tests that each add several plugins and run side by
    /// side would queue behind one another inside the scan, and a scan's
    /// deadline would run out on the queue. A file that has been checked
    /// loads at once.
    fn warm(&self, name: &str) {
        if !cfg!(target_os = "macos") {
            return;
        }
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let status = Command::new(&built().1)
            .arg("scan")
            .arg(self.0.join(format!("{name}.vst3")))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run the scan program");
        assert!(status.success(), "the plugin does not load");
    }

    /// Where the bundle's binary goes; its folder is made.
    fn place(&self, name: &str) -> PathBuf {
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
        #[cfg(target_os = "macos")]
        {
            let info = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{name}</string><key>CFBundleIdentifier</key><string>org.lumen.test.{name}</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>"
            );
            std::fs::write(
                binary
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("Info.plist"),
                info,
            )
            .unwrap();
        }
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
    assert!(
        matches!(reason("Crash"), Reason::Crashed { .. }),
        "{:?}",
        reason("Crash")
    );
    assert_eq!(reason("Hang"), Reason::TimedOut { seconds: 2 });
    assert!(
        matches!(reason("NoFactory"), Reason::Failed { .. }),
        "{:?}",
        reason("NoFactory")
    );

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
    root.add_changed("Good", &[0; 16]);

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
    root.add_changed("Crash", &[1; 8]);
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
    // Cancel when the good module is in, not after a wait that a busy machine
    // could cut short: the hung one is then what is left.
    let summary = one.scan(
        std::slice::from_ref(&root.0),
        &mut cache,
        Mode::Changed,
        &cancel,
        &mut |event| {
            if matches!(event, Event::Scanned { .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    );
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
fn a_block_that_came_from_the_scan_program_is_tried_again_by_a_plain_rescan() {
    // An earlier, faulty scan program left "ended without a reply" for a
    // module that is fine; its file never changed. A plain rescan must not
    // trust that block, while a real one (here, a crash) stays.
    let root = Root::new("scanfault");
    root.add("Good");
    root.add("Crash");
    let mut cache = Cache::new();
    run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    let mut planted = cache.entry(&root.path("Good")).unwrap().clone();
    planted.result = Result::Blocked {
        reason: Reason::Malformed {
            message: "the scan process ended without a reply".into(),
        },
        at: 1,
    };
    cache.set(planted);

    let summary = run(&scanner(QUICK), &root, &mut cache, Mode::Changed);
    assert_eq!((summary.scanned, summary.reused), (1, 1));
    assert!(matches!(
        cache.entry(&root.path("Good")).unwrap().result,
        Result::Scanned { .. }
    ));
}

#[test]
fn a_host_that_gives_no_reply_blocks_nothing() {
    // A scan program from before scanning existed ends cleanly and says
    // nothing. Every module would come out "blocked"; none may be recorded.
    let root = Root::new("stalehost");
    four_modules(&root);
    let mut cache = Cache::new();
    let program = if cfg!(windows) {
        std::env::var_os("ComSpec").map_or_else(|| PathBuf::from("cmd.exe"), PathBuf::from)
    } else {
        // macOS keeps `true` in /usr/bin, not /bin.
        ["/usr/bin/true", "/bin/true"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .expect("a `true` program in /usr/bin or /bin")
    };
    let stale = Scanner::new(program);
    let summary = run(&stale, &root, &mut cache, Mode::Changed);
    let why = summary.host_missing.expect("the program is reported");
    assert!(why.contains("out of date"), "{why}");
    assert_eq!((summary.scanned, summary.blocked), (0, 0));
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
