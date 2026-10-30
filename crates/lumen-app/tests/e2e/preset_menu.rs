//! The card's preset menu (M6 slice 5f): a field with the name of the sound,
//! arrows to the previous and next, and a menu of the plugin's factory
//! programs and the preset files saved for it, with a search, Save, Save as…
//! and Show presets folder; a dot marks a changed sound. The plugins and the
//! host program are the real ones, built by the test; preset files are real
//! files in folders of the test's own.

use std::path::{Path, PathBuf};

use eframe::egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use lumen_hosting::Preset;
use lumen_project::model::Id;
use lumen_project::testing::TempDir;

use super::Lumen;
use super::hosting::{LEVEL, PLUGIN, faults, instrument_device, play, project, wait_for_state};
use super::plugins::{built, plugin_root};
use super::presets::{preset_for_instrument, state};

const KEYS: Id = Id(1);
const NAME: &str = "Deterministic Test Instrument";
const FIELD: &str = "Preset of Deterministic Test Instrument";
const PREVIOUS: &str = "Previous preset of Deterministic Test Instrument";
const NEXT: &str = "Next preset of Deterministic Test Instrument";

/// The folders of one plugin's presets in the user's place and the system's.
struct Places {
    user: PathBuf,
    system: PathBuf,
}

fn plugin_folder(root: &Path) -> PathBuf {
    root.join("Lumen").join(NAME)
}

fn put(folder: &Path, name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join(name), bytes).unwrap();
}

/// Lumen with the instrument on a track, its card showing, and system presets
/// "Quiet square" (good), "Broken" (not a preset) and "Bass/Warm" (good).
fn launch(name: &str) -> (Lumen, Places) {
    launch_with(name, "Presets")
}

/// As [`launch`], with the module of the test plugin named `module` installed.
fn launch_with(name: &str, module: &str) -> (Lumen, Places) {
    let dir = TempDir::new(&format!("e2e-preset-menu-{name}"));
    let places = Places {
        user: dir.path().join("user presets"),
        system: dir.path().join("system presets"),
    };
    let system = plugin_folder(&places.system);
    put(
        &system,
        "Quiet square.vstpreset",
        &preset_for_instrument(0.1, 0.0),
    );
    put(&system, "Broken.vstpreset", b"this is not a preset");
    put(
        &system.join("Bass"),
        "Warm.vstpreset",
        &preset_for_instrument(0.4, 1.0),
    );
    let project = project(|d| *d = vec![instrument_device(PLUGIN)]);
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).expect("save the test project");
    let root = plugin_root(dir.path(), &[module]);
    let cache = dir.path().join("plugin-cache.json");
    let (user, sys) = (places.user.clone(), places.system.clone());
    let mut lumen = Lumen::start_with(dir, move |launch| {
        launch
            .opening(path)
            .with_audio_buffer(1024)
            .with_plugins(vec![root], &built().1, cache)
            .with_plugin_faults(faults)
            .with_preset_folders(user, vec![sys])
    });
    lumen.wait_for("the project", |app| app.project().tracks.len() == 1);
    lumen.wait_for("the plugin scan", |app| !app.plugins_busy());
    lumen.app_mut().select_tracks(&[KEYS]);
    wait_for_state(&mut lumen, PLUGIN, "ready");
    lumen.wait_for("the plugin's programs", |app| {
        !app.device_programs(PLUGIN).is_empty()
    });
    lumen.show_whole_cards();
    lumen.wait_for("the card's field", |app| {
        app.device_has_preset_field(PLUGIN)
    });
    (lumen, places)
}

impl Lumen {
    fn open_preset_menu(&mut self) {
        self.click(FIELD);
        self.frames(5);
        assert!(self.shows("Search presets"), "the menu opened");
    }

    fn type_into_menu(&mut self, text: &str) {
        self.harness.event(Event::Text(text.to_owned()));
        self.frames(4);
    }
}

fn found(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in read.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(found(&path));
        } else if path.extension().is_some_and(|ext| ext == "vstpreset") {
            out.push(path);
        }
    }
    out
}

fn any_preset(root: &Path) -> bool {
    !found(root).is_empty()
}

