//! A lane that drives a plugin's parameter (M7 slice A3): played through the
//! whole app against the real test plugins, kept apart from the value the
//! project holds at rest, and chosen from a plugin's own list of parameters
//! with a search. The plugins are the test plugin's instrument and effect.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use super::Lumen;
use super::hosting::{
    PLUGIN, effect_device, instrument_device, launch, play, project, wait_for_state,
};

const KEYS: Id = Id(1);
const FX: Id = Id(21);
/// The parameter both test plugins give their level to.
const LEVEL: u32 = 0;
/// The effect's gain at 0 dB, normalized: 60 of its 72 dB.
const UNITY: f32 = 60.0 / 72.0;

fn lane(points: &[(i64, f32)]) -> Lane {
    Lane::new(
        points
            .iter()
            .map(|&(q, v)| Point::new(Ticks::quarters(q), v).with_shape(Shape::Step))
            .collect(),
    )
    .unwrap()
}

fn peak(lumen: &mut Lumen, ms: u64) -> f32 {
    lumen
        .listen(Duration::from_millis(ms), 4_800)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

#[test]
fn a_plugin_effects_parameter_lane_is_heard_changing_it_when_the_song_reaches_it() {
    // The effect's gain is held at its bottom (−60 dB) until two seconds in,
    // then opened to unity: the instrument is all but gone, then heard.
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let mut lumen = launch("lane-effect", &song, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    wait_for_state(&mut lumen, FX, "ready");
    lumen
        .app_mut()
        .give_lane(
            KEYS,
            Target::Device {
                device: FX,
                param: LEVEL,
            },
            lane(&[(0, 0.0), (4, UNITY)]),
        )
        .unwrap();
    lumen.frames(5);
    play(&mut lumen);
    lumen.wait_for("the lane's quiet part", |app| {
        app.playhead() > Ticks::quarters(1)
    });
    // An effect whose host answers late is passed through for that block, so
    // what the lane takes away is what the quietest stretch shows.
    let quiet = (0..4)
        .map(|_| peak(&mut lumen, 100))
        .fold(f32::MAX, f32::min);
    assert!(
        quiet < 0.005,
        "the gain lane holds the plugin at −60 dB: {quiet}"
    );
    lumen.wait_for("the lane opening", |app| {
        app.playhead() > Ticks::quarters(6)
    });
    let open = peak(&mut lumen, 300);
    assert!(open > 0.01, "and opens it: {open}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_plugin_instruments_parameter_lane_is_heard_changing_it_when_the_song_reaches_it() {
    let song = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let mut lumen = launch("lane-instrument", &song, &["Plug"]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen
        .app_mut()
        .give_lane(
            KEYS,
            Target::Device {
                device: PLUGIN,
                param: LEVEL,
            },
            lane(&[(0, 0.0), (4, 1.0)]),
        )
        .unwrap();
    lumen.frames(5);
    play(&mut lumen);
    lumen.wait_for("the lane's silent part", |app| {
        app.playhead() > Ticks::quarters(1)
    });
    let silent = peak(&mut lumen, 300);
    assert!(silent < 1e-4, "level 0 is silence: {silent}");
    lumen.wait_for("the lane opening", |app| {
        app.playhead() > Ticks::quarters(6)
    });
    let open = peak(&mut lumen, 300);
    assert!(open > 0.01, "level 1 is heard: {open}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn the_value_a_lane_drives_is_not_also_sent_to_the_plugin_over_the_control_channel() {
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let mut lumen = launch("lane-control", &song, &["Plug"]);
    wait_for_state(&mut lumen, FX, "ready");
    let target = Target::Device {
        device: FX,
        param: LEVEL,
    };
    // Not driven: a value the project holds reaches the plugin.
    lumen.app_mut().set_device_param(FX, LEVEL, 0.3);
    lumen.frames(30);
    let sent = lumen.app().values_sent_to(FX);
    assert!(sent >= 1, "a free parameter follows the project: {sent}");
    // Driven: the lane has it, so the project's value at rest is not sent.
    lumen
        .app_mut()
        .give_lane(KEYS, target, lane(&[(0, 0.5)]))
        .unwrap();
    lumen.frames(5);
    lumen.app_mut().set_device_param(FX, LEVEL, 0.9);
    lumen.frames(30);
    assert_eq!(lumen.app().values_sent_to(FX), sent, "the lane has it");
    // The lane off, the project's value is the plugin's again.
    lumen.app_mut().switch_lane(KEYS, target, false).unwrap();
    lumen.frames(30);
    assert!(lumen.app().values_sent_to(FX) > sent);
}

/// Lumen with the Keys track holding the instrument and the effect, selected,
/// its lane shown, and the chooser open on the effect.
fn chooser(name: &str, module: &str) -> Lumen {
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let mut lumen = launch(name, &song, &[module]);
    wait_for_state(&mut lumen, FX, "ready");
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.frames(10);
    lumen.key(Modifiers::NONE, Key::A);
    lumen.frames(40);
    lumen.click("Automation lane of Keys: Volume");
    lumen.click("Parameters of Deterministic Test Effect\u{2026}");
    lumen.frames(5);
    lumen
}

#[test]
fn the_chip_opens_a_plugins_parameters_and_choosing_one_shows_its_lane() {
    let mut lumen = chooser("lane-chooser", "Plug");
    assert!(lumen.shows("Search parameters to automate"));
    assert!(lumen.shows("Automate Gain"));
    lumen.click("Automate Gain");
    lumen.frames(5);
    assert_eq!(
        lumen.app().automation_shown(KEYS),
        Some(Target::Device {
            device: FX,
            param: LEVEL
        })
    );
    assert!(!lumen.shows("Search parameters to automate"), "it closes");
    assert!(
        lumen.shows("Automation lane of Keys: Deterministic Test Effect \u{00B7} Gain"),
        "the chip names the plugin's parameter in the plugin's words"
    );
}

#[test]
fn the_chooser_searches_by_name_and_chooses_with_the_keys_alone() {
    let mut lumen = chooser("lane-chooser-keys", "Plug");
    lumen.harness.event(Event::Text("lat".into()));
    lumen.frames(5);
    assert!(lumen.shows("Automate Latency"));
    assert!(!lumen.shows("Automate Gain"), "the search left it out");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert_eq!(
        lumen.app().automation_shown(KEYS),
        Some(Target::Device {
            device: FX,
            param: 1
        })
    );
}

#[test]
fn a_plugin_with_two_thousand_parameters_lists_only_the_rows_in_view_and_filters_at_once() {
    let mut lumen = chooser("lane-chooser-many", "Thousands");
    let rows = |lumen: &Lumen| {
        lumen
            .harness
            .query_all_by_label_contains("Automate ")
            .count()
    };
    let shown = rows(&lumen);
    assert!(shown > 0 && shown < 40, "{shown} rows are drawn");
    lumen.harness.event(Event::Text("param 1999".into()));
    lumen.frames(5);
    assert!(
        lumen.shows("Automate Param 1999"),
        "found among two thousand"
    );
    assert!(rows(&lumen) < 4);
    lumen.click("Automate Param 1999");
    lumen.frames(5);
    assert!(matches!(
        lumen.app().automation_shown(KEYS),
        Some(Target::Device { device, .. }) if device == FX
    ));
}

#[test]
fn a_lane_for_a_plugin_parameter_survives_a_save_and_moves_with_its_device() {
    let song = project(|d| *d = vec![instrument_device(PLUGIN), effect_device(FX)]);
    let mut lumen = launch("lane-save", &song, &["Plug"]);
    wait_for_state(&mut lumen, FX, "ready");
    let target = Target::Device {
        device: FX,
        param: LEVEL,
    };
    lumen
        .app_mut()
        .give_lane(KEYS, target, lane(&[(0, 0.2), (4, 0.9)]))
        .unwrap();
    let saved = lumen.app().project_as_saved();
    let kept = saved.tracks[0].lane(target).expect("saved with the track");
    assert_eq!(kept.lane.len(), 2);
    // The file holds it with the device: reading it back gives the same lane.
    let path = lumen.dir().join("lanes.lumen");
    lumen_project::save(&saved, &path, &Default::default()).unwrap();
    let back = lumen_project::load(&path).unwrap();
    assert_eq!(back.tracks[0].lane(target), saved.tracks[0].lane(target));
}
