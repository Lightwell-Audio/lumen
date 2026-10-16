//! A plugin that misbehaves, in a host process of its own: what Lumen sees,
//! and that nothing else is affected.

mod support;

use std::time::{Duration, Instant};

use lumen_plugin_host::client::{ExitKind, HostError};
use lumen_plugin_host::protocol::{FailureKind, MAX_PARAMETERS};
use support::{SLOW, instrument, note_on, plugin, spawn};

const BLOCK: Duration = Duration::from_millis(500);

fn play(
    host: &mut lumen_plugin_host::client::HostProcess,
    within: Duration,
) -> Result<(), HostError> {
    host.process(32, vec![], vec![note_on(0, 60)], vec![], within)
        .map(|_| ())
}

#[test]
fn a_crash_while_processing_is_reported_as_one() {
    let mut host = instrument("process_crash@2");
    play(&mut host, SLOW).expect("block 0");
    play(&mut host, SLOW).expect("block 1");
    let err = play(&mut host, SLOW).unwrap_err();
    match err {
        HostError::Ended(ExitKind::Crashed(how)) => {
            #[cfg(unix)]
            assert!(how.starts_with("signal"), "{how}");
            let _ = how;
        }
        other => panic!("expected a crash, got {other:?}"),
    }
}

#[test]
fn a_crash_does_not_touch_another_host() {
    let mut healthy = instrument("");
    let mut doomed = instrument("process_crash@0");
    assert!(matches!(
        play(&mut doomed, SLOW),
        Err(HostError::Ended(ExitKind::Crashed(_)))
    ));
    for _ in 0..4 {
        play(&mut healthy, SLOW).expect("the other host carries on");
    }
}

#[test]
fn a_hang_is_a_timeout_and_the_host_can_be_killed() {
    let mut host = instrument("process_hang@1");
    play(&mut host, SLOW).expect("block 0");
    let start = Instant::now();
    assert_eq!(play(&mut host, BLOCK), Err(HostError::Timeout));
    let waited = start.elapsed();
    assert!(
        waited >= BLOCK && waited < BLOCK * 3,
        "the deadline held: {waited:?}"
    );
    assert_eq!(host.ended(), None, "still running, hung inside the plugin");
    host.kill();
    assert_eq!(host.ended(), Some(ExitKind::Killed));
}

#[test]
fn a_late_answer_is_dropped_not_taken_for_the_next_block() {
    // Every block takes 400 ms; the first is given 30 ms. The margin is wide
    // because a deadline cannot cut off an answer that is already waiting
    // when a busy machine lets the test thread run again: that thread would
    // have to be kept off the CPU for most of the 400 ms to see the first
    // block answered in time.
    let mut host = instrument("process_slow=400000");
    let first = host.process(
        32,
        vec![],
        vec![note_on(0, 60)],
        vec![],
        Duration::from_millis(30),
    );
    assert_eq!(first.unwrap_err(), HostError::Timeout);
    // The second block is a different length, so its answer cannot be
    // mistaken for the first's. It waits long enough for both answers; the
    // first arrives first, is recognised as stale, and is not returned.
    let second = host
        .process(16, vec![], vec![], vec![], Duration::from_secs(5))
        .expect("second block");
    assert_eq!(host.stale(), 1);
    assert_eq!(second.output[0].len(), 16);
}

#[test]
fn an_error_from_the_plugin_is_a_status_not_a_crash() {
    let mut host = instrument("process_error@1");
    host.process(32, vec![], vec![], vec![], SLOW).unwrap();
    let block = host.process(32, vec![], vec![], vec![], SLOW).unwrap();
    assert_ne!(block.status, 0);
    assert_eq!(host.ended(), None);
}

#[test]
fn non_finite_output_never_leaves_the_host() {
    let mut host = instrument("process_nan@0");
    let block = host
        .process(32, vec![], vec![note_on(0, 60)], vec![], SLOW)
        .unwrap();
    assert!(block.repaired > 0, "the host noticed");
    assert!(block.output.iter().flatten().all(|s| s.is_finite()));
}

#[test]
fn a_bus_the_plugin_reports_wrongly_is_refused_at_configuration() {
    let mut host = spawn("bad_buses");
    host.load(plugin(), None, SLOW).unwrap();
    let err = host.configure(48_000.0, 512, 0, 2, SLOW).unwrap_err();
    assert!(matches!(err, HostError::Failed { .. }), "{err:?}");
    // And no block is processed in the failed configuration.
    assert!(play(&mut host, SLOW).is_err());
}

#[test]
fn millions_of_parameters_are_listed_up_to_the_cap() {
    let mut host = spawn("bad_params");
    host.load(plugin(), None, SLOW).unwrap();
    let (total, list) = host.parameters(Duration::from_secs(30)).unwrap();
    assert!(total as usize > MAX_PARAMETERS, "{total}");
    assert_eq!(list.len(), MAX_PARAMETERS);
}

#[test]
fn state_that_fails_or_hangs_is_contained() {
    let mut host = instrument("state_fail");
    assert!(matches!(
        host.state(SLOW),
        Err(HostError::Failed {
            kind: FailureKind::Plugin,
            ..
        })
    ));
    play(&mut host, SLOW).expect("processing is unaffected");

    let mut host = instrument("state_hang");
    assert_eq!(host.state(BLOCK), Err(HostError::Timeout));
    host.kill();
}

#[test]
fn state_the_plugin_will_not_read_back_is_found_before_it_is_saved_over_good_state() {
    let mut host = instrument("state_garbage");
    let state = host.state(SLOW).expect("it writes something");
    // A save that trusted this would store state the plugin rejects.
    assert!(host.set_state(state, SLOW).is_err());
}

#[test]
fn a_factory_that_returns_nothing_or_crashes_is_a_load_failure() {
    let mut host = spawn("factory_null");
    let err = host.load(plugin(), None, SLOW).unwrap_err();
    assert!(matches!(
        err,
        HostError::Failed {
            kind: FailureKind::Load,
            ..
        }
    ));
    assert_eq!(host.ended(), None, "the host reported it and stayed up");

    let mut host = spawn("factory_crash");
    assert!(matches!(
        host.load(plugin(), None, SLOW),
        Err(HostError::Ended(ExitKind::Crashed(_)))
    ));

    let mut host = spawn("factory_hang");
    assert_eq!(host.load(plugin(), None, BLOCK), Err(HostError::Timeout));
    host.kill();
}

#[test]
fn a_missing_module_is_a_load_failure_that_leaves_the_host_usable() {
    let mut host = spawn("");
    let err = host
        .load(std::path::Path::new("/nonexistent/x.vst3"), None, SLOW)
        .unwrap_err();
    assert!(matches!(
        err,
        HostError::Failed {
            kind: FailureKind::Load,
            ..
        }
    ));
    host.load(plugin(), None, SLOW).expect("then a real one");
}
