//! Built-in effects in the app (M8 slice B0): Prism, the utility device, from
//! the command that adds it to its card's knobs, All parameters, automation
//! lanes, MIDI learn, overriding a lane and giving it back, presets and copying
//! it to another track. Everything a plugin's card does, done by a device that
//! is Lumen's own code.

use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_devices::utility::{GAIN, PAN, POLARITY_L, POLARITY_R, SWAP, WIDTH};
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::automation::Target;
use lumen_project::model::{Id, PluginRef};
use lumen_project::testing::TempDir;

use super::lane_view::{peak, run_command, tone};
use super::{Lumen, wav};

/// The built-in device the first track has, if it has one.
fn prism(lumen: &Lumen, track: usize) -> Option<Id> {
    lumen
        .app()
        .project()
        .tracks
        .get(track)?
        .devices
        .iter()
        .find(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
        .map(|d| d.id)
}

/// Starts with a tone on one audio track, selected, and Prism added to it by
/// the command (as a person runs it from the palette).
fn launched(name: &str) -> (Lumen, Id, Id) {
    let (mut lumen, track) = tone(name);
    run_command(&mut lumen, "Add Prism");
    lumen.wait_for("Prism's card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    lumen.frames(20);
    let device = prism(&lumen, 0).expect("the device is on the track");
    (lumen, track, device)
}

#[track_caller]
fn at(lumen: &Lumen, label: &str) -> Pos2 {
    lumen.harness.get_by_label_contains(label).rect().center()
}

/// What the app is playing now: the loudest frame of a moment.
fn heard(lumen: &mut Lumen) -> f32 {
    lumen
        .listen(Duration::from_millis(120), 1_000)
        .iter()
        .fold(0.0f32, |m, f| m.max(f[0].abs()))
}

fn param(lumen: &Lumen, device: Id, id: u32) -> Option<f64> {
    lumen
        .app()
        .device_params(device)
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, value)| *value)
}

#[test]
fn the_command_puts_prism_on_the_track_named_as_its_own_device_with_eight_knobs() {
    let (mut lumen, _, device) = launched("builtin-add");
    assert_eq!(lumen.app().undo_label(), Some("Add Prism"));
    assert!(lumen.app().is_builtin(device));
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(device)
    });
    // The card says the device's own name.
    assert!(lumen.shows("Prism on tone"), "the card's spoken name");
    // The eight knobs are the table's, in its order.
    assert_eq!(
        lumen.app().strip_parameters(device),
        [
            GAIN,
            PAN,
            WIDTH,
            lumen_devices::utility::BASS_MONO,
            POLARITY_L,
            POLARITY_R,
            SWAP,
            lumen_devices::utility::DC_BLOCK
        ]
    );
    assert!(
        lumen.shows("Gain, parameter: 0.0 dB"),
        "the gain's own words"
    );
    assert!(lumen.shows("Width, parameter: 100 %"));
    assert!(
        lumen.harness.query_by_label("All parameters (8)").is_some(),
        "every parameter is one button away"
    );
}