fn saved_files(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|read| {
            read.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn the_field_shows_on_a_running_plugin_and_the_menu_lists_programs_and_files() {
    let (mut lumen, _places) = launch("list");
    assert!(lumen.shows(PREVIOUS) && lumen.shows(NEXT));
    assert_eq!(lumen.app().device_preset(PLUGIN), None, "none chosen yet");
    lumen.open_preset_menu();
    lumen.frames(20);
    for row in [
        "Square, quiet",
        "Saw, loud",
        "Saw, half",
        "Quiet square",
        "Broken",
        "Warm",
    ] {
        assert!(lumen.shows(row), "{row} is listed");
    }
    for action in ["Save", "Save as\u{2026}", "Show presets folder"] {
        assert!(lumen.shows(action), "{action} is offered");
    }

    // Every word of the search has to be in a name.
    lumen.type_into_menu("saw");
    assert!(lumen.shows("Saw, loud") && lumen.shows("Saw, half"));
    assert!(!lumen.shows("Square, quiet") && !lumen.shows("Warm"));
    lumen.type_into_menu(" half");
    assert!(lumen.shows("Saw, half") && !lumen.shows("Saw, loud"));
    lumen.type_into_menu(" nothing");
    assert!(!lumen.shows("Saw, half"));
}

#[test]
fn choosing_a_program_names_the_field_and_a_change_afterwards_marks_it() {
    let (mut lumen, _places) = launch("program");
    play(&mut lumen);
    lumen.open_preset_menu();
    lumen.click("Saw, loud");
    lumen.wait_for("the program to be loaded", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.9)
    });
    assert_eq!(lumen.app().undo_label(), Some("Load Saw, loud"));
    // The sound has settled once the program's answering edits are in.
    lumen.wait_for("the field to name it", |app| {
        app.device_preset(PLUGIN) == Some(("Saw, loud".to_owned(), false))
    });
    assert!(!lumen.shows("Search presets"), "choosing closes the menu");
    // A program's answering edits belong to it for a moment.
    std::thread::sleep(std::time::Duration::from_millis(1700));
    lumen.frames(5);

    // A control moved in the plugin's own window changes the sound.
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.2);
    lumen.wait_for("the dot", |app| {
        app.device_preset(PLUGIN) == Some(("Saw, loud".to_owned(), true))
    });
}

#[test]
fn the_arrows_step_through_programs_then_files_and_wrap_round() {
    let (mut lumen, _places) = launch("arrows");
    play(&mut lumen);
    // None chosen: next is the first, previous is the last.
    lumen.click(NEXT);
    lumen.wait_for("the first program", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Square, quiet".to_owned())
    });
    lumen.click(NEXT);
    lumen.wait_for("the second program", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Saw, loud".to_owned())
    });
    lumen.click(PREVIOUS);
    lumen.wait_for("back to the first", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Square, quiet".to_owned())
    });
    // Before the first comes the last: the last file, loaded.
    lumen.click(PREVIOUS);
    lumen.wait_for("the last file", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Warm".to_owned())
    });
    lumen.wait_for("its sound to be held", |app| {
        app.project().tracks[0].devices[0].state == state(0.4, 1.0)
    });
    // And after the last, the first again.
    lumen.click(NEXT);
    lumen.wait_for("round to the first", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Square, quiet".to_owned())
    });
}

#[test]
fn a_preset_file_chosen_in_the_menu_is_one_undo_step_and_undo_clears_the_name() {
    let (mut lumen, _places) = launch("file");
    lumen.open_preset_menu();
    lumen.click("Quiet square");
    lumen.wait_for("the project to hold the preset", |app| {
        app.project().tracks[0].devices[0].state == state(0.1, 0.0)
    });
    assert_eq!(lumen.app().undo_label(), Some("Load Quiet square"));
    lumen.wait_for("the field to name it", |app| {
        app.device_preset(PLUGIN) == Some(("Quiet square".to_owned(), false))
    });

    // Undo takes the sound away, and the name that went with it.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the old sound", |app| {
        app.project().tracks[0].devices[0].state.is_empty()
    });
    lumen.wait_for("the name to go", |app| app.device_preset(PLUGIN).is_none());
}

