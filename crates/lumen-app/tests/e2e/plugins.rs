//! The browser's Devices tab (M6 slice 1): plugins found by a scan in child
//! processes, listed by vendor, narrowed by type, category and words, with
//! favorites, a blocked section and a way to retry. Each module is a copy of
//! the test plugin, given a fault by its name; the scan program is the real
//! `lumen-plugin-host`. Driven by accessible names and keys.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::testing::TempDir;

use super::Lumen;

const INSTRUMENT: &str = "Deterministic Test Instrument";
const EFFECT: &str = "Deterministic Test Effect";

/// The test plugin's library and the scan program, built once.
pub(super) fn built() -> &'static (PathBuf, PathBuf) {
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

/// Set once the module named "Mended" is fixed: its fault goes away.
static MENDED: AtomicBool = AtomicBool::new(false);

/// The fault each test module is given, by its name.
fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        "Crash" => Some("factory_crash"),
        "Hang" => Some("factory_hang"),
        "Mended" => (!MENDED.load(Ordering::Relaxed)).then_some("factory_crash"),
        _ => None,
    }
}

/// Puts a copy of the test plugin in `root` as the bundle `<name>.vst3`.
fn add_module(root: &Path, name: &str) {
    let (arch, extension) = if cfg!(windows) {
        ("x86_64-win", "vst3")
    } else if cfg!(target_os = "linux") {
        ("x86_64-linux", "so")
    } else {
        ("MacOS", "")
    };
    let file = if extension.is_empty() {
        name.to_owned()
    } else {
        format!("{name}.{extension}")
    };
    let binary = root.join(format!("{name}.vst3/Contents/{arch}/{file}"));
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    #[cfg(target_os = "macos")]
    std::fs::write(binary.parent().unwrap().join("A-decoy"), b"not a module").unwrap();
    std::fs::copy(&built().0, binary).unwrap();
    #[cfg(target_os = "macos")]
    {
        let contents = root.join(format!("{name}.vst3/Contents"));
        let info = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{name}</string><key>CFBundleIdentifier</key><string>org.lumen.test.{name}</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>"
        );
        std::fs::write(contents.join("Info.plist"), info).unwrap();
    }
    warm(&root.join(format!("{name}.vst3")));
}

/// Loads a new plugin once, here and alone, on macOS. macOS checks every new
/// file the first time it is loaded, one file after another at about 0.1 s
/// each. Tests that each add several plugins and run side by side would queue
/// behind one another inside the scan, and its two-second deadline would run
/// out on the queue. A file that has been checked loads at once.
fn warm(bundle: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let status = Command::new(&built().1)
        .arg("scan")
        .arg(bundle)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run the scan program");
    assert!(status.success(), "the plugin does not load");
}

/// A folder of plugins in `dir`, holding the modules named.
pub(super) fn plugin_root(dir: &Path, modules: &[&str]) -> PathBuf {
    let root = dir.join("plugins");
    std::fs::create_dir_all(&root).unwrap();
    for name in modules {
        add_module(&root, name);
    }
    root
}

/// Lumen with `modules` installed, scanning at start.
fn start(name: &str, modules: &[&str]) -> Lumen {
    let dir = TempDir::new(&format!("e2e-plugins-{name}"));
    let root = plugin_root(dir.path(), modules);
    start_in(dir, root, |l| l)
}

fn start_in(
    dir: TempDir,
    root: PathBuf,
    adjust: impl FnOnce(lumen_app::Launch) -> lumen_app::Launch,
) -> Lumen {
    let cache = dir.path().join("plugin-cache.json");
    let mut lumen = Lumen::start_with(dir, |launch| {
        adjust(
            launch
                .with_plugins(vec![root], &built().1, cache)
                .with_plugin_deadline(Duration::from_secs(2))
                .with_plugin_faults(faults),
        )
    });
    lumen.settled();
    lumen
}

