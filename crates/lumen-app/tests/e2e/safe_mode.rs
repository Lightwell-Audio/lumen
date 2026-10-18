//! Safe mode (M6 slice 5e): no plugin is scanned, loaded or hosted; the cards
//! keep their settings and the project saves them as they were; one plugin can
//! be loaded at a time; leaving loads the rest; and a start that never
//! finished loading plugins is offered safe mode, with nothing loaded until it
//! is answered. The plugins and the host program are the real ones, built by
//! the test.

use std::path::{Path, PathBuf};

use eframe::egui::{Event, Key, Modifiers};
use std::time::Duration;

use lumen_project::model::{Device, Id, Project};
use lumen_project::testing::TempDir;

use super::Lumen;
use super::hosting::{PLUGIN, effect_device, instrument_device, project, wait_for_state};
use super::plugins::{built, plugin_root};

const SECOND: Id = Id(21);
const MARKER: &str = "plugin-loading.marker";

/// A device with a saved state of its own, as a plugin that was set up has.
fn with_state(mut device: Device, bytes: &[u8]) -> Device {
    device.state = bytes.to_vec();
    device
}

fn song(devices: Vec<Device>) -> Project {
    project(|d| *d = devices)
}

fn marker(dir: &Path) -> PathBuf {
    dir.join(MARKER)
}

/// A start of Lumen on `project` in `dir`, with the test plugin installed.
fn start(
    dir: TempDir,
    project: &Project,
    adjust: impl FnOnce(lumen_app::Launch) -> lumen_app::Launch,
) -> Lumen {
    let path = dir.path().join("song.lumen");
    if !path.exists() {
        lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    }
    // The folder of plugins is made once: a second start finds the first
    // start's, and hosts of that start may still hold its files.
    let root = if dir.path().join("plugins").exists() {
        dir.path().join("plugins")
    } else {
        plugin_root(dir.path(), &["Plug"])
    };
    let cache = dir.path().join("plugin-cache.json");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |launch| {
        adjust(launch.opening(path).with_audio_buffer(1024).with_plugins(
            vec![root],
            &built().1,
            cache,
        ))
    });
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);
    lumen
}

/// A normal run first, so the plugin cache holds what is installed, then the
/// same folder started again with `adjust`.
fn after_a_normal_run(
    name: &str,
    project: &Project,
    adjust: impl FnOnce(lumen_app::Launch) -> lumen_app::Launch,
) -> Lumen {
    let dir = TempDir::new(&format!("e2e-safe-{name}"));
    let mut first = start(dir, project, |l| l);
    first.wait_for("the scan", |app| !app.plugins_busy());
    wait_for_state(&mut first, PLUGIN, "ready");
    let dir = first.crash();
    // What the cache holds is on disk by now (it is written as a scan ends).
    assert!(dir.path().join("plugin-cache.json").exists());
    start(dir, project, adjust)
}

#[test]
fn safe_mode_loads_no_plugin_scans_nothing_and_saves_the_project_as_it_was() {
    let state = b"LTP1\x00some saved sound";
    let project = song(vec![with_state(instrument_device(PLUGIN), state)]);
    let dir = TempDir::new("e2e-safe-flag");
    let mut lumen = start(dir, &project, lumen_app::Launch::in_safe_mode);
    assert_eq!(lumen.app().safe_mode(), (true, false));
    wait_for_state(&mut lumen, PLUGIN, "safe");
    assert!(lumen.shows("Leave safe mode"));
    // Not a host, not a scan, however long it waits.
    std::thread::sleep(Duration::from_millis(1500));
    lumen.frames(10);
    assert_eq!(lumen.app().plugin_hosts(), 0);
    assert!(lumen.app().plugin_names().is_empty(), "nothing was scanned");
    assert_eq!(
        lumen.app().device_state(PLUGIN).as_deref(),
        Some("safe"),
        "still not loaded"
    );

    // What the project saves is what it holds: the settings are kept.
    let path = lumen.dir().join("saved.lumen");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy());
    let reopened = lumen_project::load(&path).expect("the saved project loads");
    assert_eq!(reopened.tracks[0].devices[0].state, state);
}

#[test]
fn leaving_safe_mode_loads_the_plugins() {
    let project = song(vec![instrument_device(PLUGIN)]);
    let mut lumen = after_a_normal_run("leave", &project, lumen_app::Launch::in_safe_mode);
    wait_for_state(&mut lumen, PLUGIN, "safe");
    lumen.click("Leave safe mode");
    assert_eq!(lumen.app().safe_mode(), (false, false));
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(!lumen.shows("Leave safe mode"));
    assert_eq!(lumen.app().plugin_hosts(), 1);
}

