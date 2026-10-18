//! A plugin's own window and Learn (M6 slice 4f), driven as a person does it.
//! The host's stand-in window (`headless_editor` in a module's faults) records
//! where a window would be, so the whole path from the card's button to the
//! host and back is exercised without a window system; the real Windows
//! window is MT-VST-4.

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_plugin_catalog::catalog::class_id;
use lumen_project::model::{Device, DeviceKind, Id, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
const PADS: Id = Id(7);
const PLUGIN: Id = Id(20);
const TRIGGER: u32 = 90;
const WAVE: u32 = 1;

fn project() -> Project {
    let mut track = midi_track(
        KEYS.0,
        "Keys",
        "poly",
        (0, 64),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
        vec![],
    );
    track.devices = vec![Device::vst3(
        PLUGIN,
        &class_id(&lumen_test_plugin::INSTRUMENT.map(|c| c as u8)),
        "Deterministic Test Instrument",
        "Lumen",
        DeviceKind::Instrument,
    )];
    // A second track with nothing on it, to go to and come back from.
    let pads = midi_track(PADS.0, "Pads", "poly", (0, 64), None, vec![], vec![]);
    project_of("Editors", vec![track, pads])
}

fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        // A window that is only a record, and a plugin that edits its wave
        // when its trigger is set, as a control moved in its window would.
        "Window" => Some("window_edits,many_params=20,headless_editor"),
        // Opening the window blocks the host's main thread for a while.
        "Hang" => Some("headless_editor,headless_hang=2500"),
        // The user closes the window a moment after it opens.
        "Gone" => Some("headless_editor,headless_close=400"),
        // The user presses A in the window once its host takes A.
        "Press" => Some("headless_editor,headless_press=65"),
        _ => None,
    }
}