#[test]
fn dragging_the_gain_makes_the_track_quieter_and_is_one_undo_step() {
    let (mut lumen, _, device) = launched("builtin-gain");
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let before = heard(&mut lumen);
    assert!(before > 0.0, "the tone is heard: {before}");

    let knob = at(&lumen, "Gain, parameter");
    lumen.drag(knob, knob + vec2(0.0, 60.0), Modifiers::NONE);
    let gain = param(&lumen, device, GAIN).expect("the gain moved");
    assert!(gain < 0.62, "dragged down: {gain}");
    assert_eq!(lumen.app().undo_label(), Some("Change Gain"));
    let after = heard(&mut lumen);
    assert!(after < before * 0.9, "quieter: {before} then {after}");

    // One undo takes the whole drag back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(20);
    let back = param(&lumen, device, GAIN).expect("the gain is still there");
    assert!(back > 0.7, "at its default again: {back}");
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_value_typed_on_the_knob_is_read_at_once_and_is_one_undo_step() {
    let (mut lumen, _, device) = launched("builtin-typed");
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    lumen.harness.event(Event::Text("-6".into()));
    lumen.frames(4);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    let gain = param(&lumen, device, GAIN).expect("the gain is set");
    // −6 dB of −60 to +24 is 0.6429.
    assert!((gain - 0.6429).abs() < 1e-3, "{gain}");
    assert_eq!(lumen.app().undo_label(), Some("Change Gain"));
    lumen.frames(5);
    assert!(
        lumen.shows("Gain, parameter: \u{2212}6.0 dB"),
        "the knob reads it"
    );
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(8);

    // What it does not understand changes nothing, and says so.
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("loud");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the note", |app| {
        app.note().is_some_and(|n| n.contains("did not understand"))
    });
    assert_eq!(
        lumen.app().note(),
        Some("Prism did not understand \u{201c}loud\u{201d}.")
    );
}

#[test]
fn all_parameters_lists_the_eight_and_a_star_puts_one_on_the_strip() {
    let (mut lumen, _, device) = launched("builtin-panel");
    lumen.click("All parameters (8)");
    lumen.frames(10);
    assert!(
        lumen.shows("8 of 8 on the strip"),
        "every parameter is on the strip"
    );
    lumen.click("Take Pan off the strip");
    lumen.frames(5);
    assert_eq!(lumen.app().strip_parameters(device).len(), 7);
    assert!(!lumen.app().strip_parameters(device).contains(&PAN));
    lumen.click("Put Pan on the strip");
    lumen.frames(5);
    assert!(lumen.app().strip_parameters(device).contains(&PAN));
    // The strip's choice is part of the project's view.
    assert!(
        lumen
            .app()
            .project_as_saved()
            .view
            .shown_params
            .get(&device)
            .is_some_and(|s| s.contains(&PAN)),
        "the strip is saved with the project"
    );
}

#[test]
fn a_switch_reads_its_own_words_and_its_click_is_one_undo_step() {
    let (mut lumen, _, device) = launched("builtin-switch");
    lumen.show_whole_cards();
    lumen.click("Swap L/R");
    lumen.frames(10);
    assert_eq!(param(&lumen, device, SWAP), Some(1.0));
    assert_eq!(lumen.app().undo_label(), Some("Change Swap L/R"));
    assert_eq!(lumen.app().device_choices(device, SWAP), ["Off", "On"]);
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(8);
    assert_eq!(param(&lumen, device, SWAP), Some(0.0));
}

/// A lane of Prism's gain from the top to silence over two quarters.
fn gain_lane() -> Lane {
    Lane::new(vec![
        Point::new(Ticks::ZERO, 0.7142857),
        Point::new(Ticks::quarters(2), 0.0),
    ])
    .unwrap()
}

#[test]
fn a_lane_drives_the_gain_it_is_taken_over_by_a_drag_and_given_back() {
    let (mut lumen, track, device) = launched("builtin-lane");
    let target = Target::Device {
        device,
        param: GAIN,
    };
    lumen
        .app_mut()
        .give_lane(track, target, gain_lane())
        .unwrap();
    lumen.frames(20);
    // The knob's menu shows the lane it drives.
    lumen.right_click(at(&lumen, "Gain, parameter"));
    lumen.click("Automate");
    lumen.frames(30);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(2));
    lumen.frames(30);
    let quiet = peak(&mut lumen);
    lumen.key(Modifiers::NONE, Key::Space);
    assert!(quiet < 0.01, "the lane fades it out: {quiet}");

    // Moving the knob takes the lane over: the control's own value is heard.
    let knob = at(&lumen, "Gain, parameter");
    lumen.drag(knob, knob + vec2(0.0, -40.0), Modifiers::NONE);
    lumen.frames(20);
    assert!(lumen.app().lane_taken_over(track, target));
    // And the lane's Re-enable button gives it back.
    let reenable = lumen
        .harness
        .get_by_label_contains("Re-enable")
        .rect()
        .center();
    lumen.harness.hover_at(reenable);
    lumen.frames(2);
    lumen.harness.drag_at(reenable);
    lumen.harness.drop_at(reenable);
    lumen.frames(20);
    assert!(!lumen.app().lane_taken_over(track, target));
}