impl Lumen {
    /// Waits for the scan, then draws once more.
    fn settled(&mut self) {
        self.wait_for("the plugin scan", |app| !app.plugins_busy());
        self.frames(3);
    }

    /// Whether a control or row whose name starts with `label` is shown.
    fn shows_row(&self, label: &str) -> bool {
        self.harness.query_by_label_contains(label).is_some()
    }

    /// Runs frames until a row whose name starts with `label` is shown.
    #[track_caller]
    fn wait_for_row(&mut self, label: &str) {
        let started = std::time::Instant::now();
        while !self.shows_row(label) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "timed out waiting for the row {label}"
            );
            std::thread::sleep(Duration::from_millis(5));
            self.frames(1);
        }
    }

    #[track_caller]
    fn click_row(&mut self, label: &str) {
        self.harness.get_by_label_contains(label).click();
        self.frames(3);
    }

    fn type_in_search(&mut self, text: &str) {
        self.key(Modifiers::COMMAND, Key::F);
        self.harness.event(Event::Text(text.into()));
        self.frames(3);
    }
}

#[test]
fn a_startup_scan_lists_the_installed_plugins_by_vendor() {
    let lumen = start("listed", &["Good"]);
    assert_eq!(lumen.app().plugin_names(), [EFFECT, INSTRUMENT]);
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, Test Vendor, instrument")));
    assert!(lumen.shows_row(&format!("{EFFECT}, Test Vendor, effect")));
    assert!(
        lumen.shows_row("Test Vendor, 2 devices, open"),
        "the vendor's header"
    );
    assert!(
        lumen.shows_row("Lumen, 1 device, open"),
        "Lumen's own devices come first"
    );
    assert!(lumen.shows_row("Synth, Lumen, instrument"));
    assert!(lumen.shows("Search devices"));
    assert!(lumen.shows("Rescan"), "the scan is over");
}

#[test]
fn without_plugins_the_browser_lists_only_the_synth_and_says_plugins_are_off() {
    let lumen = Lumen::start("plugins-off");
    assert!(lumen.shows_row("Synth, Lumen, instrument"));
    assert!(lumen.app().plugin_names().is_empty());
    assert!(lumen.shows("Plugins are off for this run"));
}

#[test]
fn a_crashing_and_a_hanging_plugin_are_blocked_with_their_reasons_and_the_rest_still_load() {
    let mut lumen = start("blocked", &["Good", "Crash", "Hang"]);
    assert_eq!(lumen.app().plugin_names(), [EFFECT, INSTRUMENT]);
    assert_eq!(lumen.app().blocked_plugins(), ["Crash", "Hang"]);
    assert!(lumen.shows_row("Crash, blocked"));
    assert!(lumen.shows_row("Hang, blocked"));
    assert!(
        lumen.shows_row("Blocked, 2 devices, open"),
        "the section header"
    );

    // Selecting one says why, and what Lumen will do.
    lumen.click_row("Crash, blocked");
    assert!(lumen.shows_row("It crashed while loading"));
    assert!(lumen.shows_row("will not try again until its file changes"));
    assert!(lumen.shows("Retry"));
    assert!(lumen.shows("Show in folder"));
    lumen.click_row("Hang, blocked");
    assert!(lumen.shows_row("It did not respond within 2 seconds"));
    assert!(
        !lumen.shows_row("It crashed while loading"),
        "one card at a time"
    );
}

#[test]
fn retry_loads_a_blocked_module_again_and_it_joins_the_list_once_it_works() {
    MENDED.store(false, Ordering::Relaxed);
    let mut lumen = start("retry", &["Mended"]);
    assert_eq!(lumen.app().blocked_plugins(), ["Mended"]);
    assert!(lumen.app().plugin_names().is_empty());

    // Retried while it still fails: still blocked, and only it was loaded.
    lumen.click_row("Mended, blocked");
    lumen.click("Retry");
    lumen.settled();
    assert_eq!(lumen.app().blocked_plugins(), ["Mended"]);

    // Mended, and Retry: its devices are listed and nothing is blocked.
    MENDED.store(true, Ordering::Relaxed);
    lumen.click("Retry");
    lumen.settled();
    assert!(lumen.app().blocked_plugins().is_empty());
    assert_eq!(lumen.app().plugin_names(), [EFFECT, INSTRUMENT]);
    assert!(!lumen.shows_row("Mended, blocked"));
    MENDED.store(false, Ordering::Relaxed);
}

