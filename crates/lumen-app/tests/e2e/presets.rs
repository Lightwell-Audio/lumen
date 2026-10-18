//! Programs and presets on plugin devices (M6 slice 5c): a program chosen is
//! one undo step with the values the plugin sets in answer, and is heard; a
//! preset file's state is tried on the running plugin, put into the project as
//! one undo step, and given back exactly by undo; a preset made for another
//! plugin, or a state the plugin refuses, changes nothing. The plugins and the
//! host program are the real ones, built by the test; the card's preset menu is
//! slice 5f's, so these drive the same commands the menu will.

use eframe::egui::{Key, Modifiers};
use lumen_hosting::Preset;

use super::hosting::{
    LEVEL, PLUGIN, WAVE, instrument_device, launch, level, play, project, wait_for_state,
};

/// The test instrument's program-change parameter.
const PROGRAM: u32 = 91;

/// A preset file for the test instrument holding `level` and `wave` (the
/// plugin's saved state is its tag, its kind (0 for the instrument) and its two
/// parameters' values).
pub(super) fn preset_for_instrument(level: f64, wave: f64) -> Vec<u8> {
    Preset {
        class_id: lumen_test_plugin::INSTRUMENT.map(|c| c as u8),
        component: state(level, wave),
        controller: None,
        info: None,
    }
    .to_bytes()
}

pub(super) fn state(level: f64, wave: f64) -> Vec<u8> {
    [
        b"LTP1".as_slice(),
        &[0],
        &level.to_le_bytes(),
        &wave.to_le_bytes(),
    ]
    .concat()
}

fn held_state(lumen: &super::Lumen) -> Vec<u8> {
    lumen.app().project().tracks[0].devices[0].state.clone()
}

#[test]
fn choosing_a_program_is_one_undo_step_it_is_heard_and_undo_goes_back() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("program", &project, &["Presets"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.wait_for("the plugin's programs", |app| {
        !app.device_programs(PLUGIN).is_empty()
    });
    assert_eq!(
        lumen.app().device_programs(PLUGIN),
        [(
            "Factory".to_owned(),
            vec![
                "Square, quiet".to_owned(),
                "Saw, loud".to_owned(),
                "Saw, half".to_owned()
            ]
        )]
    );
    play(&mut lumen);
    let before = level(&mut lumen);
    assert!(before > 0.05, "the instrument is silent: {before}");

    // "Saw, loud": the plugin sets its own level and wave when the program
    // parameter moves, and what it sets is part of the choice.
    lumen.app_mut().choose_program(PLUGIN, 0, 1);
    lumen.wait_for("the program's values to be in the project", |app| {
        let held = app.device_params(PLUGIN);
        held.contains(&(PROGRAM, 0.5))
            && held.contains(&(LEVEL, 0.9))
            && held.contains(&(WAVE, 1.0))
    });
    lumen.frames(20);
    assert_eq!(lumen.app().undo_label(), Some("Load Saw, loud"));
    let loud = level(&mut lumen);
    assert!(loud > before * 1.3, "{loud} is not louder than {before}");

    // One undo takes the whole choice back, and the plugin with it.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the plugin to go back", |app| {
        app.device_params(PLUGIN).is_empty()
            && app.device_param_known(PLUGIN, LEVEL) == Some(0.5)
            && app.device_param_known(PLUGIN, WAVE) == Some(0.0)
            && app.device_param_known(PLUGIN, PROGRAM) == Some(0.0)
    });
    let again = level(&mut lumen);
    assert!(again < loud * 0.8, "{again} is not back below {loud}");

    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.wait_for("the program to come back", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.9)
            && app.device_param_known(PLUGIN, PROGRAM) == Some(0.5)
    });
}

