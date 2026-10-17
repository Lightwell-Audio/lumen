//! The shared-memory audio path against real host processes running the
//! deterministic test plugins: the same sound as the pipe path, in every
//! wake-up mode, and the faults that matter to a block in flight.

mod support;

use std::time::{Duration, Instant};

use lumen_plugin_host::client::{ExitKind, HostProcess};
use lumen_plugin_host::protocol::{Event, ParamQueue};
use lumen_plugin_host::shm::{BlockLink, Layout, Outcome, Poll, SubmitError, Waiting, Wake};
use lumen_test_plugin::params::LEVEL;
use support::{SLOW, effect, instrument, note_off, note_on, plugin, spawn};

fn level(value: f64) -> Vec<ParamQueue> {
    vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, value)],
    }]
}

fn modes() -> Vec<(&'static str, Wake)> {
    vec![
        ("spin/spin", Wake::SPIN),
        ("event/event", Wake::EVENT),
        (
            "hybrid/hybrid",
            Wake {
                host: Waiting::Hybrid(Duration::from_micros(300)),
                lumen: Waiting::Hybrid(Duration::from_micros(300)),
            },
        ),
        (
            "host events, lumen polls",
            Wake {
                host: Waiting::Event,
                lumen: Waiting::Spin,
            },
        ),
    ]
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// A block's output, taken from the link once it has been answered.
fn take(link: &mut BlockLink, channels: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| {
            let mut out = vec![0.0; frames];
            link.copy_output(c, &mut out);
            out
        })
        .collect()
}

fn notes(block: u32) -> Vec<Event> {
    // A different note pattern every block, so a stale or reordered block
    // cannot look right by accident.
    let pitch = 48 + (block % 24) as i16;
    vec![note_on(block % 32, pitch), note_off(40 + block % 20, pitch)]
}

#[test]
fn blocks_sound_exactly_as_they_do_over_the_pipe_in_every_wake_mode() {
    for (name, wake) in modes() {
        let mut piped = instrument("");
        let mut shared = instrument("");
        let mut link = shared
            .attach(Layout::new(512, 0, 2), wake, SLOW)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let generation = shared.generation();
        for block in 0..300_u32 {
            let params = level(f64::from(block % 11) / 10.0);
            let expected = piped
                .process(64, vec![], notes(block), params.clone(), SLOW)
                .expect("pipe");
            link.submit(generation, 64, &[], &notes(block), &params)
                .unwrap_or_else(|e| panic!("{name}: block {block}: {e}"));
            let Poll::Done(answer) = link.wait(soon()) else {
                panic!("{name}: block {block} was not answered");
            };
            assert_eq!(answer.outcome, Outcome::Done, "{name}");
            assert_eq!(answer.status, expected.status, "{name}");
            assert_eq!(answer.latency, expected.latency, "{name}");
            assert_eq!(
                take(&mut link, 2, 64),
                expected.output,
                "{name}: block {block}"
            );
        }
        assert_eq!((link.stale(), link.invalid()), (0, 0), "{name}");
        assert_eq!(shared.quit(SLOW), ExitKind::Clean, "{name}");
    }
}

#[test]
fn an_effect_processes_the_audio_it_is_given() {
    let mut piped = effect("");
    let mut shared = effect("");
    let mut link = shared
        .attach(Layout::new(512, 2, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = shared.generation();
    let left: Vec<f32> = (0..128).map(|i| (i as f32 * 0.05).sin()).collect();
    let right: Vec<f32> = (0..128).map(|i| (i as f32 * 0.03).cos()).collect();
    for _ in 0..20 {
        let expected = piped
            .process(128, vec![left.clone(), right.clone()], vec![], vec![], SLOW)
            .expect("pipe");
        link.submit(generation, 128, &[&left, &right], &[], &[])
            .expect("submit");
        assert!(matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Done));
        assert_eq!(take(&mut link, 2, 128), expected.output);
    }
}

#[test]
fn a_block_of_an_old_generation_is_stale_and_not_processed() {
    let mut host = instrument("");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let old = host.generation();
    // A new configuration while the link is up.
    host.configure(48_000.0, 512, 0, 2, SLOW)
        .expect("configure");
    let new = host.generation();
    assert_ne!(old, new);

    link.submit(old, 64, &[], &notes(1), &level(1.0)).unwrap();
    let Poll::Done(answer) = link.wait(soon()) else {
        panic!("no answer");
    };
    assert_eq!(answer.outcome, Outcome::Stale);
    assert_eq!(link.stale(), 1);
    // The plugin never saw it: the current generation's first block is the
    // plugin's first block, and a note on the stale block would still be
    // sounding if it had.
    link.submit(new, 64, &[], &[], &[]).unwrap();
    assert!(matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Done));
    assert!(take(&mut link, 2, 64).iter().flatten().all(|&s| s == 0.0));
}