#[test]
fn the_plugins_page_lists_a_blocked_module_and_retries_it() {
    MENDED.store(false, Ordering::Relaxed);
    let mut lumen = start("preferences-blocked", &["Mended"]);
    assert_eq!(lumen.app().blocked_plugins(), ["Mended"]);
    lumen.open_preferences("Plugins");
    assert!(lumen.shows("Blocked plugins"));
    assert!(lumen.shows("Retry Mended"));
    MENDED.store(true, Ordering::Relaxed);
    lumen.click("Retry Mended");
    lumen.settled();
    assert!(lumen.app().blocked_plugins().is_empty());
    assert!(!lumen.shows("Retry Mended"));
    assert!(lumen.shows("None. A plugin that fails while it is scanned is listed here."));
    MENDED.store(false, Ordering::Relaxed);
}

#[test]
fn a_module_added_since_is_found_by_rescan() {
    let mut lumen = start("rescan", &["Good"]);
    assert!(lumen.app().blocked_plugins().is_empty());
    assert!(!lumen.shows_row("Blocked,"));
    let root = lumen.dir().join("plugins");
    add_module(&root, "Crash");
    lumen.click("Rescan");
    lumen.settled();
    assert_eq!(lumen.app().blocked_plugins(), ["Crash"]);
    assert_eq!(
        lumen.app().plugin_names(),
        [EFFECT, INSTRUMENT],
        "the others were not disturbed"
    );
}

#[test]
fn the_palette_rescan_runs_and_cannot_run_while_scanning_or_when_plugins_are_off() {
    let mut lumen = Lumen::start("rescan-off");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("rescan plugins".into()));
    lumen.frames(2);
    let ctx = lumen.harness.ctx.clone();
    let rows = lumen.app().palette_shows(&ctx);
    assert_eq!(rows[0], ("Rescan plugins".to_owned(), false), "{rows:?}");
}

#[test]
fn an_unchanged_restart_lists_plugins_from_the_saved_list_without_loading_any() {
    let dir = TempDir::new("e2e-plugins-cached");
    let root = plugin_root(dir.path(), &["Good", "Crash"]);
    let cache = dir.path().join("plugin-cache.json");
    let first = start_in(dir, root.clone(), |l| l);
    assert!(cache.is_file(), "the scan saved its answers");
    let dir = first.crash();

    // Scan at startup is off and the scan program is gone: only the saved
    // list can have made these.
    let mut second = Lumen::start_with(dir, |launch| {
        launch
            .with_plugins(vec![root], Path::new("/no/such/host"), cache)
            .without_plugin_scan_at_startup()
    });
    second.settled();
    assert_eq!(second.app().plugin_names(), [EFFECT, INSTRUMENT]);
    assert_eq!(second.app().blocked_plugins(), ["Crash"]);
    assert!(second.shows_row(&format!("{INSTRUMENT}, Test Vendor, instrument")));
}

#[test]
fn a_missing_scan_program_says_what_to_build_and_the_saved_list_still_shows() {
    let dir = TempDir::new("e2e-plugins-nohost");
    let root = plugin_root(dir.path(), &["Good"]);
    let cache = dir.path().join("plugin-cache.json");
    let first = start_in(dir, root.clone(), |l| l);
    let dir = first.crash();

    let mut lumen = Lumen::start_with(dir, |launch| {
        launch.with_plugins(vec![root], Path::new("/no/such/host"), cache)
    });
    lumen.settled();
    assert!(
        lumen.shows_row("The plugin scanner was not found"),
        "the card names the problem"
    );
    assert!(lumen.shows_row("cargo build --release -p lumen-plugin-host"));
    assert_eq!(
        lumen.app().plugin_names(),
        [EFFECT, INSTRUMENT],
        "from the saved list"
    );
}