#[test]
fn a_preset_file_is_one_undo_step_and_undo_gives_the_old_sound_back() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("preset-file", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    let before = level(&mut lumen);
    assert!(held_state(&lumen).is_empty());

    lumen
        .app_mut()
        .load_preset_from_bytes(PLUGIN, "Quiet square", &preset_for_instrument(0.1, 0.0))
        .expect("a good file");
    // The project holds the state once the plugin has taken it.
    lumen.wait_for("the project to hold the preset", |app| {
        app.project().tracks[0].devices[0].state == state(0.1, 0.0)
    });
    lumen.wait_for("the plugin's values to be read back", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.1)
    });
    assert_eq!(lumen.app().undo_label(), Some("Load Quiet square"));
    assert!(lumen.app().is_dirty());
    let quiet = level(&mut lumen);
    assert!(quiet < before * 0.5, "{quiet} is not quieter than {before}");
    // The plugin was given the state once: the project did not send it again.
    lumen.frames(30);
    assert_eq!(lumen.app().device_param_known(PLUGIN, LEVEL), Some(0.1));

    // Undo gives back what the project held: no state at all, so the plugin
    // starts again as it was added.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the project to hold no state", |app| {
        app.project().tracks[0].devices[0].state.is_empty()
    });
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.wait_for("the plugin to start again", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.5)
    });
    // A plugin instrument that starts again does not sound a note that is
    // already held until the next one (KNOWN_ISSUES): play from the start.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback to stop", |app| !app.is_playing());
    play(&mut lumen);
    let back = level(&mut lumen);
    assert!(back > quiet * 2.0, "{back} is not back above {quiet}");

    // Redo gives the preset's state to the running plugin again.
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    lumen.wait_for("the plugin to take the preset again", |app| {
        app.project().tracks[0].devices[0].state == state(0.1, 0.0)
            && app.device_param_known(PLUGIN, LEVEL) == Some(0.1)
    });
}

#[test]
fn a_second_preset_is_undone_to_the_first_without_restarting_the_plugin() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("preset-twice", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    for (name, level) in [("First", 0.2), ("Second", 0.8)] {
        lumen
            .app_mut()
            .load_preset_from_bytes(PLUGIN, name, &preset_for_instrument(level, 0.0))
            .unwrap();
        lumen.wait_for("the preset to be held", move |app| {
            app.project().tracks[0].devices[0].state == state(level, 0.0)
        });
        lumen.frames(10);
    }
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the first preset to be back in the plugin", |app| {
        app.project().tracks[0].devices[0].state == state(0.2, 0.0)
            && app.device_param_known(PLUGIN, LEVEL) == Some(0.2)
    });
    // The host kept running: a state was loaded, nothing was started again.
    assert_eq!(lumen.app().device_state(PLUGIN).as_deref(), Some("ready"));
}

#[test]
fn a_preset_made_for_another_plugin_changes_nothing() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("preset-foreign", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let before = lumen.app().project().clone();
    let foreign = Preset {
        class_id: [9; 16],
        component: state(0.1, 0.0),
        controller: None,
        info: None,
    }
    .to_bytes();
    lumen
        .app_mut()
        .load_preset_from_bytes(PLUGIN, "Elsewhere", &foreign)
        .unwrap();
    lumen.frames(30);
    assert_eq!(lumen.app().project(), &before);
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("made for another plugin")),
        "{:?}",
        lumen.app().note()
    );
    assert!(!lumen.app().is_dirty());
}

#[test]
fn a_state_the_plugin_refuses_changes_nothing_and_says_so() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("preset-refused", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let before = lumen.app().project().clone();
    let damaged = Preset {
        class_id: lumen_test_plugin::INSTRUMENT.map(|c| c as u8),
        component: vec![7; 40],
        controller: None,
        info: None,
    }
    .to_bytes();
    lumen
        .app_mut()
        .load_preset_from_bytes(PLUGIN, "Broken", &damaged)
        .unwrap();
    lumen.wait_for("the refusal", |app| {
        app.note().is_some_and(|n| n.contains("could not use"))
    });
    assert_eq!(lumen.app().project(), &before);
    assert!(!lumen.app().is_dirty());
    assert_eq!(lumen.app().device_state(PLUGIN).as_deref(), Some("ready"));
    // The plugin is as it was: a good preset still loads.
    lumen
        .app_mut()
        .load_preset_from_bytes(PLUGIN, "Fine", &preset_for_instrument(0.3, 0.0))
        .unwrap();
    lumen.wait_for("the good preset", |app| {
        app.project().tracks[0].devices[0].state == state(0.3, 0.0)
    });
}

#[test]
fn a_damaged_file_is_refused_before_the_plugin_hears_of_it() {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("preset-damaged", &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    let before = lumen.app().project().clone();
    let mut file = preset_for_instrument(0.1, 0.0);
    file.truncate(file.len() - 25);
    assert!(
        lumen
            .app_mut()
            .load_preset_from_bytes(PLUGIN, "Cut short", &file)
            .is_err()
    );
    assert!(
        lumen
            .app_mut()
            .load_preset_from_bytes(PLUGIN, "Not a preset", b"hello, this is text")
            .is_err()
    );
    assert_eq!(lumen.app().project(), &before);
}
