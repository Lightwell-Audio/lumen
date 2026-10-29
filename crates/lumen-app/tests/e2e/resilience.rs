//! A plugin that fails or changes without a word (M6 slice 5d): running
//! plugins are read for checkpoints, a plugin that stopped starts again from its
//! last good state and the values that moved since, "Reload without sound"
//! starts it as it was added, and one that keeps stopping is counted and never
//! restarted by itself. The plugins and the host program are the real ones,
//! built by the test.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_project::model::Id;

use super::hosting::{
    LEVEL, PLUGIN, WAVE, effect_device, instrument_device, launch, play, project, wait_for_state,
};

/// The wave a test instrument's saved state holds (its tag, kind, level, wave).
fn saved_wave(lumen: &super::Lumen) -> Option<f64> {
    let state = lumen.app().project_as_saved().tracks[0].devices[0]
        .state
        .clone();
    (state.len() == 21).then(|| f64::from_le_bytes(state[13..21].try_into().unwrap()))
}

/// The instrument playing, with its checkpoint every 300 ms and a wave set
/// inside the plugin without a word, which a checkpoint then reads.
fn with_a_checkpoint(name: &str) -> super::Lumen {
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch(name, &project, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    play(&mut lumen);
    lumen
        .app_mut()
        .set_checkpoint_every(Duration::from_millis(300));
    // The plugin changes with no report to Lumen, and no project edit.
    lumen.app_mut().poke_plugin(PLUGIN, WAVE, 1.0);
    lumen.wait_for("a checkpoint to read the plugin", |app| {
        app.project_as_saved().tracks[0].devices[0].state.len() == 21
    });
    lumen.wait_for("the checkpoint to hold the wave", |app| {
        let state = &app.project_as_saved().tracks[0].devices[0].state;
        state.len() == 21 && f64::from_le_bytes(state[13..21].try_into().unwrap()) == 1.0
    });
    // No more checkpoints: what follows is after the last good state.
    lumen
        .app_mut()
        .set_checkpoint_every(Duration::from_secs(3600));
    lumen
}

#[test]
fn a_running_plugin_is_read_for_a_checkpoint_though_it_reported_no_change() {
    let mut lumen = with_a_checkpoint("checkpoint");
    assert_eq!(saved_wave(&lumen), Some(1.0));
    assert!(lumen.app().device_last_good_age(PLUGIN).is_some());
    // Nothing is an edit of the project: its own state is what moved.
    assert!(lumen.app().device_params(PLUGIN).is_empty());
    lumen.frames(1);
}

#[test]
fn a_plugin_that_stopped_starts_again_from_its_last_good_state_and_the_values_since() {
    let mut lumen = with_a_checkpoint("last-good");
    // After the checkpoint, the user changes a value: the project holds it.
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.8);
    lumen.frames(5);
    lumen.wait_for("the value to reach the plugin", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.8)
    });

    lumen.app_mut().end_device_host(PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "failed");
    lumen.app_mut().reload_device(PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    // The wave came from the last good state (the project holds none), and the
    // level from what the project holds over it.
    lumen.wait_for("the plugin to be as it was", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(1.0)
            && app.device_param_known(PLUGIN, LEVEL) == Some(0.8)
    });
}

#[test]
fn reload_without_sound_starts_the_plugin_as_it_was_added_and_keeps_the_projects_state() {
    let mut lumen = with_a_checkpoint("clean");
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.8);
    lumen.frames(5);
    lumen.app_mut().end_device_host(PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "failed");
    lumen.app_mut().reload_device_clean(PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    // No sound of its own, but what the project holds is set over it.
    lumen.wait_for("the plugin to start clean", |app| {
        app.device_param_known(PLUGIN, WAVE) == Some(0.0)
            && app.device_param_known(PLUGIN, LEVEL) == Some(0.8)
    });
    // The project is not touched by it.
    assert!(
        lumen.app().project().tracks[0].devices[0].state.is_empty(),
        "the project's own state is left until the next save"
    );
}

#[test]
fn a_plugin_that_keeps_stopping_is_counted_and_never_restarted_by_itself() {
    // The effect's host crashes on its 40th block, every time it is started.
    let project = project(|d| d.push(effect_device(PLUGIN)));
    let mut lumen = launch("keeps-stopping", &project, &["Dies"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    for failures in 1..=3 {
        play(&mut lumen);
        wait_for_state(&mut lumen, PLUGIN, "failed");
        assert_eq!(
            lumen.app().device_keeps_failing(PLUGIN),
            failures >= 3,
            "after {failures} failures"
        );
        // Nothing starts it again by itself, however long it waits.
        std::thread::sleep(Duration::from_millis(1500));
        lumen.frames(5);
        assert!(
            lumen
                .app()
                .device_state(PLUGIN)
                .is_some_and(|s| s.starts_with("failed")),
            "restarted without being asked"
        );
        if failures < 3 {
            lumen.key(Modifiers::NONE, Key::Space);
            lumen.wait_for("stop", |app| !app.is_playing());
            lumen.app_mut().reload_device(PLUGIN);
            wait_for_state(&mut lumen, PLUGIN, "ready");
        }
    }
}

/// The chain's tab for the track the devices are on.
fn show_cards(lumen: &mut super::Lumen) {
    lumen.app_mut().select_tracks(&[Id(1)]);
    lumen.show_whole_cards();
}

#[test]
fn a_stopped_card_says_which_sound_reload_starts_from_and_how_old_it_is() {
    let mut lumen = with_a_checkpoint("card-first");
    show_cards(&mut lumen);
    lumen.app_mut().end_device_host(PLUGIN);
    wait_for_state(&mut lumen, PLUGIN, "failed");
    lumen.frames(10);
    // One failure: the plain card, with the one button.
    let words = lumen
        .harness
        .get_by_label_contains("Stopped responding")
        .accesskit_node()
        .value()
        .unwrap_or_default();
    assert!(
        words.contains("Reload starts it again from its sound from"),
        "{words}"
    );
    assert!(words.contains(" ago"), "{words}");
    assert!(lumen.shows("Reload"));
    assert!(
        !lumen.shows("Reload without sound"),
        "only once it keeps stopping"
    );
}

#[test]
fn a_card_that_keeps_stopping_offers_reload_without_sound_and_it_starts_clean() {
    let project = project(|d| d.push(effect_device(PLUGIN)));
    let mut lumen = launch("keeps-card", &project, &["Dies"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    show_cards(&mut lumen);
    for failures in 1..=3 {
        play(&mut lumen);
        wait_for_state(&mut lumen, PLUGIN, "failed");
        // Two words about one failure within a second count once.
        std::thread::sleep(Duration::from_millis(1200));
        lumen.key(Modifiers::NONE, Key::Space);
        lumen.wait_for("stop", |app| !app.is_playing());
        if failures < 3 {
            lumen.app_mut().reload_device(PLUGIN);
            wait_for_state(&mut lumen, PLUGIN, "ready");
        }
    }
    lumen.frames(10);
    assert!(lumen.app().device_keeps_failing(PLUGIN));
    let words = lumen
        .harness
        .get_by_label_contains("Keeps stopping")
        .accesskit_node()
        .value()
        .unwrap_or_default();
    assert!(
        words.contains("stopped three times in two minutes"),
        "{words}"
    );
    assert!(lumen.shows("Reload") && lumen.shows("Reload without sound"));
    lumen.click("Reload without sound");
    wait_for_state(&mut lumen, PLUGIN, "ready");
    assert!(!lumen.shows("Reload without sound"));
}