#[test]
fn a_damaged_file_is_refused_and_says_why_and_nothing_changes() {
    let (mut lumen, _places) = launch("broken");
    let before = lumen.app().project().clone();
    lumen.open_preset_menu();
    lumen.click("Broken");
    lumen.wait_for("the note", |app| {
        app.note()
            .is_some_and(|n| n.contains("Broken") && n.contains("cannot be used"))
    });
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().device_preset(PLUGIN), None);
    assert!(!lumen.app().is_dirty());
}

#[test]
fn save_as_writes_a_file_in_the_users_folder_that_loads_back_and_never_writes_over_one() {
    let (mut lumen, places) = launch("save-as");
    play(&mut lumen);
    // A sound to save: a program the plugin has taken.
    lumen.app_mut().choose_program(PLUGIN, 0, 1);
    lumen.wait_for("the program to settle", |app| {
        app.device_preset(PLUGIN).is_none() && app.device_param_known(PLUGIN, LEVEL) == Some(0.9)
    });
    lumen.frames(120);

    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.frames(3);
    assert!(lumen.shows("Preset name"), "the name field");
    lumen.type_into_menu("My lead");
    lumen.click("Save preset");
    let folder = plugin_folder(&places.user);
    let file = folder.join("My lead.vstpreset");
    let wait = file.clone();
    lumen.wait_for("the file", move |_| wait.exists());
    let saved = Preset::parse(&std::fs::read(&file).unwrap()).expect("a preset file");
    assert_eq!(
        saved.class_id,
        lumen_test_plugin::INSTRUMENT.map(|c| c as u8)
    );
    assert_eq!(saved.component, state(0.9, 1.0), "the sound the plugin had");
    assert!(saved.info.is_some(), "described by its XML");
    lumen.wait_for("the card to name it", |app| {
        app.device_preset(PLUGIN) == Some(("My lead".to_owned(), false))
    });
    assert!(
        lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("Saved") && n.contains("My lead"))
    );

    // The same name again gets another file, and the first is untouched.
    let first = std::fs::read(&file).unwrap();
    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("My lead");
    lumen.click("Save preset");
    let second = folder.join("My lead 2.vstpreset");
    let wait = second.clone();
    lumen.wait_for("the second file", move |_| wait.exists());
    assert_eq!(std::fs::read(&file).unwrap(), first);
    assert_eq!(
        saved_files(&folder),
        ["My lead 2.vstpreset", "My lead.vstpreset"]
    );

    // Both are in the menu now.
    lumen.open_preset_menu();
    assert!(lumen.shows("My lead") && lumen.shows("My lead 2"));
}

#[test]
fn save_writes_over_the_users_own_file_after_a_change() {
    let (mut lumen, places) = launch("save-over");
    play(&mut lumen);
    lumen.app_mut().choose_program(PLUGIN, 0, 1);
    lumen.wait_for("the program", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.9)
    });
    lumen.frames(120);
    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("Mine");
    lumen.click("Save preset");
    let file = plugin_folder(&places.user).join("Mine.vstpreset");
    let wait = file.clone();
    lumen.wait_for("the file", move |_| wait.exists());
    lumen.wait_for("the card to name it", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Mine".to_owned())
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    lumen.frames(5);
    let before = std::fs::read(&file).unwrap();

    // The sound changes in the plugin's window; Save puts it over the file.
    lumen.app_mut().set_device_param(PLUGIN, LEVEL, 0.2);
    lumen.wait_for("the dot", |app| {
        app.device_preset(PLUGIN).is_some_and(|p| p.1)
    });
    lumen.wait_for("the plugin to hear it", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.2)
    });
    lumen.frames(60);
    lumen.open_preset_menu();
    lumen.click("Save");
    let wait = file.clone();
    lumen.wait_for("the file to change", move |_| {
        std::fs::read(&wait).is_ok_and(|now| now != before)
    });
    assert_eq!(
        saved_files(&plugin_folder(&places.user)),
        ["Mine.vstpreset"],
        "no second file"
    );
    let now = Preset::parse(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(now.component, state(0.2, 1.0));
}

