//! Parameters through the hosting manager and real host processes: the list
//! a link brings, values the project holds over a plugin's saved state, values
//! set while running, what the plugin does inside itself, its own text, and
//! controllers carried to a plugin that maps them.

mod support;

use std::time::Duration;

use lumen_core::midi::{Event, Msg};
use lumen_hosting::{Happened, Manager};
use lumen_plugin_host::protocol::EditKind;
use lumen_test_plugin::controller::TRIGGER;
use lumen_test_plugin::params::{LEVEL, WAVE};
use support::*;

fn manager() -> Manager {
    Manager::new(|| {})
}

/// The loudest sample a held note makes in a block.
fn loudness(link: &mut lumen_hosting::Link, events: &[Event]) -> f32 {
    let (left, _, reply) = play(link, 64, &[], events);
    assert!(reply.is_some(), "the block was taken");
    left[2..60].iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

fn held() -> [Event; 2] {
    [note_on(0, 69), note_off(63, 69)]
}

#[test]
fn a_link_brings_the_plugins_parameters_and_groups() {
    let mut manager = manager();
    manager.want(1, instrument("many_params=40")).unwrap();
    let linked = linked(&mut manager, 1);
    assert_eq!(linked.params.len(), 42);
    assert_eq!(linked.params_total, 42);
    assert_eq!(linked.params[0].title, "Level");
    assert_eq!(linked.units.len(), 5);
    assert_eq!(linked.units[1].name, "Group 1");
}

#[test]
fn values_the_project_holds_are_in_place_before_the_first_block() {
    let mut manager = manager();
    let mut start = instrument("");
    start.values = vec![(LEVEL, 0.25)];
    manager.want(1, start).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(loudness(&mut linked.link, &held()), 0.25);
    // The list shows the value the saved state holds, before the project's.
    assert_eq!(linked.params[0].value, 0.5);
}

#[test]
fn a_value_set_while_running_is_heard_in_the_next_block() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(loudness(&mut linked.link, &held()), 0.5);
    manager.set_param(1, LEVEL, 0.75);
    // The host thread sends it on; give it a moment, then the next block has it.
    let mut heard = 0.0;
    for _ in 0..200 {
        heard = loudness(&mut linked.link, &held());
        if heard == 0.75 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(heard, 0.75);
}

#[test]
fn a_drag_is_one_value_not_a_queue_of_them() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    for i in 0..500 {
        manager.set_param(1, LEVEL, f64::from(i) / 1000.0);
    }
    manager.set_param(1, LEVEL, 0.9);
    let mut heard = 0.0;
    for _ in 0..400 {
        heard = loudness(&mut linked.link, &held());
        if heard == 0.9 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(heard, 0.9);
}

#[test]
fn what_the_plugin_does_in_its_own_window_comes_as_edits_in_order() {
    let mut manager = manager();
    manager.want(1, instrument("window_edits")).unwrap();
    let _linked = linked(&mut manager, 1);
    manager.set_param(1, TRIGGER, 1.0);
    let taken = wait_for(
        &mut manager,
        Duration::from_secs(10),
        |id, happened| match happened {
            Happened::Edits(t) if id == 1 && !t.list.is_empty() => Ok(t),
            other => Err(other),
        },
    );
    let kinds: Vec<_> = taken.list.iter().map(|e| (e.id, e.kind)).collect();
    assert_eq!(
        kinds,
        [
            (WAVE, EditKind::Begin),
            (WAVE, EditKind::Perform),
            (WAVE, EditKind::End)
        ]
    );
}

#[test]
fn the_plugins_own_text_for_a_value_comes_back() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let _linked = linked(&mut manager, 1);
    manager.param_text(1, WAVE, 1.0);
    let (id, value, text) =
        wait_for(
            &mut manager,
            Duration::from_secs(10),
            |_, happened| match happened {
                Happened::Text { id, value, text } => Ok((id, value, text)),
                other => Err(other),
            },
        );
    assert_eq!((id, value, text.as_str()), (WAVE, 1.0, "Saw"));
}

#[test]
fn controllers_reach_a_plugin_that_maps_them_and_the_ones_it_does_not_are_told() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    // Controller 7 (volume) is the test plugin's level; 127 is all the way up.
    let events = [
        note_on(0, 69),
        Event {
            offset: 1,
            msg: Msg::controller(0, 7, 127),
        },
        // Controller 1 maps to nothing.
        Event {
            offset: 2,
            msg: Msg::controller(0, 1, 127),
        },
        note_off(63, 69),
    ];
    assert_eq!(loudness(&mut linked.link, &events), 1.0);
    let ignored = wait_for(
        &mut manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::Edits(t) if t.ignored > 0 => Ok(t.ignored),
            other => Err(other),
        },
    );
    assert_eq!(ignored, 1);
}

#[test]
fn the_plugins_state_is_read_on_request_and_a_reload_can_start_from_it() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    manager.set_param(1, LEVEL, 0.25);
    // Wait until the plugin has it, then read the state it would save.
    for _ in 0..200 {
        if loudness(&mut linked.link, &held()) == 0.25 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    manager.capture(1);
    let state = wait_for(
        &mut manager,
        Duration::from_secs(10),
        |_, happened| match happened {
            Happened::State(state) => Ok(state),
            other => Err(other),
        },
    )
    .expect("the state was read");
    assert!(!state.is_empty());

    // A host started from that state plays at the level that was set.
    assert!(manager.reload_from(1, Some(state), Vec::new()).unwrap());
    let mut fresh = linked_again(&mut manager, 1);
    assert_eq!(loudness(&mut fresh.link, &held()), 0.25);
    // And a device that is not there is not reloaded.
    assert!(!manager.reload_from(99, None, Vec::new()).unwrap());
}

/// The next link `device` gets, after its host was started again.
fn linked_again(manager: &mut Manager, device: u64) -> Box<lumen_hosting::Linked> {
    support::linked(manager, device)
}

#[test]
fn a_state_that_cannot_be_read_in_time_is_a_failure_not_a_wait() {
    let mut manager = manager();
    manager.want(1, instrument("state_hang")).unwrap();
    let _linked = linked(&mut manager, 1);
    manager.capture(1);
    let started = std::time::Instant::now();
    let state = wait_for(
        &mut manager,
        Duration::from_secs(15),
        |_, happened| match happened {
            Happened::State(state) => Ok(state),
            other => Err(other),
        },
    );
    assert!(state.is_err(), "a hung state read must fail");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "the answer came late: {:?}",
        started.elapsed()
    );
}