#[test]
fn the_lane_names_the_device_and_its_parameter() {
    let (mut lumen, track, device) = launched("builtin-chooser");
    let target = Target::Device {
        device,
        param: GAIN,
    };
    lumen
        .app_mut()
        .give_lane(track, target, gain_lane())
        .unwrap();
    lumen.frames(20);
    // The knob's menu shows the lane it drives.
    lumen.right_click(at(&lumen, "Gain, parameter"));
    lumen.click("Automate");
    lumen.frames(30);
    // The lane reads as the device and the parameter, from the table the
    // chooser lists: "Prism · Gain".
    assert!(
        lumen
            .harness
            .query_by_label_contains("Automation lane of tone: Prism")
            .is_some(),
        "the lane names Prism and its parameter"
    );
}

#[test]
fn a_controller_learned_on_the_gain_moves_it() {
    let fake = FakeControl::new();
    fake.plug(Direction::Input, "Pad");
    let dir = TempDir::new("e2e-builtin-learn");
    let with = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.with_midi(with));
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let first = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[first]);
    lumen.frames(30);
    super::midi_from::switch_on_for_tracks(&mut lumen, &fake, "Pad");
    run_command(&mut lumen, "Add Prism");
    lumen.wait_for("Prism's card", |app| {
        !app.project().tracks[0].devices.is_empty()
    });
    lumen.frames(20);
    let track = lumen.app().project().tracks[0].id;
    let device = prism(&lumen, 0).unwrap();
    let target = Target::Device {
        device,
        param: GAIN,
    };
    lumen.right_click(at(&lumen, "Gain, parameter"));
    lumen.click("Learn a controller…");
    lumen.frames(5);
    assert_eq!(lumen.app().controller_learning(), Some((track, target)));
    super::controllers::cc(&fake, 0, 21, 100);
    lumen.wait_for("the mapping", |app| app.project().mappings.len() == 1);
    for value in [10_u8, 60, 127] {
        super::controllers::cc(&fake, 0, 21, value);
        std::thread::sleep(Duration::from_millis(25));
        lumen.frames(2);
    }
    lumen.wait_for("the gain at the top", |app| {
        app.device_params(device)
            .iter()
            .any(|(id, v)| *id == GAIN && *v > 0.85)
    });
}