#[test]
fn show_presets_folder_names_the_folder_once_something_is_saved_there() {
    let (mut lumen, places) = launch("folder");
    lumen.open_preset_menu();
    lumen.click("Show presets folder");
    lumen.wait_for("the folder to be looked for", |app| {
        app.note().is_some_and(|n| n.contains("Nothing is saved"))
    });
    assert!(lumen.app().revealed_folders().is_empty());

    play(&mut lumen);
    lumen.app_mut().choose_program(PLUGIN, 0, 1);
    lumen.frames(120);
    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("Here");
    lumen.click("Save preset");
    let file = plugin_folder(&places.user).join("Here.vstpreset");
    lumen.wait_for("the file", move |_| file.exists());
    lumen.open_preset_menu();
    lumen.click("Show presets folder");
    lumen.wait_for("the folder to be shown", |app| {
        !app.revealed_folders().is_empty()
    });
    assert_eq!(
        lumen.app().revealed_folders(),
        [plugin_folder(&places.user)]
    );
}

#[test]
fn the_commands_open_the_menu_and_save_for_the_chosen_card() {
    let (mut lumen, _places) = launch("commands");
    // The card is chosen by clicking it.
    lumen.click(&format!("{NAME} on Keys"));
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen
        .harness
        .event(Event::Text("Choose a preset for the plugin".to_owned()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(lumen.shows("Search presets"), "the command opened the menu");

    // Esc closes it; Save with no file of its own asks for a name.
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(30);
    assert!(!lumen.shows("Search presets"));
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text(
        "Save the plugin's sound as a preset".to_owned(),
    ));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(5);
    assert!(lumen.shows("Preset name"), "Save asks for a name");
}

#[test]
fn a_builtin_synth_and_a_plugin_that_is_not_running_have_no_preset_field() {
    let (mut lumen, _places) = launch("no-field");
    lumen.app_mut().pretend_device_state(PLUGIN, "failed");
    lumen.frames(5);
    assert!(!lumen.shows(FIELD));
    assert!(!lumen.app().device_has_preset_field(PLUGIN));
}

#[test]
fn a_preset_saved_from_a_device_added_again_loads_back() {
    let (mut lumen, places) = launch("re-added");
    // Take the instrument off the track and add the same plugin again.
    lumen.click(&format!("{NAME} on Keys"));
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Remove device".to_owned()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the device to go", |app| {
        app.project().tracks[0].devices.is_empty()
    });
    let at = lumen
        .harness
        .get_by_label_contains(&format!("{NAME}, "))
        .rect()
        .center();
    lumen.double_click(at);
    lumen.wait_for("the new device", |app| {
        app.project().tracks[0].devices.len() == 1
    });
    let device = lumen.app().project().tracks[0].devices[0].id;
    wait_for_state(&mut lumen, device, "ready");
    lumen.wait_for("the card's field", move |app| {
        app.device_has_preset_field(device)
    });
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    play(&mut lumen);
    lumen.app_mut().choose_program(device, 0, 1);
    lumen.wait_for("the program", move |app| {
        app.device_param_known(device, LEVEL) == Some(0.9)
    });
    lumen.frames(120);

    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("Again");
    lumen.click("Save preset");
    let user = places.user.clone();
    lumen.wait_for("a file", move |_| any_preset(&user));
    lumen.frames(30);

    // And it loads: the plugin takes what it gave.
    lumen.open_preset_menu();
    lumen.click("Again");
    lumen.wait_for("the preset to be taken", move |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| d.id == device && d.state == state(0.9, 1.0))
    });
    assert!(
        !lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("could not use")),
        "{:?}",
        lumen.app().note()
    );
}

