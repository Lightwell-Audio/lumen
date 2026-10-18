//! Programs and preset files through a real host process: the lists a
//! plugin offers, choosing one, the edit controller's own state, and a
//! preset file written from one plugin and read into another.

mod support;

use lumen_plugin_host::preset::{Preset, info_xml};
use lumen_plugin_host::protocol::EditKind;
use lumen_test_plugin::controller::PROGRAM;
use lumen_test_plugin::params::{LEVEL, WAVE};
use support::{SLOW, instrument, note_off, note_on};

fn play(host: &mut lumen_plugin_host::client::HostProcess) -> Vec<f32> {
    host.process(
        64,
        vec![],
        vec![note_on(0, 69), note_off(63, 69)],
        vec![],
        SLOW,
    )
    .expect("process")
    .output[0]
        .clone()
}

/// The controller state of the test plugin: a tag and a view value.
fn view(value: f64) -> Vec<u8> {
    let mut bytes = b"LTV1".to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

#[test]
fn a_plugin_lists_its_programs_and_the_parameter_that_selects_them() {
    let mut host = instrument("presets");
    let lists = host.programs(SLOW).unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].name, "Factory");
    assert_eq!((lists[0].id, lists[0].unit), (0, 0));
    assert_eq!(lists[0].parameter, Some(PROGRAM));
    assert_eq!(
        lists[0].programs,
        vec!["Square, quiet", "Saw, loud", "Saw, half"]
    );
    // The plugin's text for the selecting parameter names the program.
    assert_eq!(host.param_text(PROGRAM, 0.5, SLOW).unwrap(), "Saw, loud");
    let (_, params) = host.parameters(SLOW).unwrap();
    let selector = params.iter().find(|p| p.id == PROGRAM).unwrap();
    assert!(selector.program_change());
    assert_eq!(selector.steps, 2);
}

#[test]
fn a_plugin_with_no_programs_lists_none() {
    let mut host = instrument("");
    assert!(host.programs(SLOW).unwrap().is_empty());
}

#[test]
fn choosing_a_program_changes_what_is_heard_and_is_reported() {
    let mut host = instrument("presets");
    let before = play(&mut host);
    assert!(before[1..60].iter().all(|&s| s.abs() == 0.5), "{before:?}");

    // Program 0 is a quiet square wave.
    host.set_param(PROGRAM, 0.0, SLOW).unwrap();
    let quiet = play(&mut host);
    assert!(quiet[1..60].iter().all(|&s| s.abs() == 0.25), "{quiet:?}");

    // The plugin changed its level and wave itself: Lumen is told, in order,
    // and told that values changed beyond them.
    let taken = host.take_edits(SLOW).unwrap();
    let mine = |id| {
        taken
            .list
            .iter()
            .filter(|e| e.id == id && e.kind == EditKind::Perform)
            .count()
    };
    assert_eq!((mine(LEVEL), mine(WAVE)), (1, 1), "{:?}", taken.list);
    assert!(taken.list.iter().any(|e| e.kind == EditKind::Dirty));

    // And the controller's values follow.
    let (_, params) = host.parameters(SLOW).unwrap();
    let value = |id| params.iter().find(|p| p.id == id).unwrap().value;
    assert_eq!(
        (value(LEVEL), value(WAVE), value(PROGRAM)),
        (0.25, 0.0, 0.0)
    );
}

#[test]
fn the_controller_keeps_a_state_of_its_own() {
    let mut host = instrument("presets");
    assert_eq!(host.controller_state(SLOW).unwrap(), view(0.5));
    host.set_controller_state(view(0.125), SLOW).unwrap();
    assert_eq!(host.controller_state(SLOW).unwrap(), view(0.125));

    // Bytes the controller did not write are refused and change nothing; the
    // connection goes on.
    assert!(host.set_controller_state(vec![1, 2, 3], SLOW).is_err());
    assert!(host.set_controller_state(view(7.0), SLOW).is_err());
    assert!(host.set_controller_state(view(f64::NAN), SLOW).is_err());
    assert_eq!(host.controller_state(SLOW).unwrap(), view(0.125));

    // The component's state does not carry it.
    let state = host.state(SLOW).unwrap();
    host.set_controller_state(view(0.75), SLOW).unwrap();
    host.set_state(state, SLOW).unwrap();
    assert_eq!(host.controller_state(SLOW).unwrap(), view(0.75));
}

#[test]
fn a_controller_with_no_state_writes_nothing() {
    let mut host = instrument("");
    assert!(host.controller_state(SLOW).unwrap().is_empty());
    host.set_controller_state(Vec::new(), SLOW).unwrap();
}

#[test]
fn a_preset_file_from_one_plugin_sets_up_another() {
    let mut a = instrument("presets");
    a.set_param(LEVEL, 0.25, SLOW).unwrap();
    a.set_controller_state(view(0.125), SLOW).unwrap();
    // The processor hears a value at the start of its next block, and its
    // state holds it from then on.
    play(&mut a);
    let file = Preset {
        class_id: [7; 16],
        component: a.state(SLOW).unwrap(),
        controller: Some(a.controller_state(SLOW).unwrap()),
        info: Some(info_xml("Test instrument")),
    }
    .to_bytes();

    // A second, fresh plugin reads the file back.
    let mut b = instrument("presets");
    let preset = Preset::parse(&file).unwrap();
    assert_eq!(preset.class_id, [7; 16]);
    b.set_state(preset.component, SLOW).unwrap();
    b.set_controller_state(preset.controller.unwrap(), SLOW)
        .unwrap();
    let (_, params) = b.parameters(SLOW).unwrap();
    assert_eq!(params.iter().find(|p| p.id == LEVEL).unwrap().value, 0.25);
    assert_eq!(b.controller_state(SLOW).unwrap(), view(0.125));
    let block = play(&mut b);
    assert!(block[1..60].iter().all(|&s| s.abs() == 0.25), "{block:?}");
}

#[test]
fn a_file_that_is_not_this_plugins_state_is_refused_and_the_plugin_keeps_its_sound() {
    let mut host = instrument("presets");
    host.set_param(LEVEL, 0.75, SLOW).unwrap();
    // A well-formed file whose state was made for something else.
    let file = Preset {
        class_id: [7; 16],
        component: vec![9; 40],
        controller: None,
        info: None,
    }
    .to_bytes();
    let preset = Preset::parse(&file).unwrap();
    assert!(host.set_state(preset.component, SLOW).is_err());
    let (_, params) = host.parameters(SLOW).unwrap();
    assert_eq!(params.iter().find(|p| p.id == LEVEL).unwrap().value, 0.75);
    let block = play(&mut host);
    assert!(block[1..60].iter().all(|&s| s.abs() == 0.75), "{block:?}");
}
