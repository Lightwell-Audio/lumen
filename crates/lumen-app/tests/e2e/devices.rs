//! The Devices tab and adding devices (M6 slice 2d): double-clicking a
//! device in the browser puts it on the selected track (or makes one), a
//! drag shows where it will land, the power button and the commands bypass
//! and remove, and a card that is failed or missing says so and offers a way
//! on. The plugins and the host are the real ones (the test plugin).

use eframe::egui::{Key, Modifiers, pos2};
use egui_kittest::kittest::Queryable;
use lumen_core::time::Ticks;
use lumen_plugin_catalog::catalog::class_id;
use lumen_project::model::{Device, DeviceKind, Id, Note, PluginRef, Project, TrackKind};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;
use super::plugins::{built, plugin_root};

const KEYS: Id = Id(1);
const AUDIO: Id = Id(10);
const FIRST: Id = Id(20);
const INSTRUMENT: &str = "Deterministic Test Instrument";
const EFFECT: &str = "Deterministic Test Effect";

fn effect_class() -> String {
    class_id(&lumen_test_plugin::EFFECT.map(|c| c as u8))
}

fn effect_device(id: Id) -> Device {
    Device::vst3(id, &effect_class(), EFFECT, "Lumen", DeviceKind::Effect)
}

/// A MIDI track "Keys" with the synth and a long note, and an empty audio
/// track "Audio", with `on_audio` on the audio track's chain.
fn project(on_audio: Vec<Device>) -> Project {
    let keys = midi_track(
        KEYS.0,
        "Keys",
        "poly",
        (0, 64),
        None,
        vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
        vec![],
    );
    let mut audio = lumen_project::model::Track::new(
        AUDIO,
        "Audio".to_owned(),
        TrackKind::Audio,
        lumen_project::model::Color(0x4F81BD),
    );
    audio.devices = on_audio;
    project_of("Chain", vec![keys, audio])
}

/// The fault a module's host is given, by the module's name: "Dies" crashes
/// on its 40th block, as in `hosting.rs`.
fn faults(stem: &str) -> Option<&'static str> {
    match stem {
        "Dies" => Some("process_crash@40"),
        _ => None,
    }
}

/// Lumen with `project` open and the test plugins installed.
fn launch(name: &str, project: &Project, modules: &[&str]) -> Lumen {
    let dir = TempDir::new(&format!("e2e-devices-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), modules);
    let cache = dir.path().join("plugin-cache.json");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
    });
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.wait_for("the plugin scan", |app| !app.plugins_busy());
    lumen.frames(30);
    lumen
}

impl Lumen {
    /// Double-clicks the browser row of the device named `name`.
    #[track_caller]
    fn choose_in_browser(&mut self, name: &str) {
        let at = self
            .harness
            .get_by_label_contains(&format!("{name}, "))
            .rect()
            .center();
        self.double_click(at);
        self.frames(10);
    }

    /// Opens the detail panel's Devices tab (the browser has one too: the
    /// panel's comes last).
    #[track_caller]
    fn show_devices_tab(&mut self) {
        self.harness
            .get_all_by_label("Devices tab")
            .last()
            .expect("the detail panel's tabs")
            .click();
        self.frames(10);
    }

