//! Parameters through a real host process: the list with values, flags and
//! groups, the plugin's own text, values set from Lumen, edits made in the
//! plugin's own window, and what the processor reports.

mod support;

use lumen_plugin_host::protocol::{
    EditKind, FLAG_CAN_AUTOMATE, FLAG_HIDDEN, FLAG_READ_ONLY, ParamQueue,
};
use lumen_test_plugin::params::{LEVEL, WAVE};
use support::{SLOW, instrument, note_off, note_on};

/// A held note over a whole block, at the plugin's level.
fn play(host: &mut lumen_plugin_host::client::HostProcess, params: Vec<ParamQueue>) -> Vec<f32> {
    let block = host
        .process(
            64,
            vec![],
            vec![note_on(0, 69), note_off(63, 69)],
            params,
            SLOW,
        )
        .expect("process");
    block.output[0].clone()
}

#[test]
fn the_list_carries_values_flags_and_groups() {
    let mut host = instrument("many_params=30");
    let (total, list) = host.parameters(SLOW).unwrap();
    assert_eq!(total, 32);
    assert_eq!(list.len(), 32);

    // The plugin's own parameters first, with their current values.
    assert_eq!(list[0].title, "Level");
    assert_eq!(list[0].value, 0.5);
    assert!(list[0].can_automate());
    assert_eq!(list[0].unit, 0);

    // The extras: ids from 100, spread over four groups, some hidden and
    // some read-only.
    let extra = |k: usize| &list[2 + k];
    assert_eq!(extra(0).id, 100);
    assert_eq!(extra(0).unit, 1);
    assert_eq!(extra(5).unit, 2);
    assert!(extra(3).hidden() && extra(3).flags & FLAG_HIDDEN != 0);
    assert!(!extra(3).can_automate());
    assert!(extra(5).read_only() && extra(5).flags & FLAG_READ_ONLY != 0);
    assert!(extra(1).flags & FLAG_CAN_AUTOMATE != 0);

    let units = host.units(SLOW).unwrap();
    assert_eq!(units.len(), 5);
    assert_eq!((units[0].id, units[0].parent), (0, -1));
    assert_eq!((units[2].id, units[2].parent), (2, 0));
    assert_eq!(units[2].name, "Group 2");
}

#[test]
fn a_plugin_with_no_groups_lists_none() {
    let mut host = instrument("");
    assert!(host.units(SLOW).unwrap().is_empty());
}

#[test]
fn the_plugins_own_text_comes_for_any_value() {
    let mut host = instrument("many_params=3");
    assert_eq!(host.param_text(WAVE, 0.0, SLOW).unwrap(), "Square");
    assert_eq!(host.param_text(WAVE, 1.0, SLOW).unwrap(), "Saw");
    assert_eq!(host.param_text(101, 0.25, SLOW).unwrap(), "25.0 %");
    // A value outside 0 to 1 is clamped, not passed on.
    assert_eq!(host.param_text(101, 3.0, SLOW).unwrap(), "100.0 %");
    // A parameter the plugin does not have has no text.
    assert!(host.param_text(9_999, 0.5, SLOW).is_err());
    // And NaN is never passed to a plugin.
    assert!(host.param_text(101, f64::NAN, SLOW).is_err());
}

#[test]
fn a_value_set_from_lumen_is_heard_in_the_next_block_and_told_to_the_controller() {
    let mut host = instrument("");
    let loud = play(&mut host, vec![]);
    // The default level is half.
    assert!(loud[1..60].iter().all(|&s| s.abs() == 0.5), "{loud:?}");

    host.set_param(LEVEL, 0.25, SLOW).unwrap();
    let quiet = play(&mut host, vec![]);
    assert!(quiet[1..60].iter().all(|&s| s.abs() == 0.25), "{quiet:?}");

    // The controller holds the new value too (a plugin window shows it), and
    // Lumen's own change is not reported back as an edit.
    let (_, list) = host.parameters(SLOW).unwrap();
    assert_eq!(list[0].value, 0.25);
    let taken = host.take_edits(SLOW).unwrap();
    let (edits, dropped) = (taken.list, taken.dropped);
    assert!(edits.is_empty() && dropped == 0, "{edits:?}");
}

#[test]
fn a_value_set_outside_the_range_or_not_a_number_is_made_safe() {
    let mut host = instrument("");
    host.set_param(LEVEL, 5.0, SLOW).unwrap();
    let block = play(&mut host, vec![]);
    assert!(block[1..60].iter().all(|&s| s.abs() == 1.0));
    // NaN is dropped: the level stays where it was.
    host.set_param(LEVEL, f64::NAN, SLOW).unwrap();
    let block = play(&mut host, vec![]);
    assert!(block[1..60].iter().all(|&s| s.abs() == 1.0));
}

