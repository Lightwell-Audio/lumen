//! A plugin's parameters on its card (M6 slice 4): the strip's knobs and the
//! panel with every parameter, driven as a person does it. The plugin is the
//! test plugin, with three hundred extra parameters in groups, some hidden
//! and some read only (the module named `Many`).

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use lumen_core::time::Ticks;
use lumen_plugin_catalog::catalog::class_id;
use lumen_project::model::{Color, Device, DeviceKind, Id, Note, Project, Track, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
const AUDIO: Id = Id(10);
const FX: Id = Id(20);
const EFFECT: &str = "Deterministic Test Effect";
const GAIN: u32 = 0;

fn project() -> Project {
    let keys = midi_track(
        KEYS.0,
        "Keys",
        "poly",
        (0, 64),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
        vec![],
    );
    let mut audio = Track::new(AUDIO, "Audio".to_owned(), TrackKind::Audio, Color(0x4F81BD));
    audio.devices = vec![Device::vst3(
        FX,
        &class_id(&lumen_test_plugin::EFFECT.map(|c| c as u8)),
        EFFECT,
        "Lumen",
        DeviceKind::Effect,
    )];
    project_of("Parameters", vec![keys, audio])
}

fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        "Many" => Some("many_params=300"),
        _ => None,
    }
}

/// Lumen with the effect on the audio track, running, and its Devices tab
/// showing.
fn launch(name: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-parameters-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), &["Many"]);
    let cache = dir.path().join("plugin-cache.json");
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
    });
    lumen.wait_for("the project", |app| app.project().tracks.len() == 2);
    lumen.wait_for("the plugin scan", |app| !app.plugins_busy());
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.wait_for("the plugin to run", |app| {
        app.device_state(FX).is_some_and(|s| s.starts_with("ready"))
    });
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(20);
    lumen
}

/// Whether the panel with every parameter is open (its title is on the
/// window and in it).
fn panel_open(lumen: &Lumen) -> bool {
    lumen
        .harness
        .query_all_by_label("Deterministic Test Effect parameters")
        .next()
        .is_some()
}

/// The centre of the control called `label` (a name starts with it).
#[track_caller]
fn at(lumen: &Lumen, label: &str) -> Pos2 {
    lumen.harness.get_by_label_contains(label).rect().center()
}

fn knobs(lumen: &Lumen) -> usize {
    lumen
        .harness
        .query_all_by_label_contains(", parameter: ")
        .count()
}

#[test]
fn the_card_shows_eight_knobs_named_with_the_plugins_own_words_and_a_button_for_all() {
    let mut lumen = launch("strip");
    lumen.wait_for("the knobs", |_| true);
    lumen.frames(10);
    assert_eq!(knobs(&lumen), 8);
    // The first parameters a person would move: not the hidden, not the
    // read-only ones.
    let ids = lumen.app().strip_parameters(FX);
    assert_eq!(ids.len(), 8);
    assert_eq!(&ids[..2], &[0, 1], "the plugin's own two come first");
    for id in &ids {
        let k = id.checked_sub(100);
        assert!(
            k.is_none_or(|k| k % 7 != 3 && k % 11 != 5),
            "{id} is hidden or read only"
        );
    }
    // The plugin's own text, once it has answered: 0.0 dB at the default.
    lumen.wait_for("the plugin's text", |_| true);
    for _ in 0..40 {
        lumen.frames(2);
        if lumen.shows("Gain, parameter: 0.0") {
            break;
        }
    }
    assert!(
        lumen.shows("Gain, parameter: 0.0"),
        "the gain knob should read in the plugin's words"
    );
    // The hidden ones are not counted.
    let all = lumen
        .harness
        .get_by_label_contains("All parameters (")
        .accesskit_node()
        .label()
        .unwrap_or_default()
        .to_string();
    assert!(all.contains("(259)"), "{all}");
}

#[test]
fn dragging_a_knob_sets_the_value_as_one_undo_step_and_double_click_resets_it() {
    let mut lumen = launch("drag");
    let knob = at(&lumen, "Gain, parameter");
    assert!(lumen.app().device_params(FX).is_empty());
    lumen.drag(knob, knob + vec2(0.0, -48.0), Modifiers::NONE);
    let held = lumen.app().device_params(FX);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].0, GAIN);
    assert!(held[0].1 > 0.9, "dragged up: {}", held[0].1);
    assert_eq!(lumen.app().undo_label(), Some("Change Gain"));

    // One undo takes the whole drag back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(lumen.app().device_params(FX).is_empty());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(lumen.app().device_params(FX).len(), 1);

    // A double-click sets the plugin's default.
    let knob = at(&lumen, "Gain, parameter");
    lumen.double_click(knob);
    let held = lumen.app().device_params(FX);
    assert!(
        (held[0].1 - 0.8333).abs() < 0.001,
        "the default gain: {held:?}"
    );
}