/// Lumen with the instrument on a track, running, and its Devices tab showing.
fn launch(name: &str, module: &str) -> Lumen {
    let dir = TempDir::new(&format!("e2e-editors-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project(), &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), &[module]);
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
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.wait_for("the plugin to run", |app| {
        app.device_state(PLUGIN)
            .is_some_and(|s| s.starts_with("ready"))
    });
    lumen
        .harness
        .get_all_by_label("Devices tab")
        .last()
        .expect("the detail panel's tabs")
        .click();
    lumen.frames(20);
    lumen.app_mut().choose_card(PLUGIN);
    lumen
}

/// Takes the knob called `name` off the strip, by its menu.
#[track_caller]
fn take_off(lumen: &mut Lumen, name: &str) {
    // A knob is named "<name>, parameter: <text>", a switch by its name.
    let at = match lumen
        .harness
        .query_by_label_contains(&format!("{name}, parameter"))
    {
        Some(knob) => knob.rect().center(),
        None => lumen.harness.get_by_label(name).rect().center(),
    };
    lumen.right_click(at);
    lumen.click("Remove from strip");
    lumen.frames(4);
}

/// Runs the command the palette finds for `words`.
fn palette(lumen: &mut Lumen, words: &str) {
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.key(Modifiers::COMMAND, Key::A);
    lumen.harness.event(Event::Text(words.into()));
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
}

/// Frames until a control called `label` is on screen.
#[track_caller]
fn wait_shows(lumen: &mut Lumen, label: &str) {
    for _ in 0..200 {
        if lumen.shows(label) {
            return;
        }
        lumen.frames(3);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("`{label}` never appeared");
}

#[test]
fn the_editor_button_opens_the_window_where_it_was_and_closes_it() {
    let mut lumen = launch("open", "Window");
    // Left at a place on an earlier day: it opens there, not where the
    // system would put it.
    lumen
        .app_mut()
        .set_editor_place(PLUGIN, (321, 123, 640, 480));
    assert!(lumen.shows("Open editor"));
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    assert_eq!(lumen.app().editor_place(PLUGIN), Some((321, 123, 640, 480)));
    lumen.click("Close editor");
    wait_shows(&mut lumen, "Open editor");
    assert_eq!(lumen.app().editor_place(PLUGIN), Some((321, 123, 640, 480)));
}

#[test]
fn a_window_goes_when_another_track_is_chosen_and_comes_back_with_its_own() {
    let mut lumen = launch("tracks", "Window");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    assert!(!lumen.app().editor_hidden(PLUGIN));
    // On the other track the window is hidden, not closed.
    lumen.app_mut().select_tracks(&[PADS]);
    lumen.wait_for("the window to be hidden", |app| app.editor_hidden(PLUGIN));
    // Back on its own track it is shown again, and the card still reports it
    // open: nothing was lost.
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.wait_for("the window to be shown", |app| !app.editor_hidden(PLUGIN));
    lumen.app_mut().choose_card(PLUGIN);
    wait_shows(&mut lumen, "Close editor");
}

#[test]
fn learn_ends_when_its_window_goes_with_the_track() {
    let mut lumen = launch("learn-tracks", "Window");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    lumen.click("Learn");
    lumen.frames(4);
    lumen.app_mut().select_tracks(&[PADS]);
    lumen.wait_for("the window to be hidden", |app| app.editor_hidden(PLUGIN));
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.wait_for("the window to be shown", |app| !app.editor_hidden(PLUGIN));
    lumen.app_mut().choose_card(PLUGIN);
    lumen.frames(10);
    assert!(
        !lumen.shows("Stop learning"),
        "Learn does not come back by itself"
    );
}

#[test]
fn a_window_first_opened_is_remembered_for_next_time() {
    let mut lumen = launch("first", "Window");
    assert_eq!(lumen.app().editor_place(PLUGIN), None);
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    lumen.wait_for("its place to be remembered", |app| {
        app.editor_place(PLUGIN).is_some()
    });
    // The stand-in's own default place and size.
    assert_eq!(lumen.app().editor_place(PLUGIN), Some((100, 100, 320, 200)));
}

#[test]
fn a_window_the_user_closes_reads_as_closed_again() {
    let mut lumen = launch("gone", "Gone");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    wait_shows(&mut lumen, "Open editor");
}

#[test]
fn a_plugin_that_stops_answering_is_flagged_and_ended_only_when_the_user_says() {
    let mut lumen = launch("hang", "Hang");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Editor not responding");
    assert!(lumen.shows("End plugin"));
    // It is never ended on its own: the window code finishes after a while
    // and the button goes on as before.
    wait_shows(&mut lumen, "Close editor");
    assert!(!lumen.shows("End plugin"));
}

/// The owner's report (MT-VST-4): with the plugin's window in front, the
/// computer keyboard played nothing, so changes could not be heard without
/// going back to Lumen. With the mode on, its keys pressed in the window play.
#[test]
fn the_computer_keyboard_plays_from_the_plugins_window() {
    let mut lumen = launch("press", "Press");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "Close editor");
    // Off: the host is asked for no keys, and the window keeps them.
    lumen.frames(20);
    assert!(lumen.app_mut().computer_keyboard_sent().is_empty());
    lumen.key(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
    assert!(lumen.app().computer_keyboard_on());
    let mut sent = Vec::new();
    for _ in 0..300 {
        sent.extend(lumen.app_mut().computer_keyboard_sent());
        if sent.len() >= 2 {
            break;
        }
        lumen.frames(2);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        sent,
        [
            lumen_core::midi::Msg::note_on(0, 60, 100),
            lumen_core::midi::Msg::note_off(0, 60)
        ],
        "A pressed and let go of in the window"
    );
    assert!(lumen.app().computer_keyboard_held().is_empty());
}

#[test]
fn ending_a_stuck_plugin_shows_the_card_as_stopped_with_reload() {
    let mut lumen = launch("end", "Hang");
    lumen.click("Open editor");
    wait_shows(&mut lumen, "End plugin");
    lumen.click("End plugin");
    lumen.wait_for("the plugin to be failed", |app| {
        app.device_state(PLUGIN)
            .is_some_and(|s| s.starts_with("failed"))
    });
    wait_shows(&mut lumen, "Reload");
}

#[test]
fn learn_puts_the_next_control_touched_in_the_window_on_the_strip() {
    let mut lumen = launch("learn", "Window");
    // Take the wave off the strip, so touching it has something to do.
    assert!(lumen.app().strip_parameters(PLUGIN).contains(&WAVE));
    take_off(&mut lumen, "Wave");
    // And a slot more, so the strip still has room once the wave is on it.
    take_off(&mut lumen, "Level");
    assert_eq!(lumen.app().strip_parameters(PLUGIN).len(), 6);
    assert!(!lumen.app().strip_parameters(PLUGIN).contains(&WAVE));

    lumen.click("Learn");
    wait_shows(&mut lumen, "Stop learning");
    wait_shows(&mut lumen, "Close editor");
    assert!(lumen.app().learning(PLUGIN));
    // A control touched in the plugin's window: its trigger makes it edit
    // its wave as a gesture.
    lumen.app_mut().poke_plugin(PLUGIN, TRIGGER, 1.0);
    lumen.wait_for("the wave to join the strip", |app| {
        app.strip_parameters(PLUGIN).contains(&WAVE)
    });
    let ids = lumen.app().strip_parameters(PLUGIN);
    assert_eq!(*ids.last().unwrap(), WAVE, "it takes the next free slot");
    // A second click ends it.
    lumen.click("Stop learning");
    lumen.frames(3);
    assert!(!lumen.app().learning(PLUGIN));
}

#[test]
fn learn_ends_when_escape_is_pressed_or_the_window_closes() {
    let mut lumen = launch("learn-end", "Window");
    take_off(&mut lumen, "Wave");
    lumen.click("Learn");
    wait_shows(&mut lumen, "Stop learning");
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(4);
    assert!(!lumen.app().learning(PLUGIN));

    lumen.click("Learn");
    wait_shows(&mut lumen, "Stop learning");
    lumen.click("Close editor");
    wait_shows(&mut lumen, "Open editor");
    assert!(!lumen.app().learning(PLUGIN));
}

#[test]
fn the_editor_command_opens_and_closes_the_chosen_plugins_window() {
    let mut lumen = launch("command", "Window");
    palette(&mut lumen, "plugin's own window");
    wait_shows(&mut lumen, "Close editor");
    palette(&mut lumen, "plugin's own window");
    wait_shows(&mut lumen, "Open editor");
}

#[test]
fn learn_will_not_start_with_no_room_and_ends_when_the_strip_fills() {
    let mut lumen = launch("learn-full", "Window");
    // The strip starts full: Learn says so and stays off.
    assert_eq!(lumen.app().strip_parameters(PLUGIN).len(), 8);
    lumen.click("Learn");
    lumen.frames(4);
    assert!(!lumen.app().learning(PLUGIN));
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("strip is full")),
        "{:?}",
        lumen.app().note()
    );
    // With one slot free, the first control touched fills it and ends Learn.
    take_off(&mut lumen, "Wave");
    lumen.click("Learn");
    wait_shows(&mut lumen, "Stop learning");
    lumen.app_mut().poke_plugin(PLUGIN, TRIGGER, 1.0);
    lumen.wait_for("the strip to fill", |app| {
        app.strip_parameters(PLUGIN).len() == 8
    });
    lumen.frames(5);
    assert!(!lumen.app().learning(PLUGIN));
}

#[cfg(not(windows))]
#[test]
fn where_there_are_no_windows_the_button_says_so_and_learn_hides() {
    let mut lumen = launch("none", "Plain");
    assert!(lumen.shows("Open editor"));
    lumen.click("Open editor");
    wait_shows(&mut lumen, "No editor");
    assert!(!lumen.shows("Learn"));
    assert!(lumen.shows("All parameters (2)"));
}

/// Whether the track holds a hosted plugin (a new MIDI track has the
/// built-in synth, which the plugin replaces).
fn has_plugin(lumen: &Lumen, track: Id) -> bool {
    lumen
        .app()
        .project()
        .tracks
        .iter()
        .find(|t| t.id == track)
        .is_some_and(|t| {
            t.devices
                .iter()
                .any(|d| matches!(d.plugin, lumen_project::model::PluginRef::Vst3 { .. }))
        })
}

#[test]
fn a_device_dragged_from_the_browser_onto_a_track_header_shows_the_track_and_opens_its_window() {
    let mut lumen = launch("drop-header", "Window");
    assert!(!has_plugin(&lumen, PADS));
    let row = lumen
        .harness
        .get_by_label_contains("Deterministic Test Instrument, ")
        .rect()
        .center();
    let pads = lumen.harness.get_by_label_contains("Track Pads").rect();
    lumen.drag(row, pads.center(), Modifiers::NONE);

    assert!(has_plugin(&lumen, PADS), "the device is on the track");
    assert!(lumen.shows("Deterministic Test Instrument on Pads"));
    wait_shows(&mut lumen, "Close editor");
    // One undo step put it there, and Undo takes it away again.
    assert_eq!(
        lumen.app().undo_label(),
        Some("Replace instrument with Deterministic Test Instrument")
    );
}

#[test]
fn a_device_dragged_onto_a_tracks_lane_does_the_same() {
    let mut lumen = launch("drop-lane", "Window");
    let row = lumen
        .harness
        .get_by_label_contains("Deterministic Test Instrument, ")
        .rect()
        .center();
    let pads = lumen.harness.get_by_label_contains("Track Pads").rect();
    lumen.drag(
        row,
        eframe::egui::pos2(pads.right() + 300.0, pads.center().y),
        Modifiers::NONE,
    );
    assert!(has_plugin(&lumen, PADS));
    wait_shows(&mut lumen, "Close editor");
}

#[test]
fn a_drag_that_ends_on_no_track_adds_nothing_and_opens_nothing() {
    let mut lumen = launch("drop-nowhere", "Window");
    let row = lumen
        .harness
        .get_by_label_contains("Deterministic Test Instrument, ")
        .rect()
        .center();
    lumen.drag(row, eframe::egui::pos2(row.x, row.y + 3.0), Modifiers::NONE);
    assert!(!has_plugin(&lumen, PADS));
    lumen.frames(20);
    assert!(!lumen.shows("Close editor"));
}

#[test]
fn a_value_changed_in_the_plugins_window_is_never_sent_back_to_it() {
    // The old value must not go back to the plugin in the moment before the
    // project holds the new one: the control would jump back and forth.
    let mut lumen = launch("no-snap-back", "Window");
    assert_eq!(lumen.app().values_sent_to(PLUGIN), 0);
    lumen.app_mut().poke_plugin(PLUGIN, TRIGGER, 1.0);
    lumen.wait_for("the wave to reach the project", |app| {
        app.device_params(PLUGIN).iter().any(|&(id, _)| id == WAVE)
    });
    lumen.frames(30);
    assert_eq!(
        lumen.app().values_sent_to(PLUGIN),
        0,
        "the plugin's own edit was sent back to it"
    );
}