#[test]
fn what_the_plugin_does_in_its_window_reaches_lumen_in_order_and_the_processor() {
    // With `window_edits` moving the level also moves the wave, as a gesture
    // the plugin makes itself: begin, a value, end.
    let mut host = instrument("window_edits");
    host.set_param(LEVEL, 1.0, SLOW).unwrap();
    let taken = host.take_edits(SLOW).unwrap();
    let (edits, dropped) = (taken.list, taken.dropped);
    assert_eq!(dropped, 0);
    let kinds: Vec<_> = edits.iter().map(|e| (e.id, e.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            (WAVE, EditKind::Begin),
            (WAVE, EditKind::Perform),
            (WAVE, EditKind::End)
        ]
    );
    assert_eq!(edits[1].value, 1.0);
    // A second call has nothing left.
    assert!(host.take_edits(SLOW).unwrap().list.is_empty());

    // The processor heard it: a saw ramps, where a square would only be ±1.
    let block = play(&mut host, vec![]);
    let mut distinct: Vec<u32> = block[1..60].iter().map(|s| s.to_bits()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(distinct.len() > 3, "the wave did not change: {block:?}");
}

#[test]
fn without_that_edit_the_wave_stays_a_square() {
    let mut host = instrument("");
    host.set_param(LEVEL, 1.0, SLOW).unwrap();
    let block = play(&mut host, vec![]);
    assert!(block[1..60].iter().all(|&s| s.abs() == 1.0));
}

#[test]
fn a_value_that_rode_in_a_block_is_told_to_the_controller_later() {
    let mut host = instrument("");
    play(
        &mut host,
        vec![ParamQueue {
            id: LEVEL,
            points: vec![(0, 0.9), (10, 0.75)],
        }],
    );
    // The controller has not heard yet: the control thread tells it.
    host.take_edits(SLOW).unwrap();
    let (_, list) = host.parameters(SLOW).unwrap();
    assert_eq!(list[0].value, 0.75);
}

#[test]
fn what_the_processor_reports_comes_as_output_edits() {
    // `reports_params` reports parameter 7 at 0.5 in every block.
    let mut host = instrument("reports_params");
    play(&mut host, vec![]);
    play(&mut host, vec![]);
    let edits = host.take_edits(SLOW).unwrap().list;
    // Two blocks, one value: the second replaces the first.
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!((edits[0].id, edits[0].kind), (7, EditKind::Output));
    assert_eq!(edits[0].value, 0.5);
}

fn controller(
    offset: u32,
    channel: i16,
    number: i16,
    value: f32,
) -> lumen_plugin_host::protocol::Event {
    lumen_plugin_host::protocol::Event {
        offset,
        kind: lumen_plugin_host::protocol::EventKind::Controller {
            channel,
            number,
            value,
        },
    }
}

fn play_with(
    host: &mut lumen_plugin_host::client::HostProcess,
    mut events: Vec<lumen_plugin_host::protocol::Event>,
) -> Vec<f32> {
    events.insert(0, note_on(0, 69));
    events.push(note_off(63, 69));
    host.process(64, vec![], events, vec![], SLOW)
        .expect("process")
        .output[0]
        .clone()
}

#[test]
fn a_controller_the_plugin_maps_changes_its_parameter_at_the_event() {
    let mut host = instrument("");
    // Controller 7 is the level: half until sample 20, a quarter after it.
    let out = play_with(&mut host, vec![controller(20, 0, 7, 0.25)]);
    assert!(out[1..20].iter().all(|&s| s.abs() == 0.5), "{out:?}");
    assert!(out[21..60].iter().all(|&s| s.abs() == 0.25), "{out:?}");
    assert_eq!(host.take_edits(SLOW).unwrap().ignored, 0);
}

#[test]
fn several_controller_events_in_a_block_keep_their_order() {
    let mut host = instrument("");
    let out = play_with(
        &mut host,
        vec![
            controller(10, 0, 7, 1.0),
            controller(30, 0, 7, 0.25),
            controller(50, 0, 7, 0.75),
        ],
    );
    assert!(out[1..10].iter().all(|&s| s.abs() == 0.5), "{out:?}");
    assert!(out[11..30].iter().all(|&s| s.abs() == 1.0), "{out:?}");
    assert!(out[31..50].iter().all(|&s| s.abs() == 0.25), "{out:?}");
    assert!(out[51..60].iter().all(|&s| s.abs() == 0.75), "{out:?}");
}

#[test]
fn pitch_bend_goes_through_the_mapping_too() {
    use lumen_plugin_host::protocol::{Event, EventKind};
    let mut host = instrument("");
    // The test plugin maps bend to the wave: all the way up is a saw.
    let bend = Event {
        offset: 0,
        kind: EventKind::PitchBend {
            channel: 0,
            value: 1.0,
        },
    };
    let out = play_with(&mut host, vec![bend]);
    let mut distinct: Vec<u32> = out[2..60].iter().map(|s| s.to_bits()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(distinct.len() > 3, "still a square: {out:?}");
}

#[test]
fn a_controller_nothing_maps_is_counted_and_changes_nothing() {
    use lumen_plugin_host::protocol::{Event, EventKind};
    let mut host = instrument("");
    let pressure = Event {
        offset: 5,
        kind: EventKind::Pressure {
            channel: 0,
            value: 1.0,
        },
    };
    // Controller 1 on channel 0, and controller 7 on channel 3: unmapped.
    let out = play_with(
        &mut host,
        vec![pressure, controller(8, 0, 1, 1.0), controller(9, 3, 7, 1.0)],
    );
    assert!(out[1..60].iter().all(|&s| s.abs() == 0.5), "{out:?}");
    assert_eq!(host.take_edits(SLOW).unwrap().ignored, 3);
    assert_eq!(host.take_edits(SLOW).unwrap().ignored, 0, "counted once");
}

#[test]
fn a_controller_number_out_of_range_is_ignored_not_asked() {
    let mut host = instrument("");
    let out = play_with(
        &mut host,
        vec![
            controller(8, 0, 500, 1.0),
            controller(9, -1, 7, 1.0),
            controller(9, 16, 7, 1.0),
        ],
    );
    assert!(out[1..60].iter().all(|&s| s.abs() == 0.5));
    assert_eq!(host.take_edits(SLOW).unwrap().ignored, 3);
}
