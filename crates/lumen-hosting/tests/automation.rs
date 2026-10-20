//! A plugin parameter driven by a lane, through the engine's link to a real
//! host process (M7 slice A3, ADR-0084): the points of a block land on their
//! sample in the plugin, and what does not fit the region is counted.

mod support;

use lumen_core::midi::{Event, Msg};
use lumen_devices::{MOST_POINTS, MOST_QUEUES, ParamBlock, Reply};
use lumen_hosting::Manager;
use support::*;

const FRAMES: usize = 64;
/// The parameter both test plugins give their level to.
const LEVEL: u32 = lumen_test_plugin::params::LEVEL;

fn manager() -> Manager {
    Manager::new(|| {})
}

/// A block of `FRAMES` in which parameter `id` takes each `(offset, value)`.
fn changes(id: u32, points: &[(u32, f64)]) -> ParamBlock {
    let mut block = ParamBlock::new();
    block.start(FRAMES);
    assert!(block.open(id));
    for &(offset, value) in points {
        assert!(block.point(offset, value));
    }
    block
}

fn note() -> Event {
    Event {
        offset: 0,
        msg: Msg::note_on(0, 60, 127),
    }
}

/// One block of the test effect fed a constant, with `params`.
fn through_effect(params: &ParamBlock) -> Vec<f32> {
    let mut manager = manager();
    manager.want(1, effect("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    let steady = vec![0.5_f32; FRAMES];
    let (left, _, reply) = play_with(&mut linked.link, FRAMES, &[&steady, &steady], &[], params);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    left
}

/// The first block of the test instrument holding a note, with `params`.
fn through_instrument(params: &ParamBlock) -> Vec<f32> {
    let mut manager = manager();
    manager.want(2, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 2);
    let (left, _, reply) = play_with(&mut linked.link, FRAMES, &[], &[note()], params);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    left
}

#[test]
fn an_effects_parameter_changes_on_the_sample_the_block_says() {
    let (quiet, loud) = (0.2, 0.9);
    let before = through_effect(&changes(LEVEL, &[(0, quiet)]));
    let after = through_effect(&changes(LEVEL, &[(0, loud)]));
    assert_ne!(before[0], after[0], "the two levels differ");
    for step in [1usize, 17, 31, 63] {
        let stepped = through_effect(&changes(
            LEVEL,
            &[(0, quiet), (step as u32 - 1, quiet), (step as u32, loud)],
        ));
        assert_eq!(&stepped[..step], &before[..step], "before sample {step}");
        assert_eq!(&stepped[step..], &after[step..], "from sample {step}");
    }
}

#[test]
fn an_instruments_parameter_changes_on_the_sample_the_block_says() {
    let (quiet, loud) = (0.25, 1.0);
    let before = through_instrument(&changes(LEVEL, &[(0, quiet)]));
    let after = through_instrument(&changes(LEVEL, &[(0, loud)]));
    assert!(before.iter().any(|&s| s != 0.0), "the note sounds");
    assert_ne!(before, after);
    let step = 23;
    let stepped = through_instrument(&changes(
        LEVEL,
        &[(0, quiet), (step as u32 - 1, quiet), (step as u32, loud)],
    ));
    assert_eq!(&stepped[..step], &before[..step]);
    assert_eq!(&stepped[step..], &after[step..]);
}

#[test]
fn a_block_with_no_parameters_leaves_the_plugin_as_it_was() {
    let mut manager = manager();
    manager.want(1, effect("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    let steady = vec![0.5_f32; FRAMES];
    let set = changes(LEVEL, &[(0, 0.2)]);
    let (a, _, _) = play_with(&mut linked.link, FRAMES, &[&steady, &steady], &[], &set);
    // The plugin keeps the last value it was given.
    let (b, _, _) = play(&mut linked.link, FRAMES, &[&steady, &steady], &[]);
    assert_eq!(a, b);
}

#[test]
fn the_most_parameters_and_points_a_block_holds_fit_the_region_exactly() {
    let mut manager = manager();
    manager.want(1, effect("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    let steady = vec![0.5_f32; FRAMES];
    let mut block = ParamBlock::new();
    block.start(FRAMES);
    let per = (MOST_POINTS / MOST_QUEUES) as u32;
    for id in 0..MOST_QUEUES as u32 {
        assert!(block.open(id + 100));
        for at in 0..per {
            assert!(block.point(at, 0.5));
        }
    }
    let (_, _, reply) = play_with(&mut linked.link, FRAMES, &[&steady, &steady], &[], &block);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    let counts = linked.link.counts();
    assert_eq!(counts.dropped_params(), 0, "all of them fit");
    assert_eq!(counts.unfit_blocks(), 0, "and the region took the block");
}
