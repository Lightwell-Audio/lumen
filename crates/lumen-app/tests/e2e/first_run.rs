//! The first-run window (M9 R7b, ADR-0115): it opens on a first start (here
//! with `--first-run`, as the harness never opens it otherwise), walks
//! Sound, Plugins and Ready, plays the sound check left, right, then both,
//! and closing it in any way marks setup done. Esc goes to an open menu
//! before it skips setup.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_project::testing::TempDir;

use super::Lumen;
use super::plugins::{plugin_root, start_in};

fn first_run(name: &str) -> Lumen {
    let mut lumen = Lumen::start_with(TempDir::new(&format!("e2e-first-run-{name}")), |l| {
        l.first_run()
    });
    lumen.run_until("the welcome window", |l| {
        l.shows("Let\u{2019}s hear something")
    });
    lumen
}

#[test]
fn the_window_walks_sound_plugins_and_ready_and_start_marks_setup_done() {
    let mut lumen = first_run("walk");
    assert_eq!(lumen.app().welcome_step(), Some("Sound"));
    assert!(!lumen.app().setup_done());
    lumen.click("Next");
    lumen.run_until("the Plugins step", |l| l.shows("Bring your plugins"));
    assert_eq!(lumen.app().welcome_step(), Some("Plugins"));
    // Plugins are off in the harness: the step says so.
    assert!(lumen.shows("This run started with plugins off. Start again normally to load them."));
    lumen.click("Next");
    lumen.run_until("the Ready step", |l| l.shows("Almost set"));
    assert!(lumen.shows("Start making music"));
    // Back, then forward with a click on the pill.
    lumen.click("Back");
    lumen.run_until("Plugins again", |l| {
        l.app().welcome_step() == Some("Plugins")
    });
    lumen.click("3 Ready tab");
    lumen.run_until("Ready again", |l| l.app().welcome_step() == Some("Ready"));
    lumen.click("Start making music");
    lumen.run_until("the window closed", |l| l.app().welcome_step().is_none());
    assert!(lumen.app().setup_done());
}

#[test]
fn the_sound_check_plays_left_then_right_then_both() {
    let mut lumen = first_run("chime");
    lumen.click("Play test sound");
    for side in ["left", "right", "both", ""] {
        lumen.run_until(&format!("the chime's {side:?} part"), |l| {
            l.app().sound_check_side() == side
        });
    }
    // Stop ends a sound check at once.
    lumen.click("Play test sound");
    lumen.run_until("the chime", |l| !l.app().sound_check_side().is_empty());
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.run_until("the chime stopped", |l| {
        l.app().sound_check_side().is_empty()
    });
}

#[test]
fn esc_closes_an_open_menu_first_then_skips_setup() {
    let mut lumen = first_run("esc");
    lumen.click("Driver: Test device");
    lumen.run_until("the driver menu", |l| l.shows("Test device that refuses"));
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.run_until("the menu closed", |l| !l.shows("Test device that refuses"));
    lumen.frames(3);
    assert_eq!(
        lumen.app().welcome_step(),
        Some("Sound"),
        "Esc closed the menu only"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.run_until("the window closed", |l| l.app().welcome_step().is_none());
    assert!(lumen.app().setup_done(), "skipping counts as done");
}

#[test]
fn skip_setup_closes_and_the_menu_opens_it_again_on_sound() {
    let mut lumen = first_run("again");
    lumen.click("Skip setup");
    lumen.run_until("the window closed", |l| l.app().welcome_step().is_none());
    lumen.click("Lumen");
    lumen.click("Set up Lumen\u{2026}");
    lumen.run_until("the window again", |l| {
        l.app().welcome_step() == Some("Sound")
    });
}

#[test]
fn the_harness_alone_never_opens_the_window() {
    let mut lumen = Lumen::start("first-run-none");
    lumen.frames(5);
    assert_eq!(lumen.app().welcome_step(), None);
}

#[test]
fn with_plugins_scanned_the_plugins_step_counts_them_and_ready_says_set() {
    let dir = TempDir::new("e2e-first-run-plugins");
    let root = plugin_root(dir.path(), &["Good"]);
    let mut lumen = start_in(dir, root, |l| l.first_run());
    lumen.click("2 Plugins tab");
    lumen.run_until("the count", |l| {
        l.harness.query_by_label_contains("ready to use").is_some()
    });
    assert!(!lumen.shows("This run started with plugins off. Start again normally to load them."));
    lumen.click("Next");
    lumen.run_until("the Ready step", |l| l.shows("You are set"));
}

#[test]
fn enter_moves_on_from_next_which_has_the_keyboard_when_a_step_opens() {
    let mut lumen = first_run("enter");
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the Plugins step", |l| {
        l.app().welcome_step() == Some("Plugins")
    });
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the Ready step", |l| {
        l.app().welcome_step() == Some("Ready")
    });
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the window closed", |l| l.app().welcome_step().is_none());
    assert!(lumen.app().setup_done());
}