/// A lane drives the gain, the song plays, the lane is let go of (`remove`
/// takes it away, else it is switched off) and the knob is dragged.
fn lane_then_drag(name: &str, remove: bool) {
    use lumen_core::automation::{Lane, Point, Shape};
    use lumen_project::automation::Target;
    let mut lumen = launch(name);
    let target = Target::Device {
        device: FX,
        param: GAIN,
    };
    let lane = Lane::new(vec![
        Point::new(Ticks::ZERO, 0.2).with_shape(Shape::Step),
        Point::new(Ticks::quarters(1), 0.4).with_shape(Shape::Step),
    ])
    .unwrap();
    lumen.app_mut().give_lane(AUDIO, target, lane).unwrap();
    lumen.frames(30);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(2));
    if remove {
        lumen.app_mut().take_lane_away(AUDIO, target).unwrap();
    } else {
        lumen.app_mut().switch_lane(AUDIO, target, false).unwrap();
    }
    lumen.frames(60);
    let knob = at(&lumen, "Gain, parameter");
    lumen.drag(knob, knob + vec2(0.0, -48.0), Modifiers::NONE);
    let held = lumen.app().device_params(FX);
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].1 > 0.9, "dragged up: {}", held[0].1);
    lumen.frames(60);
    let known = lumen.app().device_param_known(FX, GAIN);
    assert!(
        known.is_some_and(|v| v > 0.9),
        "the plugin has the dragged value: {known:?}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_knob_can_be_dragged_again_once_its_lane_is_switched_off() {
    lane_then_drag("lane-off-drag", false);
}

#[test]
fn a_knob_can_be_dragged_again_once_its_lane_is_removed() {
    lane_then_drag("lane-removed-drag", true);
}

/// Types `text` into the focused knob of `label` and confirms with Enter.
#[track_caller]
fn type_value(lumen: &mut Lumen, label: &str, text: &str) {
    lumen.harness.get_by_label_contains(label).click();
    lumen.frames(3);
    lumen.harness.event(Event::Text(text.into()));
    lumen.frames(4);
    lumen.key(Modifiers::NONE, Key::Enter);
}

#[test]
fn a_value_typed_on_a_knob_is_read_by_the_plugin_and_is_one_undo_step() {
    let mut lumen = launch("typed");
    assert!(lumen.app().device_params(FX).is_empty());
    // The test plugin reads a number as decibels: -6 dB of -60 to +12.
    type_value(&mut lumen, "Gain, parameter", "-6");
    lumen.wait_for("the plugin to read the text", |app| {
        !app.device_params(FX).is_empty()
    });
    let held = lumen.app().device_params(FX);
    assert_eq!(held[0].0, GAIN);
    assert!((held[0].1 - 0.75).abs() < 1e-6, "{held:?}");
    assert_eq!(lumen.app().undo_label(), Some("Change Gain"));
    // The knob reads it in the plugin's words.
    for _ in 0..40 {
        lumen.frames(2);
        if lumen.shows("Gain, parameter: -6.0") {
            break;
        }
    }
    assert!(lumen.shows("Gain, parameter: -6.0"));
    // One undo takes it back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    assert!(lumen.app().device_params(FX).is_empty());
}

#[test]
fn a_value_typed_on_a_panel_bar_reaches_the_plugin_too() {
    let mut lumen = launch("typed-panel");
    lumen.click("All parameters (259)");
    lumen.frames(10);
    // The panel is the later window: its bar is the last of the two.
    let bar = lumen
        .harness
        .get_all_by_label_contains("Gain, parameter")
        .last()
        .expect("the panel's bar")
        .rect()
        .center();
    lumen
        .harness
        .get_all_by_label_contains("Gain, parameter")
        .last()
        .unwrap()
        .click();
    lumen.frames(3);
    lumen.harness.event(Event::Text("-12".into()));
    lumen.frames(4);
    assert!(
        lumen.shows("Gain: type a value"),
        "the field is up at {bar:?}"
    );
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the plugin to read the text", |app| {
        !app.device_params(FX).is_empty()
    });
    let held = lumen.app().device_params(FX);
    assert!((held[0].1 - 0.6667).abs() < 0.001, "{held:?}");
}