#[test]
fn a_block_before_the_plugin_is_configured_is_not_ready() {
    let mut host = spawn("");
    host.load(plugin(), None, SLOW).expect("load");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    link.submit(1, 64, &[], &[], &[]).unwrap();
    assert!(matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::NotReady));
}

#[test]
fn a_layout_that_differs_from_the_format_is_invalid() {
    let mut host = effect("");
    // The effect is stereo in and out; this layout has no input.
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    link.submit(generation, 64, &[], &[], &[]).unwrap();
    assert!(matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Invalid));
}

#[test]
fn a_block_longer_than_the_plugin_was_promised_is_played_in_pieces_exactly() {
    // A plugin is told the device's block (ADR-0075); a driver that delivers
    // more must not hand it more. The host gives it pieces it accepts, each
    // with its notes and parameter points, and the result is the same as one
    // call to a plugin promised the whole block.
    let events = [note_on(100, 60), note_off(700, 60), note_on(900, 64)];
    let params = vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, 0.5), (600, 1.0)],
    }];
    let mut heard = Vec::new();
    for promised in [512, 2048] {
        let mut host = spawn("");
        host.load(plugin(), None, SLOW).expect("load");
        host.configure(48_000.0, promised, 0, 2, SLOW)
            .expect("configure");
        let mut link = host
            .attach(Layout::new(2048, 0, 2), Wake::EVENT, SLOW)
            .expect("attach");
        let generation = host.generation();
        link.submit(generation, 1024, &[], &events, &params)
            .unwrap();
        assert!(
            matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Done),
            "promised {promised}"
        );
        heard.push(take(&mut link, 2, 1024));
    }
    assert!(heard[1][0].iter().any(|&s| s != 0.0), "the notes sound");
    assert_eq!(heard[0], heard[1]);
}

#[test]
fn output_that_is_not_finite_is_silence_and_counted() {
    let mut host = instrument("process_nan");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    link.submit(generation, 64, &[], &notes(0), &level(1.0))
        .unwrap();
    let Poll::Done(answer) = link.wait(soon()) else {
        panic!("no answer");
    };
    assert_eq!(answer.outcome, Outcome::Done);
    assert!(answer.repaired > 0);
    assert!(
        take(&mut link, 2, 64)
            .iter()
            .flatten()
            .all(|s| s.is_finite())
    );
}

#[test]
fn a_plugin_error_is_an_outcome_and_the_host_goes_on() {
    let mut host = instrument("process_error@1");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    for block in 0..3 {
        link.submit(generation, 64, &[], &notes(block), &[])
            .unwrap();
        assert!(matches!(link.wait(soon()), Poll::Done(_)), "block {block}");
    }
    assert!(host.ended().is_none());
}

