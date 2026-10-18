//! Loading a state into a running plugin through the hosting manager and real
//! host processes: the programs a link brings, a preset's state heard at once
//! with its values read back, a state the plugin refuses, the edit
//! controller's own state, and a fresh read of every value.

mod support;

use std::time::Duration;

use lumen_core::midi::Event;
use lumen_hosting::{Captured, Happened, Manager};
use lumen_test_plugin::controller::PROGRAM;
use lumen_test_plugin::params::{LEVEL, WAVE};
use support::*;

fn manager() -> Manager {
    Manager::new(|| {})
}

fn held() -> [Event; 2] {
    [note_on(0, 69), note_off(63, 69)]
}

/// The loudest sample a held note makes in a block.
fn loudness(link: &mut lumen_hosting::Link) -> f32 {
    let (left, _, reply) = play(link, 64, &[], &held());
    assert!(reply.is_some(), "the block was taken");
    left[2..60].iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

/// The controller state of the test plugin: a tag and a view value.
fn view(value: f64) -> Vec<u8> {
    let mut bytes = b"LTV1".to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn captured(manager: &mut Manager, device: u64) -> Captured {
    manager.capture(device);
    wait_for(
        manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::State(state) => Ok(state.expect("the state was read")),
            other => Err(other),
        },
    )
}

fn loaded(manager: &mut Manager) -> Result<Vec<(u32, f64)>, String> {
    wait_for(
        manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::Loaded(answer) => Ok(answer),
            other => Err(other),
        },
    )
}

#[test]
fn a_link_brings_the_programs_the_plugin_offers() {
    let mut manager = manager();
    manager.want(1, instrument("presets")).unwrap();
    let linked = linked(&mut manager, 1);
    assert_eq!(linked.programs.len(), 1);
    assert_eq!(linked.programs[0].name, "Factory");
    assert_eq!(linked.programs[0].parameter, Some(PROGRAM));
    assert_eq!(linked.programs[0].programs.len(), 3);
    // A plugin with none lists none.
    manager.want(2, instrument("")).unwrap();
    assert!(self::linked(&mut manager, 2).programs.is_empty());
}

#[test]
fn a_state_loaded_into_a_running_plugin_is_heard_and_its_values_come_back() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut first = linked(&mut manager, 1);
    manager.set_param(1, LEVEL, 0.25);
    for _ in 0..200 {
        if loudness(&mut first.link) == 0.25 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let preset = captured(&mut manager, 1);

    // A second plugin, still at its defaults, is given that sound.
    manager.want(2, instrument("")).unwrap();
    let mut second = linked(&mut manager, 2);
    assert_eq!(loudness(&mut second.link), 0.5);
    manager.load_state(2, preset.component, preset.controller, Vec::new());
    let values = loaded(&mut manager).expect("the plugin took the state");
    assert!(values.contains(&(LEVEL, 0.25)), "{values:?}");
    assert_eq!(loudness(&mut second.link), 0.25);
}

#[test]
fn values_given_with_a_state_are_set_over_it() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    let state = captured(&mut manager, 1);
    manager.load_state(1, state.component, None, vec![(WAVE, 1.0), (LEVEL, 0.75)]);
    let values = loaded(&mut manager).unwrap();
    assert!(values.contains(&(LEVEL, 0.75)) && values.contains(&(WAVE, 1.0)));
    assert!(loudness(&mut linked.link) > 0.0);
}

#[test]
fn a_state_the_plugin_refuses_is_reported_and_leaves_it_as_it_was() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(loudness(&mut linked.link), 0.5);
    manager.load_state(1, vec![9; 40], None, Vec::new());
    assert!(loaded(&mut manager).is_err());
    assert_eq!(loudness(&mut linked.link), 0.5);
    // And the host goes on: it can still be asked things.
    assert!(!captured(&mut manager, 1).component.is_empty());
}

#[test]
fn the_controllers_own_state_goes_in_with_the_component_and_comes_out_with_it() {
    let mut manager = manager();
    manager.want(1, instrument("presets")).unwrap();
    let _linked = linked(&mut manager, 1);
    assert_eq!(
        captured(&mut manager, 1).controller,
        Some(view(0.5)),
        "the plugin's own, before anything was loaded"
    );
    let state = captured(&mut manager, 1).component;
    manager.load_state(1, state, Some(view(0.125)), Vec::new());
    loaded(&mut manager).unwrap();
    assert_eq!(captured(&mut manager, 1).controller, Some(view(0.125)));

    // A host started with it has it too.
    let mut start = instrument("presets");
    start.controller_state = view(0.25);
    manager.want(2, start).unwrap();
    let _second = linked(&mut manager, 2);
    assert_eq!(captured(&mut manager, 2).controller, Some(view(0.25)));

    // A plugin whose controller keeps none gives none.
    manager.want(3, instrument("")).unwrap();
    let _third = linked(&mut manager, 3);
    assert_eq!(captured(&mut manager, 3).controller, None);
}

#[test]
fn a_refresh_brings_every_parameters_value_as_the_plugin_has_it_now() {
    let mut manager = manager();
    manager.want(1, instrument("presets")).unwrap();
    let _linked = linked(&mut manager, 1);
    manager.set_param(1, PROGRAM, 0.0);
    manager.refresh(1);
    let values = wait_for(
        &mut manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::Values(v) => Ok(v),
            other => Err(other),
        },
    );
    // The plugin's program parameter set its own level and wave.
    assert!(values.contains(&(PROGRAM, 0.0)), "{values:?}");
    assert!(values.contains(&(LEVEL, 0.25)), "{values:?}");
}