#[test]
fn words_the_type_switch_and_chips_narrow_the_list_and_clear_filters_brings_it_back() {
    let mut lumen = start("narrow", &["Good"]);

    // Words: only what matches.
    lumen.type_in_search("effect");
    assert!(lumen.shows_row(&format!("{EFFECT}, Test Vendor, effect")));
    assert!(!lumen.shows_row(&format!("{INSTRUMENT}, ")));
    assert!(!lumen.shows_row("Synth, Lumen"));
    lumen.click("Clear search");
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, ")));

    // The type switch.
    lumen.click("Instruments only");
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, ")));
    assert!(lumen.shows_row("Synth, Lumen"));
    assert!(!lumen.shows_row(&format!("{EFFECT}, ")));
    lumen.click("Effects only");
    assert!(lumen.shows_row(&format!("{EFFECT}, ")));
    assert!(!lumen.shows_row(&format!("{INSTRUMENT}, ")));
    assert!(!lumen.shows_row("Synth, Lumen"));

    // A filter with nothing in it says so and offers the way back.
    lumen.click("Favorites filter");
    assert!(lumen.shows_row("No devices match"));
    lumen.click("Clear filters");
    assert!(
        lumen.shows_row(&format!("{INSTRUMENT}, ")),
        "everything is back"
    );
    assert!(lumen.shows_row(&format!("{EFFECT}, ")));
}

#[test]
fn a_category_chip_narrows_to_that_category_and_turns_off_when_clicked_again() {
    let mut lumen = start("chip", &["Good"]);
    // The test instrument's category is its factory sub-category.
    let chip = lumen
        .harness
        .query_all_by_label_contains(" filter")
        .map(|n| n.accesskit_node().label().unwrap_or_default().to_owned())
        .find(|l| l != "Favorites filter" && l != "Recent filter")
        .expect("a category chip");
    lumen.click(&chip);
    assert!(lumen.is_on(&chip));
    lumen.click(&chip);
    assert!(!lumen.is_on(&chip));
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, ")));
    assert!(lumen.shows_row(&format!("{EFFECT}, ")));
}

#[test]
fn a_star_marks_a_favorite_and_the_favorites_chip_shows_only_those() {
    let mut lumen = start("favorite", &["Good"]);
    let id = format!("Favorite {INSTRUMENT}");
    assert!(!lumen.is_on(&id));
    lumen.click(&id);
    assert!(lumen.is_on(&id));
    assert_eq!(lumen.app().favorite_devices().len(), 1);

    lumen.click("Favorites filter");
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, ")));
    assert!(!lumen.shows_row(&format!("{EFFECT}, ")));
    lumen.click("Favorites filter");
    lumen.click(&id);
    assert!(!lumen.is_on(&id));
    assert!(lumen.app().favorite_devices().is_empty());
}

#[test]
fn favorites_and_recents_survive_a_restart() {
    let dir = TempDir::new("e2e-plugins-restart");
    let root = plugin_root(dir.path(), &["Good"]);
    let file = dir.path().join("settings.json");
    let mut first = start_in(dir, root.clone(), |l| l.with_settings_file(&file));
    first.click(&format!("Favorite {EFFECT}"));
    // Double-clicking a device chooses it: it becomes a recent.
    let at = first
        .harness
        .get_by_label_contains(&format!("{INSTRUMENT}, "))
        .rect()
        .center();
    first.double_click(at);
    assert_eq!(first.app().recent_devices().len(), 1, "chosen");
    // The file already names the favorite (and the key "recents") from the
    // click before: wait until the recents list itself holds the device.
    first.wait_for("the settings to be saved", |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| {
            t.split("\"recents\"")
                .nth(1)
                .and_then(|rest| rest.split(']').next())
                .is_some_and(|list| list.contains("vst3:"))
        })
    });
    let dir = first.crash();

    let mut second = Lumen::start_with(dir, |launch| {
        launch
            .with_plugins(
                vec![root],
                &built().1,
                file.parent().unwrap().join("plugin-cache.json"),
            )
            .with_settings_file(&file)
            .without_plugin_scan_at_startup()
    });
    second.settled();
    assert!(second.is_on(&format!("Favorite {EFFECT}")));
    assert!(!second.is_on(&format!("Favorite {INSTRUMENT}")));
    second.click("Recent filter");
    // The list fills when the cache has been read, which a busy machine can
    // put after the first frames.
    second.wait_for_row(&format!("{INSTRUMENT}, "));
    assert!(!second.shows_row(&format!("{EFFECT}, ")));
}