#[test]
fn text_the_plugin_does_not_understand_changes_nothing_and_says_so() {
    let mut lumen = launch("typed-refused");
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    // Enter opens the field with the plugin's own text; replace it.
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("loud");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the plugin's answer", |app| {
        app.note().is_some_and(|n| n.contains("did not understand"))
    });
    assert_eq!(
        lumen.app().note(),
        Some("The plugin did not understand \u{201c}loud\u{201d}.")
    );
    assert!(lumen.app().device_params(FX).is_empty());
    assert_eq!(lumen.app().undo_label(), None);
}

#[test]
fn the_knobs_menu_types_a_value_and_escape_leaves_the_value_alone() {
    let mut lumen = launch("typed-menu");
    lumen.right_click(at(&lumen, "Gain, parameter"));
    lumen.click("Type a value…");
    lumen.frames(4);
    assert!(lumen.shows("Gain: type a value"));
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("3");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(6);
    assert!(!lumen.shows("Gain: type a value"));
    assert!(lumen.app().device_params(FX).is_empty());
}

#[test]
fn the_panel_lists_every_parameter_quickly_narrows_by_search_and_stars_onto_the_strip() {
    let mut lumen = launch("panel");
    lumen.click("All parameters (259)");
    lumen.frames(10);
    assert!(panel_open(&lumen));
    // Hundreds of parameters, and only the rows in view are made.
    let rows = lumen
        .harness
        .query_all_by_label_contains("the strip")
        .count();
    assert!(rows > 3 && rows < 30, "{rows} rows drawn");

    // Take one off in the panel, then star another one found by search.
    assert_eq!(lumen.app().strip_parameters(FX)[0], 0);
    lumen.click("Take Gain off the strip");
    lumen.frames(3);
    assert_eq!(lumen.app().strip_parameters(FX).len(), 7);
    lumen.click("Search parameters");
    lumen
        .harness
        .get_by_label("Search parameters")
        .type_text("param 8");
    lumen.frames(8);
    lumen.click("Put Param 8 on the strip");
    lumen.frames(3);
    assert!(lumen.app().strip_parameters(FX).contains(&108));
    // The choice is saved with the project's view.
    let saved = lumen.app().project_as_saved();
    assert!(
        saved
            .view
            .shown_params
            .get(&FX)
            .is_some_and(|s| s.contains(&108))
    );

    // The strip is full again, so another star is refused with the reason.
    lumen.click("Search parameters");
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen
        .harness
        .get_by_label("Search parameters")
        .type_text("param 9");
    lumen.frames(8);
    lumen.click("Put Param 9 on the strip");
    lumen.frames(3);
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("holds eight")),
        "{:?}",
        lumen.app().note()
    );
    assert!(!lumen.app().strip_parameters(FX).contains(&109));
}

#[test]
fn the_search_finds_parameters_by_name_and_group_words() {
    let mut lumen = launch("search");
    lumen.click("All parameters (259)");
    lumen.frames(10);
    // Typed into the search field, which has the keyboard when the panel opens.
    lumen
        .harness
        .get_by_label("Search parameters")
        .type_text("group 2 param 5");
    lumen.frames(10);
    // Param 5 is read only (5 % 11 == 5) and its group is (5 % 4) + 1 = 2.
    assert!(
        lumen.shows("Param 5, parameter: 50.0 %"),
        "the bar for Param 5 (read only, still listed)"
    );
    // Nothing else in the first screen matches both words except the
    // parameters of the same group whose names contain "param 5".
    assert!(
        lumen
            .harness
            .query_by_label("Put Param 8 on the strip")
            .is_none(),
        "a parameter of another group is not listed"
    );
    assert!(
        lumen
            .harness
            .query_by_label("Put Param 5 on the strip")
            .is_some()
    );
}

#[test]
fn a_knob_leaves_the_strip_from_its_menu_and_an_empty_slot_offers_the_panel() {
    let mut lumen = launch("remove");
    let knob = at(&lumen, "Latency, parameter");
    lumen.right_click(knob);
    lumen.click("Remove from strip");
    lumen.frames(5);
    assert_eq!(knobs(&lumen), 7);
    assert_eq!(lumen.app().strip_parameters(FX).len(), 7);
    // An empty slot is a button that opens the panel.
    lumen.click("Choose a parameter for this slot");
    lumen.frames(5);
    assert!(panel_open(&lumen));
    assert!(lumen.shows("Star a parameter to put it on the card."));
}

#[test]
fn the_command_opens_the_panel_for_the_chosen_card() {
    let mut lumen = launch("command");
    lumen.app_mut().choose_card(FX);
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text("device parameters".into()));
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(panel_open(&lumen));
}
