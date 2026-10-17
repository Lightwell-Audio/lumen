//! What real plugins expect of a host, simulated by the test plugin's strict
//! behaviours (`needs_message`, `aux_buses`): a host that does not provide
//! them takes the plugin down, as it does with real plugins on Windows.

mod support;

use lumen_plugin_host::client::ExitKind;
use lumen_plugin_host::protocol::ParamQueue;
use lumen_test_plugin::params::LEVEL;
use support::{SLOW, instrument, note_off, note_on};

/// The plugin's level at full, so a note is heard at full scale.
fn full() -> Vec<ParamQueue> {
    vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    }]
}

#[test]
fn a_plugin_that_asks_the_host_for_a_message_object_gets_one() {
    // `instrument` loads and configures; the plugin creates a message in
    // `initialize` and crashes the process when the host gives it none.
    let mut host = instrument("needs_message");
    let block = host
        .process(
            64,
            vec![],
            vec![note_on(0, 69), note_off(40, 69)],
            vec![],
            SLOW,
        )
        .expect("process");
    assert_eq!(block.status, 0);
    assert_eq!(host.quit(SLOW), ExitKind::Clean);
}

#[test]
fn a_plugin_with_auxiliary_outputs_is_given_every_bus_each_block_with_a_buffer_list_for_each() {
    let mut host = instrument("aux_buses");
    let block = host
        .process(
            64,
            vec![],
            vec![note_on(10, 69), note_off(40, 69)],
            full(),
            SLOW,
        )
        .expect("process");
    assert_eq!(block.status, 0);
    // The main bus is still the one that is heard.
    assert!(
        block.output[0][10..40]
            .iter()
            .all(|&s| s == 1.0 || s == -1.0)
    );
    assert_eq!(host.quit(SLOW), ExitKind::Clean);
}

#[test]
fn a_plugin_that_reports_a_parameter_change_is_given_a_queue_to_put_it_in() {
    let mut host = instrument("reports_params");
    let block = host
        .process(64, vec![], vec![note_on(0, 69)], full(), SLOW)
        .expect("process");
    assert_eq!(block.status, 0);
    assert_eq!(host.quit(SLOW), ExitKind::Clean);
}

#[test]
fn the_host_says_how_long_its_first_blocks_took_and_lumen_keeps_it() {
    use lumen_plugin_host::shm::{Layout, Poll, Wake};
    let mut host = instrument("");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    for _ in 0..3 {
        link.submit(generation, 64, &[], &[], &[]).expect("submit");
        let soon = std::time::Instant::now() + std::time::Duration::from_secs(5);
        assert!(matches!(link.wait(soon), Poll::Done(_)));
    }
    // The reader thread picks the host's lines up from its pipe.
    let log = host.log();
    let started = std::time::Instant::now();
    while log.lines().len() < 2 && started.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let lines = log.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("block 1 (64 frames) took ")),
        "{lines:?}"
    );
    assert!(log.crash().is_none());
}