#[test]
fn the_keyboard_alone_walks_the_list_stars_and_chooses() {
    let mut lumen = start("keys", &["Good"]);
    // F6 goes to the arrangement, the detail panel, then the browser.
    for _ in 0..3 {
        lumen.key(Modifiers::NONE, Key::F6);
    }
    let ctx = lumen.harness.ctx.clone();
    assert_eq!(lumen.app().focused_pane(&ctx).as_deref(), Some("Browser"));

    // Lumen's header, the synth, Test Vendor's header, then the effect.
    for _ in 0..4 {
        lumen.key(Modifiers::NONE, Key::ArrowDown);
    }
    let effect = lumen.app().selected_device().expect("a device is selected");
    assert!(effect.starts_with("vst3:"));
    lumen.key(Modifiers::NONE, Key::F);
    assert_eq!(
        lumen.app().favorite_devices(),
        std::slice::from_ref(&effect)
    );
    lumen.key(Modifiers::NONE, Key::Enter);
    assert_eq!(lumen.app().recent_devices(), [effect]);

    // Up and Home go back; Left on a device goes to its vendor's header.
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(lumen.app().selected_device(), None);
    lumen.key(Modifiers::NONE, Key::ArrowLeft);
    assert!(
        !lumen.shows_row(&format!("{EFFECT}, ")),
        "the vendor folded"
    );
    lumen.key(Modifiers::NONE, Key::ArrowRight);
    assert!(lumen.shows_row(&format!("{EFFECT}, ")), "and opened again");
}

#[test]
fn ctrl_f_opens_the_search_and_typing_there_runs_no_commands() {
    let mut lumen = start("search-keys", &["Good"]);
    lumen.type_in_search("f");
    // "F" is the favorite key, and the field has the keyboard.
    assert!(lumen.app().favorite_devices().is_empty());
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.shows_row(&format!("{INSTRUMENT}, ")) || lumen.shows_row(&format!("{EFFECT}, ")));
}

#[test]
fn cancel_stops_a_scan_that_is_stuck_in_a_hung_plugin_and_keeps_what_finished() {
    let dir = TempDir::new("e2e-plugins-cancel");
    let root = plugin_root(dir.path(), &["Good", "Hang"]);
    let cache = dir.path().join("plugin-cache.json");
    // A long deadline: only Cancel can end this scan soon.
    let mut lumen = Lumen::start_with(dir, |launch| {
        launch
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_deadline(Duration::from_secs(60))
            .with_plugin_faults(faults)
    });
    lumen.wait_for("the good module to be scanned", |app| {
        app.plugin_status().contains("done: 1")
    });
    assert!(lumen.shows("Cancel"));
    assert!(lumen.shows_row("Scanning 1 of 2"));
    lumen.click("Cancel");
    lumen.settled();
    assert_eq!(
        lumen.app().plugin_names(),
        [EFFECT, INSTRUMENT],
        "what finished stays"
    );
    assert!(
        lumen.app().blocked_plugins().is_empty(),
        "an unfinished scan blocks nothing"
    );
    assert!(lumen.shows("Rescan"));
}