#[test]
fn a_preset_is_saved_from_the_card_and_loaded_back_as_one_undo_step() {
    let dir = TempDir::new("e2e-builtin-preset");
    let presets = dir.path().join("presets");
    let with_presets = presets.clone();
    let mut lumen = Lumen::start_with(dir, move |l| {
        l.with_preset_folders(with_presets.clone(), Vec::new())
    });
    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    let first = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[first]);
    lumen.frames(30);
    run_command(&mut lumen, "Add Prism");
    lumen.wait_for("Prism's card", |app| {
        !app.project().tracks[0].devices.is_empty()
    });
    lumen.frames(20);
    let device = prism(&lumen, 0).unwrap();
    assert!(
        lumen.app().device_has_preset_field(device),
        "a built-in keeps presets too"
    );
    lumen.show_whole_cards();
    lumen.frames(10);

    // Save as… from the card's own field.
    lumen.click("Preset of Prism");
    lumen.frames(5);
    lumen.click("Save as…");
    lumen.frames(5);
    lumen
        .harness
        .get_by_label("Preset name")
        .type_text("Bright");
    lumen.key(Modifiers::NONE, Key::Enter);
    let root = format!("{} Presets", lumen_core::product::PRODUCT_NAME);
    // The folder is the device's stable saved name, not its personality's: a
    // renamed personality never orphans a person's presets (review L5).
    let folder = presets.join(root).join("utility");
    lumen.wait_for("the preset file", move |_| {
        folder.join("Bright.lumenpreset").is_file()
    });
    lumen.frames(10);

    // Change the gain, then take the sound back from the file's row.
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("-30");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    let changed = param(&lumen, device, GAIN).expect("a value");
    assert!((changed - 0.357).abs() < 0.01, "{changed}");
    lumen.click("Preset of Prism");
    lumen.frames(10);
    lumen.click("Bright");
    lumen.wait_for("the preset's values", |app| {
        app.device_params(device)
            .iter()
            .any(|(id, v)| *id == GAIN && (*v - 0.714).abs() < 0.01)
    });
    assert_eq!(lumen.app().undo_label(), Some("Load Bright"));
    // One undo puts the changed sound back.
    lumen.key(Modifiers::COMMAND, Key::Z);
    lumen.frames(10);
    let back = param(&lumen, device, GAIN).expect("a value");
    assert!((back - 0.357).abs() < 0.01, "{back}");
}

#[test]
fn the_card_copies_to_another_track_with_its_values() {
    let (mut lumen, track, device) = launched("builtin-copy");
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("-12");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    run_command(&mut lumen, "Add audio track");
    lumen.frames(30);
    let second = lumen.app().project().tracks[1].id;
    let copy = lumen
        .app_mut()
        .copy_device(track, device, second, None)
        .expect("the copy is made");
    lumen.frames(20);
    assert!(lumen.app().is_builtin(copy));
    lumen.wait_for("the copy at work", |app| app.builtin_attached(copy));
    let gain = param(&lumen, copy, GAIN).expect("the copy's gain");
    // −12 dB of −60 to +24.
    assert!((gain - 0.571).abs() < 0.01, "{gain}");
}

/// M4: the track's own menu adds Prism, so the device is reachable without the
/// browser or the palette — one undo step, as the command is.
#[test]
fn the_track_menu_adds_prism_to_that_track() {
    let (mut lumen, track) = tone("builtin-menu");
    lumen.frames(20);
    let header = lumen.harness.get_by_label_contains("Track tone,").rect();
    lumen.right_click(header.center());
    lumen.frames(5);
    assert!(lumen.shows("Add Prism (utility)"), "the row is there");
    lumen.click("Add Prism (utility)");
    lumen.wait_for("Prism's card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    let device = prism(&lumen, 0).expect("the device is on the track");
    assert_eq!(lumen.app().undo_label(), Some("Add Prism"));
    lumen.wait_for("the audio thread to have it", |app| {
        app.builtin_attached(device)
    });
    // The device went to the track the menu was opened on.
    assert_eq!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map(|t| t.devices.len()),
        Some(1)
    );
    assert!(lumen.shows("Prism on tone"));
}