#[test]
fn a_slow_block_is_given_up_on_and_its_late_answer_dropped() {
    // Every block takes 300 ms (so a thread a busy machine keeps off the CPU
    // still finds the first unanswered). Lumen gives each 5 ms.
    let mut host = instrument("process_slow=300000");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    link.submit(generation, 64, &[], &notes(0), &level(1.0))
        .unwrap();
    assert_eq!(
        link.wait(Instant::now() + Duration::from_millis(5)),
        Poll::Pending
    );
    assert_eq!(
        link.submit(generation, 64, &[], &[], &[]),
        Err(SubmitError::Busy)
    );
    // The late answer is stale; the link takes the next block once it has
    // come. Try until then, not after a fixed time.
    let started = Instant::now();
    loop {
        match link.submit(generation, 64, &[], &[], &[]) {
            Ok(_) => break,
            Err(SubmitError::Busy) => {
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "the late answer never came"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(other) => panic!("next block: {other:?}"),
        }
    }
    assert_eq!(link.stale(), 1);
    let Poll::Done(answer) = link.wait(soon()) else {
        panic!("no answer");
    };
    assert_eq!(answer.seq, 2);
    assert_eq!(answer.outcome, Outcome::Done);
}

#[test]
fn a_crash_in_processing_leaves_the_block_in_flight_and_the_host_dead() {
    let mut host = instrument("process_crash@2");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    for block in 0..2 {
        link.submit(generation, 64, &[], &notes(block), &[])
            .unwrap();
        assert!(matches!(link.wait(soon()), Poll::Done(_)));
    }
    link.submit(generation, 64, &[], &notes(2), &[]).unwrap();
    // The deadline holds although nobody will ever answer.
    let started = Instant::now();
    assert_eq!(
        link.wait(started + Duration::from_millis(50)),
        Poll::Pending
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    match host.wait_ended(Duration::from_secs(5)) {
        Some(ExitKind::Crashed(_)) => {}
        other => panic!("expected a crash, got {other:?}"),
    }
    assert_eq!(
        link.submit(generation, 64, &[], &[], &[]),
        Err(SubmitError::Busy)
    );
}

#[test]
fn a_hang_in_processing_is_a_missed_deadline_and_the_host_can_be_killed() {
    let mut host = instrument("process_hang@1");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    link.submit(generation, 64, &[], &notes(0), &[]).unwrap();
    assert!(matches!(link.wait(soon()), Poll::Done(_)));
    link.submit(generation, 64, &[], &notes(1), &[]).unwrap();
    for _ in 0..3 {
        assert_eq!(
            link.wait(Instant::now() + Duration::from_millis(20)),
            Poll::Pending
        );
    }
    assert!(host.ended().is_none());
    host.kill();
    assert_eq!(host.ended(), Some(ExitKind::Killed));
}

#[test]
fn attaching_again_replaces_the_link() {
    let mut host = instrument("");
    let mut first = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    first.submit(generation, 64, &[], &notes(0), &[]).unwrap();
    assert!(matches!(first.wait(soon()), Poll::Done(_)));

    let mut second = host
        .attach(Layout::new(1024, 0, 2), Wake::SPIN, SLOW)
        .expect("attach again");
    second.submit(generation, 64, &[], &notes(1), &[]).unwrap();
    assert!(matches!(second.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Done));
    // The first link's host loop has ended: it answers nothing.
    first.submit(generation, 64, &[], &notes(2), &[]).unwrap();
    assert_eq!(
        first.wait(Instant::now() + Duration::from_millis(100)),
        Poll::Pending
    );
}

#[test]
fn many_hosts_run_side_by_side() {
    let mut hosts: Vec<HostProcess> = (0..6).map(|_| instrument("")).collect();
    let mut links: Vec<BlockLink> = hosts
        .iter_mut()
        .map(|h| {
            h.attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
                .expect("attach")
        })
        .collect();
    let generation = hosts[0].generation();
    for block in 0..50 {
        for link in &mut links {
            link.submit(generation, 64, &[], &notes(block), &level(1.0))
                .unwrap();
        }
        for link in &mut links {
            assert!(matches!(link.wait(soon()), Poll::Done(a) if a.outcome == Outcome::Done));
        }
    }
    // The same notes into the same plugin: the same sound from every host.
    let first = take(&mut links[0], 2, 64);
    for link in &mut links[1..] {
        assert_eq!(take(link, 2, 64), first);
    }
}

#[test]
fn a_host_does_not_outlive_its_handle_with_the_audio_path_attached() {
    let mut host = instrument("");
    let _link = host
        .attach(Layout::new(512, 0, 2), Wake::SPIN, SLOW)
        .expect("attach");
    let pid = host.id();
    drop(host);
    assert!(!support::exists(pid), "host {pid} is still running");
}

#[test]
fn quitting_with_the_audio_path_attached_is_clean() {
    for (name, wake) in modes() {
        let mut host = instrument("");
        let _link = host
            .attach(Layout::new(512, 0, 2), wake, SLOW)
            .expect("attach");
        assert_eq!(host.quit(SLOW), ExitKind::Clean, "{name}");
    }
}

#[test]
fn a_bad_layout_is_refused_by_the_client_and_the_host_alike() {
    let mut host = instrument("");
    assert!(
        host.attach(Layout::new(0, 0, 2), Wake::EVENT, SLOW)
            .is_err()
    );
    // The host is fine and can still be attached.
    assert!(
        host.attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
            .is_ok()
    );
}

/// MT-VST-4 run 1 (RP-6): a slow controller call (here a state that never
/// comes) held the lock the audio thread takes for every block, and the
/// plugin was given up on for missing a second of blocks. Controller calls
/// are made without it, so the blocks go on.
#[test]
fn blocks_go_on_while_the_main_thread_waits_on_the_plugin() {
    let mut host = instrument("state_hang");
    let mut link = host
        .attach(Layout::new(512, 0, 2), Wake::EVENT, SLOW)
        .expect("attach");
    let generation = host.generation();
    let asking = std::thread::spawn(move || {
        let state = host.state(Duration::from_secs(2));
        (host, state)
    });
    // The state call is under way on the host's main thread now.
    std::thread::sleep(Duration::from_millis(200));
    for block in 0..100_u32 {
        link.submit(generation, 64, &[], &notes(block), &[])
            .unwrap_or_else(|e| panic!("block {block}: {e}"));
        let Poll::Done(answer) = link.wait(Instant::now() + Duration::from_millis(500)) else {
            panic!("block {block} waited for the state call");
        };
        assert_eq!(answer.outcome, Outcome::Done, "block {block}");
    }
    let (_host, state) = asking.join().expect("the asking thread");
    assert!(state.is_err(), "the state never came");
}