#[test]
fn one_plugin_can_be_loaded_in_safe_mode_and_the_rest_wait() {
    let project = song(vec![instrument_device(PLUGIN), effect_device(SECOND)]);
    let mut lumen = after_a_normal_run("one", &project, lumen_app::Launch::in_safe_mode);
    wait_for_state(&mut lumen, PLUGIN, "safe");
    wait_for_state(&mut lumen, SECOND, "safe");
    lumen.app_mut().load_one(SECOND);
    wait_for_state(&mut lumen, SECOND, "ready");
    lumen.frames(20);
    assert_eq!(
        lumen.app().device_state(PLUGIN).as_deref(),
        Some("safe"),
        "the other is still not loaded"
    );
    assert_eq!(lumen.app().plugin_hosts(), 1);
    assert_eq!(lumen.app().safe_mode(), (true, false));
}

#[test]
fn a_start_that_never_finished_loading_plugins_is_offered_safe_mode_and_nothing_loads_until_answered()
 {
    let project = song(vec![instrument_device(PLUGIN)]);
    let dir = TempDir::new("e2e-safe-offer");
    // What a start cut short leaves behind.
    std::fs::write(marker(dir.path()), b"plugins were loading\n").unwrap();
    let mut lumen = start(dir, &project, |l| l);
    assert_eq!(lumen.app().safe_mode(), (true, true));
    assert!(lumen.shows("Start in safe mode"));
    assert!(lumen.shows("Load plugins"));
    std::thread::sleep(Duration::from_millis(1500));
    lumen.frames(10);
    assert_eq!(
        lumen.app().plugin_hosts(),
        0,
        "nothing loads before the answer"
    );
    assert_eq!(lumen.app().device_state(PLUGIN).as_deref(), Some("safe"));

    // Starting in safe mode keeps it, and keeps the marker for the next start.
    lumen.click("Start in safe mode");
    assert_eq!(lumen.app().safe_mode(), (true, false));
    assert!(lumen.shows("Leave safe mode"));
    assert!(marker(lumen.dir()).exists());
    assert_eq!(lumen.app().plugin_hosts(), 0);
}

#[test]
fn answering_the_offer_with_load_plugins_loads_them_and_clears_the_marker_once_they_have() {
    let project = song(vec![instrument_device(PLUGIN)]);
    let mut lumen = {
        // The cache is there from a normal run; then a cut-short start.
        let dir = TempDir::new("e2e-safe-offer-load");
        let mut first = start(dir, &project, |l| l);
        first.wait_for("the scan", |app| !app.plugins_busy());
        wait_for_state(&mut first, PLUGIN, "ready");
        let dir = first.crash();
        std::fs::write(marker(dir.path()), b"plugins were loading\n").unwrap();
        start(dir, &project, |l| l)
    };
    assert_eq!(lumen.app().safe_mode(), (true, true));
    lumen.click("Load plugins");
    assert_eq!(lumen.app().safe_mode(), (false, false));
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let path = marker(lumen.dir());
    lumen.wait_for("the marker to go once loading has finished", move |_| {
        !path.exists()
    });
}

#[test]
fn the_flag_goes_with_a_marker_without_an_offer_and_keeps_the_marker() {
    let project = song(vec![instrument_device(PLUGIN)]);
    let dir = TempDir::new("e2e-safe-flag-marker");
    std::fs::write(marker(dir.path()), b"plugins were loading\n").unwrap();
    let lumen = start(dir, &project, lumen_app::Launch::in_safe_mode);
    assert_eq!(lumen.app().safe_mode(), (true, false));
    assert!(marker(lumen.dir()).exists());
}

#[test]
fn a_normal_start_leaves_no_marker_behind_once_its_plugins_have_loaded() {
    let project = song(vec![instrument_device(PLUGIN)]);
    let dir = TempDir::new("e2e-safe-clean");
    let mut lumen = start(dir, &project, |l| l);
    assert_eq!(lumen.app().safe_mode(), (false, false));
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let path = marker(lumen.dir());
    lumen.wait_for("the marker to be gone", move |_| !path.exists());
}

#[test]
fn leave_safe_mode_is_a_command_only_while_in_it() {
    let project = song(vec![instrument_device(PLUGIN)]);
    let dir = TempDir::new("e2e-safe-command");
    let mut lumen = start(dir, &project, lumen_app::Launch::in_safe_mode);
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Leave safe mode".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    assert_eq!(lumen.app().safe_mode(), (false, false));
}