#[test]
fn the_palette_opens_the_window_again() {
    let mut lumen = Lumen::start("first-run-palette");
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(eframe::egui::Event::Text("Set up".into()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.run_until("the window", |l| l.app().welcome_step() == Some("Sound"));
}

/// Starts on the settings in `file`, with the first-run window following
/// them as on a real start.
fn real_start(dir: TempDir, file: &std::path::Path) -> Lumen {
    Lumen::start_with(dir, |l| l.with_settings_file(file).with_first_start_rule())
}

#[test]
fn quitting_unfinished_brings_the_window_back_and_skipping_is_kept() {
    let dir = TempDir::new("e2e-first-run-restart");
    let file = dir.path().join("settings.json");
    let mut lumen = real_start(dir, &file);
    lumen.run_until("the window on a first start", |l| {
        l.app().welcome_step() == Some("Sound")
    });
    lumen.click("Next");
    // Quit without finishing: the next start opens it again, on Sound.
    let dir = lumen.crash();
    let mut again = real_start(dir, &file);
    again.run_until("the window again", |l| {
        l.app().welcome_step() == Some("Sound")
    });
    again.click("Skip setup");
    again.wait_for("setup saved as done", |_| {
        std::fs::read_to_string(&file).is_ok_and(|text| text.contains("\"setup_done\": true"))
    });
    let dir = again.crash();
    let mut third = real_start(dir, &file);
    third.frames(10);
    assert_eq!(third.app().welcome_step(), None, "setup was done");
}

#[test]
fn esc_with_history_open_closes_history_and_keeps_setup_open() {
    let mut lumen = first_run("history");
    lumen.click("Lumen");
    lumen.click("History\u{2026}");
    lumen.run_until("History", |l| l.app().history_shown());
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.run_until("History closed", |l| !l.app().history_shown());
    lumen.frames(3);
    assert_eq!(lumen.app().welcome_step(), Some("Sound"));
    assert!(!lumen.app().setup_done(), "setup was not skipped");
}

#[test]
fn audio_off_during_the_sound_check_turns_the_lights_off() {
    let mut lumen = first_run("lights");
    lumen.click("Play test sound");
    lumen.run_until("the chime", |l| !l.app().sound_check_side().is_empty());
    super::clip_commands::command(&mut lumen, "Audio on or off");
    lumen.run_until("audio off", |l| !l.app().audio_running());
    assert_eq!(lumen.app().sound_check_side(), "", "no light stays lit");
    assert!(lumen.shows("Audio on"), "the step offers to turn it on");
}

/// The chime as heard: a note on the left alone, then one on the right
/// alone, then both, never louder than −18 dBFS.
#[test]
fn the_sound_check_is_heard_left_then_right_then_both() {
    let mut lumen = first_run("heard");
    lumen.app_mut().heard_all();
    lumen.click("Play test sound");
    let heard = lumen.listen(std::time::Duration::ZERO, 48_000 * 3 / 2);
    let loud = 0.01;
    let first_right = heard
        .iter()
        .position(|f| f[1].abs() > loud)
        .expect("the right side sounds");
    assert!(
        heard[..first_right].iter().any(|f| f[0].abs() > loud),
        "the left side sounds first"
    );
    let alone = &heard[first_right..first_right + 48_000 * 2 / 5];
    let left_then = alone.iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    assert!(
        left_then < 0.005,
        "the right note is alone: left at {left_then}"
    );
    assert!(
        heard[first_right..]
            .iter()
            .any(|f| f[0].abs() > loud && f[1].abs() > loud),
        "then both"
    );
    let peak = heard
        .iter()
        .flat_map(|f| [f[0], f[1]])
        .fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak <= 0.13, "peak {peak} is above -18 dBFS");
}

/// Preferences' Audio page and the Sound step show the same device rows at
/// once: each menu opens on its own.
#[test]
fn the_device_menus_in_preferences_and_setup_are_separate() {
    let mut lumen = first_run("two-menus");
    lumen.open_audio_settings();
    let drivers = || "Driver: Test device";
    let count = |l: &Lumen| l.harness.query_all_by_label(drivers()).count();
    lumen.run_until("both device rows", |l| count(l) == 2);
    lumen
        .harness
        .query_all_by_label(drivers())
        .next()
        .unwrap()
        .click();
    lumen.frames(3);
    let options = lumen
        .harness
        .query_all_by_label("Test device that refuses")
        .count();
    assert_eq!(options, 1, "one menu opened, not both");
}