/// M4: the tray's own menu offers Prism too — in its empty state and past the
/// last card — so the chain panel can grow without the browser.
#[test]
fn the_device_trays_menu_adds_prism_too() {
    let (mut lumen, track) = tone("builtin-tray-menu");
    // Open the Devices tab and make the tray tall, as a person would.
    lumen.show_whole_cards();
    lumen.frames(20);
    // The tray is empty: its menu offers Prism.
    let browse = lumen.harness.get_by_label("Browse devices").rect();
    let room = Pos2::new(browse.right() + 120.0, browse.center().y + 40.0);
    lumen.right_click(room);
    lumen.frames(5);
    assert!(
        lumen.shows("Add Prism (utility)"),
        "the empty tray's menu has it"
    );
    lumen.click("Add Prism (utility)");
    lumen.wait_for("Prism's card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    let device = prism(&lumen, 0).expect("the device is on the track");
    assert_eq!(lumen.app().undo_label(), Some("Add Prism"));
    assert_eq!(
        lumen
            .app()
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map(|t| t.devices.len()),
        Some(1)
    );
    // Past the last card the menu is there still.
    lumen.show_whole_cards();
    lumen.frames(20);
    let card = lumen.harness.get_by_label_contains("Prism on tone").rect();
    // Past the output chip, in the room the tray keeps after the last card.
    let past = Pos2::new(card.right() + 300.0, card.center().y);
    lumen.right_click(past);
    lumen.frames(5);
    assert!(
        lumen.shows("Add Prism (utility)"),
        "the tail's menu has it too"
    );
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(lumen.app().is_builtin(device));
}

/// M1: the card's power button means something for a built-in too. Bypassed,
/// the device is not run and the track plays its dry signal; turned on again it
/// glides back in. Neither click clicks.
#[test]
fn the_power_button_bypasses_prism_and_the_dry_signal_comes_back() {
    let (mut lumen, _, device) = launched("builtin-power");
    // −12 dB, typed, so the device changes the sound plainly.
    lumen
        .harness
        .get_by_label_contains("Gain, parameter")
        .click();
    lumen.frames(3);
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(3);
    lumen
        .harness
        .get_by_label("Gain: type a value")
        .type_text("-12");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.frames(10);
    assert!((param(&lumen, device, GAIN).unwrap() - 0.571).abs() < 0.01);

    lumen.show_whole_cards();
    lumen.frames(10);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    let quiet = heard(&mut lumen);
    assert!(quiet > 0.0, "the tone is heard through the device: {quiet}");

    lumen.click("Power Prism");
    lumen.frames(10);
    assert!(
        lumen.app().project().tracks[0]
            .devices
            .iter()
            .any(|d| d.id == device && d.bypass),
        "the power button bypassed it"
    );
    let loud = heard(&mut lumen);
    assert!(
        loud > quiet * 1.8,
        "the dry signal is back: {quiet} then {loud}"
    );

    lumen.click("Power Prism");
    lumen.frames(10);
    let again = heard(&mut lumen);
    assert!(
        again < loud * 0.7,
        "on again, it is quieter: {loud} then {again}"
    );
    lumen.key(Modifiers::NONE, Key::Space);
}

#[test]
fn a_value_changed_on_prism_is_saved_with_the_project_and_is_back_after_a_reopen() {
    let (mut lumen, _, device) = launched("builtin-save");
    let knob = at(&lumen, "Gain, parameter");
    lumen.drag(knob, knob + vec2(0.0, 60.0), Modifiers::NONE);
    let changed = param(&lumen, device, GAIN).expect("the gain moved");
    assert!(changed < 0.62, "dragged down: {changed}");

    // The save is not refused for the values a built-in holds.
    let path = lumen_app::testing::project_file(lumen.dir(), "Prism song");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());

    let dir: TempDir = lumen.crash();
    let mut again = Lumen::start_with(dir, |launch| launch.opening(path.clone()));
    again.wait_for("the project to open", |app| !app.is_busy());
    again.frames(10);
    let device = prism(&again, 0).expect("Prism is on the reopened track");
    let back = param(&again, device, GAIN).expect("the gain is saved");
    assert!(
        (back - changed).abs() < 1e-6,
        "the value is back: {changed} then {back}"
    );
}

#[test]
fn the_whole_cards_footer_opens_all_parameters_and_the_header_names_the_device() {
    let (mut lumen, _, _) = launched("builtin-footer");
    lumen.show_whole_cards();
    // The chosen card's header: the power button and the name that opens the
    // card's menu; the footer's button opens every parameter.
    assert!(lumen.harness.query_by_label_contains("Prism").is_some());
    lumen.click("Open all 8 parameters");
    lumen.frames(10);
    assert!(
        lumen.shows("8 of 8 on the strip"),
        "the footer's button opens All parameters"
    );
}