#[test]
fn a_preset_saved_from_a_device_put_back_by_undo_loads_back() {
    let (mut lumen, places) = launch("undone");
    // Take the instrument off the track and add the same plugin again.
    lumen.click(&format!("{NAME} on Keys"));
    lumen.key(Modifiers::COMMAND, Key::K);
    lumen.harness.event(Event::Text("Remove device".to_owned()));
    lumen.frames(2);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the device to go", |app| {
        app.project().tracks[0].devices.is_empty()
    });
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.wait_for("the new device", |app| {
        app.project().tracks[0].devices.len() == 1
    });
    let device = lumen.app().project().tracks[0].devices[0].id;
    wait_for_state(&mut lumen, device, "ready");
    lumen.wait_for("the card's field", move |app| {
        app.device_has_preset_field(device)
    });
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    play(&mut lumen);
    lumen.app_mut().choose_program(device, 0, 1);
    lumen.wait_for("the program", move |app| {
        app.device_param_known(device, LEVEL) == Some(0.9)
    });
    lumen.frames(120);

    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("Again");
    lumen.click("Save preset");
    let user = places.user.clone();
    lumen.wait_for("a file", move |_| any_preset(&user));
    lumen.frames(30);

    // And it loads: the plugin takes what it gave.
    lumen.open_preset_menu();
    lumen.click("Again");
    lumen.wait_for("the preset to be taken", move |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| d.id == device && d.state == state(0.9, 1.0))
    });
    assert!(
        !lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("could not use")),
        "{:?}",
        lumen.app().note()
    );
}

#[test]
fn the_name_is_cut_to_thirty_characters_in_the_field_and_in_the_file() {
    let (mut lumen, places) = launch("long-name");
    play(&mut lumen);
    lumen.app_mut().choose_program(PLUGIN, 0, 1);
    lumen.wait_for("the program", |app| {
        app.device_param_known(PLUGIN, LEVEL) == Some(0.9)
    });
    lumen.frames(120);
    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    // Typing more than the limit stops at it.
    lumen.type_into_menu(&"abcdefghij".repeat(5));
    lumen.click("Save preset");
    let user = places.user.clone();
    lumen.wait_for("a file", move |_| any_preset(&user));
    let files = found(&places.user);
    assert_eq!(files.len(), 1);
    let stem = files[0].file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(stem, "abcdefghij".repeat(3), "thirty characters");
}

#[test]
fn the_menu_stays_open_while_a_name_is_typed_whatever_the_pointer_does() {
    let (mut lumen, _places) = launch("stays");
    lumen.open_preset_menu();
    lumen.click("Save as\u{2026}");
    lumen.type_into_menu("My");
    // The pointer leaves the menu and rests far from it: it used to close.
    lumen.harness.hover_at(eframe::egui::pos2(5.0, 5.0));
    lumen.frames(120);
    assert!(lumen.shows("Preset name"), "the menu went away");
    lumen.type_into_menu(" lead");
    assert!(lumen.shows("Preset name"));
}

#[test]
fn a_preset_a_plugin_refuses_at_first_is_given_to_it_again_and_taken() {
    let (mut lumen, _places) = launch_with("reluctant", "Reluctant");
    lumen.open_preset_menu();
    lumen.click("Quiet square");
    // Refused twice, then taken on the third try: no note of a refusal.
    lumen.wait_for("the project to hold the preset", |app| {
        app.project().tracks[0].devices[0].state == state(0.1, 0.0)
    });
    assert!(
        !lumen
            .app()
            .note()
            .is_some_and(|n| n.contains("could not use")),
        "{:?}",
        lumen.app().note()
    );
    lumen.wait_for("the field to name it", |app| {
        app.device_preset(PLUGIN).map(|p| p.0) == Some("Quiet square".to_owned())
    });
}

#[test]
fn a_preset_a_plugin_always_refuses_says_so_after_its_tries_and_changes_nothing() {
    let (mut lumen, _places) = launch_with("refuses", "Refuses");
    let before = lumen.app().project().clone();
    lumen.open_preset_menu();
    lumen.click("Quiet square");
    lumen.wait_for("the refusal", |app| {
        app.note().is_some_and(|n| n.contains("could not use"))
    });
    assert_eq!(lumen.app().project(), &before);
    assert_eq!(lumen.app().device_preset(PLUGIN), None);
    // The plugin is free to take another try afterwards.
    lumen.open_preset_menu();
    lumen.click("Broken");
    lumen.frames(10);
}