    fn devices_on(&self, track: Id) -> Vec<String> {
        self.app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map(|t| {
                t.devices
                    .iter()
                    .map(|d| match &d.plugin {
                        PluginRef::Vst3 { name, .. } => name.clone(),
                        PluginRef::Builtin { name } => name.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[test]
fn double_clicking_an_effect_adds_it_to_the_selected_track_in_one_undo_step() {
    let mut lumen = launch("effect", &project(vec![]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.frames(3);
    lumen.choose_in_browser(EFFECT);
    assert_eq!(lumen.devices_on(AUDIO), [EFFECT]);
    assert_eq!(
        lumen.app().undo_label(),
        Some("Add Deterministic Test Effect")
    );
    // The Devices tab shows it, chosen, on the track it is on.
    assert!(lumen.shows(&format!("{EFFECT} on Audio")), "its card");
    assert_eq!(
        lumen.app().chosen_card(),
        lumen.app().project().tracks[1]
            .devices
            .first()
            .map(|d| d.id)
    );
    // Undo is one step and takes the card away.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert!(lumen.devices_on(AUDIO).is_empty());
    assert!(!lumen.shows(&format!("{EFFECT} on Audio")));
}

#[test]
fn an_effect_goes_after_the_chosen_card() {
    let mut lumen = launch(
        "after",
        &project(vec![effect_device(FIRST), effect_device(Id(21))]),
        &["Plug"],
    );
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    // Choose the first card, then add: it lands between the two.
    lumen.click(&format!("{EFFECT} on Audio"));
    assert_eq!(lumen.app().chosen_card(), Some(FIRST));
    lumen.choose_in_browser(EFFECT);
    let ids: Vec<Id> = lumen.app().project().tracks[1]
        .devices
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], FIRST);
    assert_eq!(ids[2], Id(21));
}

#[test]
fn an_instrument_with_no_midi_track_selected_makes_one_as_a_single_step() {
    let mut lumen = launch("new-track", &project(vec![]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.frames(3);
    let before = lumen.app().project().tracks.len();
    lumen.choose_in_browser(INSTRUMENT);
    let project = lumen.app().project();
    assert_eq!(project.tracks.len(), before + 1);
    let made = project
        .tracks
        .iter()
        .find(|t| t.devices.iter().any(|d| d.is_vst3()))
        .expect("the new track");
    assert_eq!(made.kind, TrackKind::Midi);
    assert_eq!(made.devices.len(), 1, "the plugin replaces the synth");
    assert_eq!(lumen.app().selected_tracks(), [made.id]);
    // One undo takes the track and its device away.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(lumen.app().project().tracks.len(), before);
}

#[test]
fn an_instrument_replaces_the_synth_on_the_selected_midi_track() {
    let mut lumen = launch("replace", &project(vec![]), &["Plug"]);
    lumen.app_mut().select_tracks(&[KEYS]);
    lumen.frames(3);
    assert_eq!(lumen.devices_on(KEYS), ["synth"]);
    lumen.choose_in_browser(INSTRUMENT);
    assert_eq!(lumen.devices_on(KEYS), [INSTRUMENT]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(
        lumen.devices_on(KEYS),
        ["synth"],
        "undo brings the synth back"
    );
}

#[test]
fn the_power_button_and_the_commands_bypass_and_remove_the_chosen_device() {
    let mut lumen = launch("power", &project(vec![effect_device(FIRST)]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    let bypassed = |l: &Lumen| l.app().project().tracks[1].devices[0].bypass;
    assert!(!bypassed(&lumen));
    assert!(lumen.is_on(&format!("Power {EFFECT}")));
    lumen.click(&format!("Power {EFFECT}"));
    assert!(bypassed(&lumen), "the power button bypasses");
    assert!(!lumen.is_on(&format!("Power {EFFECT}")));
    lumen.click(&format!("Power {EFFECT}"));
    assert!(!bypassed(&lumen), "and turns it on again");
    assert_eq!(
        lumen.app().chosen_card(),
        Some(FIRST),
        "it chooses the card"
    );

    // Delete, with the chain holding the keyboard, removes the chosen one.
    lumen.click(&format!("{EFFECT} on Audio"));
    lumen.key(Modifiers::NONE, Key::Delete);
    lumen.frames(5);
    assert!(lumen.devices_on(AUDIO).is_empty());
    assert_eq!(lumen.app().chosen_card(), None);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(lumen.devices_on(AUDIO), [EFFECT], "undo puts it back");
}

#[test]
fn dragging_a_device_from_the_browser_drops_it_where_the_line_shows() {
    let mut lumen = launch("drag", &project(vec![effect_device(FIRST)]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    let row = lumen
        .harness
        .get_by_label_contains(&format!("{EFFECT}, "))
        .rect()
        .center();
    let card = lumen
        .harness
        .get_by_label(&format!("{EFFECT} on Audio"))
        .rect();

    // Right of the card: after it.
    lumen.drag(
        row,
        pos2(card.right() + 15.0, card.center().y),
        Modifiers::NONE,
    );
    let ids: Vec<Id> = lumen.app().project().tracks[1]
        .devices
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids.len(), 2, "one effect added");
    assert_eq!(ids[0], FIRST, "after the card it was dropped beside");

    // Left of the first card: before it.
    let card = lumen
        .harness
        .get_all_by_label(&format!("{EFFECT} on Audio"))
        .next()
        .expect("the first card")
        .rect();
    lumen.drag(
        row,
        pos2(card.left() - 15.0, card.center().y),
        Modifiers::NONE,
    );
    let ids: Vec<Id> = lumen.app().project().tracks[1]
        .devices
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[1], FIRST, "the new one is first now");
}

#[test]
fn a_drag_that_ends_outside_the_chain_adds_nothing() {
    let mut lumen = launch("drag-away", &project(vec![]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    let row = lumen
        .harness
        .get_by_label_contains(&format!("{EFFECT}, "))
        .rect()
        .center();
    lumen.drag(row, pos2(700.0, 120.0), Modifiers::NONE);
    assert!(lumen.devices_on(AUDIO).is_empty());
}

#[test]
fn a_missing_plugin_says_so_and_offers_a_rescan() {
    // The project names the test effect, but no module is installed.
    let mut lumen = launch("missing", &project(vec![effect_device(FIRST)]), &[]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    assert_eq!(lumen.app().device_state(FIRST).as_deref(), Some("missing"));
    assert!(lumen.shows("Rescan plugins"), "the card's button");
    lumen.click("Rescan plugins");
    lumen.wait_for("the rescan", |app| !app.plugins_busy());
    // Its settings are untouched, whatever it shows.
    assert_eq!(lumen.devices_on(AUDIO), [EFFECT]);
}

#[test]
fn a_failed_plugin_offers_reload_and_it_starts_again() {
    let mut lumen = launch("reload", &project(vec![effect_device(FIRST)]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    lumen.wait_for("the host", |app| {
        app.device_state(FIRST).is_some_and(|s| s == "ready")
    });
    lumen.app_mut().pretend_device_state(FIRST, "failed");
    lumen.frames(5);
    assert!(lumen.shows("Reload"), "the failed card's button");
    lumen.click("Reload");
    lumen.wait_for("the new host", |app| {
        app.device_state(FIRST).is_some_and(|s| s == "ready")
    });
}

#[test]
fn the_command_adds_the_device_selected_in_the_browser() {
    let mut lumen = launch("command", &project(vec![]), &["Plug"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.frames(3);
    // One click selects the row; the palette's command adds it.
    lumen
        .harness
        .get_by_label_contains(&format!("{EFFECT}, "))
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(eframe::egui::Event::Text(
        "Add device to selected track".into(),
    ));
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    assert_eq!(lumen.devices_on(AUDIO), [EFFECT]);
}

/// Starts the song and waits for the "Dies" plugin's host to die under it.
#[track_caller]
fn play_until_it_fails(lumen: &mut Lumen, device: Id) {
    lumen.wait_for("the host", move |app| {
        app.device_state(device).is_some_and(|s| s == "ready")
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("the failure", move |app| {
        app.device_state(device)
            .is_some_and(|s| s.starts_with("failed"))
    });
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
    lumen.frames(5);
}

#[test]
fn a_host_that_dies_under_the_song_shows_on_its_card_and_reload_brings_it_back() {
    let mut lumen = launch("dies", &project(vec![effect_device(FIRST)]), &["Dies"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    play_until_it_fails(&mut lumen, FIRST);
    assert!(lumen.shows("Reload"), "the failed card's button");
    // The card says what happened, not only that it did.
    let why = lumen.app().device_state(FIRST).unwrap_or_default();
    assert!(why.len() > "failed: ".len(), "a reason on the card: {why}");
    lumen.click("Reload");
    lumen.wait_for("the new host", |app| {
        app.device_state(FIRST).is_some_and(|s| s == "ready")
    });
    assert!(!lumen.shows("Reload"), "the button goes with the failure");
}

#[test]
fn removing_a_failed_device_and_undoing_it_loads_it_again_with_its_settings() {
    // The test plugin's state: its tag, its kind (1, an effect) and two
    // parameter values.
    let state = [
        b"LTP1".as_slice(),
        &[1],
        &0.25_f64.to_le_bytes(),
        &0.5_f64.to_le_bytes(),
    ]
    .concat();
    let mut device = effect_device(FIRST);
    device.state = state.clone();
    let mut lumen = launch("undo-failed", &project(vec![device]), &["Dies"]);
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    play_until_it_fails(&mut lumen, FIRST);
    lumen.app_mut().remove_device(AUDIO, FIRST);
    lumen.frames(5);
    assert!(lumen.devices_on(AUDIO).is_empty());
    assert_eq!(lumen.app().device_state(FIRST), None, "no host remains");
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(5);
    assert_eq!(lumen.devices_on(AUDIO), [EFFECT]);
    let kept = lumen.app().project().tracks[1].devices[0].state.clone();
    assert_eq!(kept, state, "the settings came back with it");
    // A new host, not the dead one.
    lumen.wait_for("a new host", |app| {
        app.device_state(FIRST).is_some_and(|s| s == "ready")
    });
}

#[test]
fn removing_a_device_while_it_is_loading_leaves_no_host_and_no_card() {
    let mut lumen = launch(
        "remove-loading",
        &project(vec![effect_device(FIRST)]),
        &["Plug"],
    );
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    lumen.app_mut().remove_device(AUDIO, FIRST);
    lumen.frames(60);
    assert!(lumen.devices_on(AUDIO).is_empty());
    assert_eq!(lumen.app().device_state(FIRST), None);
    assert!(
        lumen
            .harness
            .query_all_by_label_contains(&format!("Power {EFFECT}"))
            .next()
            .is_none()
    );
}

#[test]
fn every_control_on_every_card_face_has_a_name_and_tab_reaches_them() {
    use super::access::{focused, unnamed};

    let mut lumen = launch(
        "access",
        &project(vec![effect_device(FIRST), effect_device(Id(21))]),
        &["Plug"],
    );
    lumen.app_mut().select_tracks(&[AUDIO]);
    lumen.show_devices_tab();
    lumen.wait_for("both hosts", |app| {
        [FIRST, Id(21)]
            .iter()
            .all(|d| app.device_state(*d).is_some_and(|s| s == "ready"))
    });
    for face in ["ready", "loading", "failed", "missing"] {
        lumen.app_mut().pretend_device_state(FIRST, face);
        lumen.frames(5);
        let problems = unnamed(&lumen);
        assert!(problems.is_empty(), "{face}: {problems:#?}");
    }
    // A pretended "missing" makes the app look for the plugin again, which
    // it finds, so a host starts and its link arrives some time later and
    // would put the card back to ready. Let it arrive first.
    lumen.wait_for("the host that missing started", |app| {
        app.device_state(FIRST).is_some_and(|s| s == "ready")
    });
    // The failed card's button is a stop of its own, named for what it does.
    lumen.app_mut().pretend_device_state(FIRST, "failed");
    lumen.frames(5);
    let mut seen = Vec::new();
    for _ in 0..80 {
        lumen.key(Modifiers::NONE, Key::Tab);
        let focused = focused(&lumen);
        if let Some(name) = focused {
            seen.push(name);
        }
    }
    assert!(
        seen.iter().any(|n| n == "Reload"),
        "Tab never reached Reload (the card is {:?}): {seen:#?}",
        lumen.app().device_state(FIRST)
    );
    assert!(
        seen.iter().any(|n| n.starts_with("Power ")),
        "Tab never reached a power button: {seen:#?}"
    );
}
