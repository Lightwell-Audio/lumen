//! Stepped parameters on a card's strip (M6 slice 5g, the owner's A): a
//! parameter with two states is a switch, one with a few named choices is a
//! chooser (click for the next, the arrow for the list), and every click is one
//! undo step that lands at once. A continuous parameter stays a knob. The
//! plugin is the test instrument with its programs (`Presets`): its Wave is a
//! two-state parameter and its Program has three named choices.

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_project::model::Id;
use lumen_project::testing::TempDir;

use super::Lumen;
use super::hosting::{LEVEL, PLUGIN, WAVE, faults, instrument_device, project, wait_for_state};
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
const PROGRAM: u32 = 91;

fn launch(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-stepped-{name}"));
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), &["Presets"]);
    let cache = dir.path().join("plugin-cache.json");
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
    });
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.wait_for("the plugin scan", |app| !app.plugins_busy());
    lumen.app_mut().select_tracks(&[KEYS]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.wait_for("the plugin's parameters", |app| {
        app.device_programs(PLUGIN).len() == 1
    });
    // Level (continuous), Wave (two states) and Program (three choices).
    lumen
        .app_mut()
        .show_on_strip(PLUGIN, &[LEVEL, WAVE, PROGRAM]);
    lumen.show_whole_cards();
    lumen
}

#[test]
fn each_kind_of_parameter_gets_the_control_that_fits_it() {
    let lumen = launch("kinds");
    // Continuous: a knob (a slider to the accessibility tree).
    assert!(
        lumen.shows("Level, parameter: 50 %")
            || lumen
                .harness
                .query_by_label_contains("Level, parameter")
                .is_some()
    );
    // Two states: a switch, named for the parameter, off.
    assert!(lumen.shows("Wave"));
    assert!(!lumen.is_on("Wave"));
    // A short list: the plugin's own word, and the arrow that lists them.
    assert!(lumen.shows("Choose Program"));
    assert!(
        lumen
            .harness
            .query_by_label_contains("Program, choice:")
            .is_some()
    );
}

#[test]
fn a_switch_is_one_undo_step_a_click_and_lands_at_once() {
    let mut lumen = launch("switch");
    lumen.click("Wave");
    lumen.wait_for("the plugin to have it", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(1.0)
    });
    assert!(lumen.is_on("Wave"));
    assert_eq!(lumen.app().undo_label(), Some("Change Wave"));
    // A second click is a step of its own.
    lumen.click("Wave");
    lumen.wait_for("it to go back", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(0.0)
    });
    assert!(!lumen.is_on("Wave"));
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("one undo to bring the first click back", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(1.0)
    });
}

#[test]
fn a_chooser_clicks_to_the_next_choice_and_wraps_and_names_choices_in_the_plugins_words() {
    let mut lumen = launch("next");
    lumen.wait_for("the plugin's words for the choices", |app| {
        app.device_choices(PLUGIN, PROGRAM)
            == ["Square, quiet", "Saw, loud", "Saw, half"].map(str::to_owned)
    });
    let at = |lumen: &Lumen, word: &str| {
        lumen
            .harness
            .query_by_label(&format!("Program, choice: {word}"))
            .is_some()
    };
    assert!(at(&lumen, "Square, quiet"));
    lumen.click("Program, choice: Square, quiet");
    lumen.wait_for("the second choice", |app| {
        app.device_param_known(PLUGIN, PROGRAM) == Some(0.5)
    });
    lumen.frames(10);
    assert!(at(&lumen, "Saw, loud"));
    assert_eq!(
        lumen
            .app()
            .undo_label()
            .map(|l| l.starts_with("Change Program")),
        Some(true)
    );
    lumen.click("Program, choice: Saw, loud");
    lumen.click("Program, choice: Saw, half");
    lumen.wait_for("round to the first", |app| {
        app.device_param_known(PLUGIN, PROGRAM) == Some(0.0)
    });
}

#[test]
fn the_arrow_lists_every_choice_and_a_row_picks_it() {
    let mut lumen = launch("list");
    lumen.wait_for("the plugin's words", |app| {
        app.device_choices(PLUGIN, PROGRAM).len() == 3
    });
    lumen.click("Choose Program");
    for row in ["Square, quiet", "Saw, loud", "Saw, half"] {
        assert!(lumen.shows(row), "{row} is in the list");
    }
    lumen.click("Saw, half");
    lumen.wait_for("the last choice", |app| {
        app.device_param_known(PLUGIN, PROGRAM) == Some(1.0)
    });
    // Picking closes the list.
    lumen.frames(30);
    assert!(!lumen.shows("Square, quiet"));
}
