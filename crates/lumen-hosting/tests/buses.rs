//! Audio buses through the engine's link (M6 slice 6b): the buses a plugin
//! declares come with the link, the auxiliary outputs turned on are read per
//! bus pair, a sidechain input the engine gives nothing for is silent, and a
//! plugin set up again with other buses gets a link that matches.

mod support;

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Hosted, Reply};
use lumen_hosting::{Aux, Manager};
use support::*;

const FRAMES: usize = 64;

fn manager() -> Manager {
    Manager::new(|| {})
}

fn on(key: u8) -> Event {
    Event {
        offset: 0,
        msg: Msg::note_on(0, key, 127),
    }
}

/// The plugin's level at full, so a note is heard at full scale.
fn full(mut start: lumen_hosting::Start) -> lumen_hosting::Start {
    start.values = vec![(lumen_test_plugin::params::LEVEL, 1.0)];
    start
}

/// What one note sounds like on the main bus of a plain instrument.
fn alone(key: u8) -> Vec<f32> {
    let mut manager = manager();
    manager.want(1, full(instrument(""))).unwrap();
    let mut linked = linked(&mut manager, 1);
    block(&mut linked.link, &[key]).0
}

/// A block of held notes: the main pair, and the link to read the others.
fn block(link: &mut dyn Hosted, keys: &[u8]) -> (Vec<f32>, Vec<f32>) {
    let events: Vec<Event> = keys.iter().map(|&k| on(k)).collect();
    let (left, right, reply) = play(link, FRAMES, &[], &events);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    (left, right)
}

#[test]
fn the_link_carries_the_buses_the_plugin_declares_and_each_output_turned_on() {
    let mut manager = manager();
    let mut start = instrument("multi_out=3");
    start.aux = Aux {
        inputs: 0,
        outputs: 0b110,
    };
    manager.want(1, full(start)).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(linked.buses.1.len(), 3);
    assert_eq!(linked.buses.1[1].name, "Output 2");
    assert_eq!(linked.aux.outputs, 0b110);
    assert_eq!(linked.link.pairs().len(), 3);

    let (main, _) = block(&mut linked.link, &[60, 61, 62]);
    assert_eq!(main, alone(60), "pitch 60 picks the main bus");
    for (pair, key) in [(1, 61), (2, 62)] {
        let (mut left, mut right) = (vec![0.0; FRAMES], vec![0.0; FRAMES]);
        assert!(linked.link.output(pair, &mut left, &mut right));
        assert_eq!(left, alone(key), "bus pair {pair}");
        assert_eq!(right, left);
    }
    let (mut left, mut right) = (vec![0.0; FRAMES], vec![0.0; FRAMES]);
    assert!(
        !linked.link.output(3, &mut left, &mut right),
        "no fourth pair"
    );
}

#[test]
fn a_sidechain_the_engine_gives_nothing_for_is_silence_to_the_plugin() {
    let mut manager = manager();
    let mut start = effect("sidechain");
    start.aux = Aux {
        inputs: 0b10,
        outputs: 0,
    };
    manager.want(2, start).unwrap();
    let mut linked = linked(&mut manager, 2);
    assert_eq!(linked.buses.0.len(), 2);
    let steady = vec![0.5_f32; FRAMES];
    // Two channels given, four carried: the sidechain pair is silence.
    let (plain, _, reply) = play(&mut linked.link, FRAMES, &[&steady, &steady], &[]);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    assert!(
        plain.iter().all(|&s| s == plain[0] && s != 0.0),
        "{plain:?}"
    );
    // And with the sidechain given, the loud sample is silenced.
    let mut impulse = vec![0.0_f32; FRAMES];
    impulse[10] = 1.0;
    let silent = vec![0.0_f32; FRAMES];
    let (ducked, _, _) = play(
        &mut linked.link,
        FRAMES,
        &[&steady, &steady, &impulse, &silent],
        &[],
    );
    assert_eq!(ducked[10], 0.0);
    assert_eq!(ducked[9], plain[9]);
}

#[test]
fn a_plugin_set_up_again_with_other_buses_gets_a_link_that_matches() {
    let mut manager = manager();
    manager.want(1, instrument("multi_out=3")).unwrap();
    let first = linked(&mut manager, 1);
    assert_eq!(first.link.pairs().len(), 1, "no auxiliary output at first");
    manager.relink_with(
        1,
        48_000.0,
        128,
        Aux {
            inputs: 0,
            outputs: 0b010,
        },
    );
    let second = linked(&mut manager, 1);
    assert_eq!(second.aux.outputs, 0b010);
    assert_eq!(second.link.pairs().len(), 2);
    // A plain relink keeps the buses it had.
    manager.relink(1, 48_000.0, 128);
    let third = linked(&mut manager, 1);
    assert_eq!(third.link.pairs().len(), 2);
}
