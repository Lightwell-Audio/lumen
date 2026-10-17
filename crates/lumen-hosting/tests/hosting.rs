//! The hosting manager against real host processes running the test
//! plugins: sound sample for sample as the host makes it over the pipe, state
//! restored at load, every way a load can fail, a host that crashes or hangs
//! mid-session, and no process left behind.

mod support;

use std::time::{Duration, Instant};

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Guard, Hosted, Outcome, Refused, Reply};
use lumen_hosting::{Manager, Step};
use lumen_plugin_host::client::HostProcess;
use lumen_plugin_host::protocol::{Event as HostEvent, EventKind, ParamQueue};
use lumen_test_plugin::params::LEVEL;
use support::*;

const SLOW: Duration = Duration::from_secs(10);

fn manager() -> Manager {
    Manager::new(|| {})
}

/// The host's own answer for the same blocks, over the pipe.
fn pipe(instrument: bool) -> HostProcess {
    let mut host = HostProcess::spawn(&built().1, &[(FAULT, "")]).expect("start");
    let class = if instrument {
        instrument_class()
    } else {
        effect_class()
    };
    host.load(&built().0, Some(class), SLOW).expect("load");
    host.configure(RATE, 512, if instrument { 0 } else { 2 }, 2, SLOW)
        .expect("configure");
    host
}

fn host_notes(events: &[Event]) -> Vec<HostEvent> {
    events
        .iter()
        .map(|e| {
            let velocity = f32::from(e.msg.0[2]) / 127.0;
            let (channel, pitch) = (i16::from(e.msg.channel()), i16::from(e.msg.0[1]));
            HostEvent {
                offset: e.offset,
                kind: if e.msg.0[0] >> 4 == 9 {
                    EventKind::NoteOn {
                        channel,
                        pitch,
                        velocity,
                    }
                } else {
                    EventKind::NoteOff {
                        channel,
                        pitch,
                        velocity,
                    }
                },
            }
        })
        .collect()
}

fn pattern(block: u32) -> Vec<Event> {
    let key = 48 + (block % 24) as u8;
    vec![note_on(block % 32, key), note_off(40 + block % 20, key)]
}

#[test]
fn an_instrument_sounds_exactly_as_the_host_makes_it_over_the_pipe() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(linked.name, "Deterministic Test Instrument");
    let mut piped = pipe(true);
    for block in 0..60 {
        let events = pattern(block);
        let expected = piped
            .process(64, vec![], host_notes(&events), vec![], SLOW)
            .expect("pipe");
        let (left, right, reply) = play(&mut linked.link, 64, &[], &events);
        assert!(matches!(reply, Some(Reply::Ready { .. })), "block {block}");
        assert_eq!(left, expected.output[0], "block {block}");
        assert_eq!(right, expected.output[1], "block {block}");
    }
    assert_eq!(linked.link.counts().dropped_events(), 0);
}

#[test]
fn an_effect_processes_the_audio_it_is_given_and_reports_its_latency() {
    let mut manager = manager();
    manager.want(2, effect("")).unwrap();
    let mut linked = linked(&mut manager, 2);
    let mut piped = pipe(false);
    // Unless told the device's block, the plugin is promised the engine's
    // largest, never the protocol's: some plugins work in proportion to it
    // on every call (ADR-0075). The link has room for the engine's largest.
    assert_eq!(linked.frames as usize, lumen_devices::MOST_FRAMES);
    assert_eq!(
        linked.link.most_frames() as usize,
        lumen_devices::MOST_FRAMES
    );
    let (_, configured_latency) = (0, linked.latency);
    let left: Vec<f32> = (0..128).map(|i| (i as f32 * 0.05).sin()).collect();
    let right: Vec<f32> = (0..128).map(|i| (i as f32 * 0.03).cos()).collect();
    for block in 0..20 {
        let expected = piped
            .process(128, vec![left.clone(), right.clone()], vec![], vec![], SLOW)
            .expect("pipe");
        let (l, r, reply) = play(&mut linked.link, 128, &[&left, &right], &[]);
        let Some(Reply::Ready {
            latency, work_ns, ..
        }) = reply
        else {
            panic!("block {block}: {reply:?}");
        };
        assert!(work_ns > 0, "the host says how long the plugin worked");
        assert_eq!(latency, expected.latency);
        assert_eq!(latency, configured_latency);
        assert_eq!(l, expected.output[0], "block {block}");
        assert_eq!(r, expected.output[1], "block {block}");
    }
}

#[test]
fn state_sent_at_load_is_what_the_plugin_plays_with() {
    // A state with the level turned up, taken from a plugin that was.
    let mut source = pipe(true);
    let up = vec![ParamQueue {
        id: LEVEL,
        points: vec![(0, 1.0)],
    }];
    let events = vec![HostEvent {
        offset: 0,
        kind: EventKind::NoteOn {
            channel: 0,
            pitch: 60,
            velocity: 1.0,
        },
    }];
    source
        .process(64, vec![], events.clone(), up, SLOW)
        .expect("level");
    let state = source.state(SLOW).expect("state");

    let mut plain = instrument("");
    plain.state = Vec::new();
    let mut restored = instrument("");
    restored.state = state;
    let mut manager = manager();
    manager.want(1, plain).unwrap();
    manager.want(2, restored).unwrap();
    let mut both = linked_all(&mut manager, &[1, 2]);
    let mut b = both.pop().unwrap();
    let mut a = both.pop().unwrap();
    let events = [note_on(0, 60)];
    let (plain_left, ..) = play(&mut a.link, 64, &[], &events);
    let (restored_left, ..) = play(&mut b.link, 64, &[], &events);
    let peak = |s: &[f32]| s.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
    assert!(
        peak(&restored_left) > peak(&plain_left) * 1.5,
        "the level the state holds was not restored: {} against {}",
        peak(&restored_left),
        peak(&plain_left)
    );
}

#[test]
fn a_load_that_cannot_work_fails_with_the_step_it_failed_in() {
    let mut manager = manager();

    let mut missing = instrument("");
    missing.module = built().0.with_file_name("no-such-plugin.vst3");
    manager.want(1, missing).unwrap();
    assert_eq!(failure(&mut manager, 1).step, Step::Load);

    let mut wrong = instrument("");
    wrong.class = [7; 16];
    manager.want(2, wrong).unwrap();
    assert_eq!(failure(&mut manager, 2).step, Step::Load);

    let mut program = instrument("");
    program.host = built().0.with_file_name("no-such-host");
    manager.want(3, program).unwrap_or(());
    if manager.has(3) {
        assert_eq!(failure(&mut manager, 3).step, Step::Start);
    }

    let mut garbage = instrument("");
    garbage.state = vec![1, 2, 3];
    manager.want(4, garbage).unwrap();
    assert_eq!(failure(&mut manager, 4).step, Step::State);

    let crash = instrument("factory_crash");
    manager.want(5, crash).unwrap();
    let f = failure(&mut manager, 5);
    assert!(f.crashed(), "{f}");
    assert!(f.to_string().contains("Crashed while loading"), "{f}");

    let mut hang = instrument("factory_hang");
    hang.deadlines.load = Duration::from_millis(600);
    manager.want(6, hang).unwrap();
    let f = failure(&mut manager, 6);
    assert!(f.hung(), "{f}");
}

#[test]
fn a_host_that_crashes_in_a_block_is_noticed_and_the_guard_fails_it() {
    let mut manager = manager();
    manager.want(1, instrument("process_crash@3")).unwrap();
    let linked = linked(&mut manager, 1);
    let mut guard = Guard::new(Box::new(linked.link), linked.latency);
    let status = guard.status();
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    let mut outcomes = Vec::new();
    for block in 0..6 {
        let deadline = Instant::now() + Duration::from_millis(400);
        let sent = guard.begin(64, &[], &pattern(block));
        let outcome = if sent {
            guard.finish(deadline, &mut l, &mut r)
        } else {
            Outcome::Silent
        };
        outcomes.push(outcome);
        if status.failed() {
            break;
        }
    }
    assert_eq!(outcomes[..3], [Outcome::Ready; 3]);
    assert!(status.failed(), "the crashed host was not given up on");
    let f = failure(&mut manager, 1);
    assert_eq!(f.step, Step::Running);
    assert!(f.crashed(), "{f}");
}

#[test]
fn a_host_that_hangs_is_failed_after_a_second_of_late_audio_and_forgetting_it_kills_it() {
    let mut manager = manager();
    manager.want(1, instrument("process_hang@2")).unwrap();
    let linked = linked(&mut manager, 1);
    let pid = linked.pid;
    assert!(exists(pid));
    let mut guard = Guard::new(Box::new(linked.link), linked.latency);
    let status = guard.status();
    let (mut l, mut r) = (vec![0.0; 8192], vec![0.0; 8192]);
    let mut blocks = 0;
    // Long blocks, so a second of audio is a handful of them.
    while !status.failed() && blocks < 12 {
        let deadline = Instant::now() + Duration::from_millis(60);
        if guard.begin(8192, &[], &pattern(blocks)) {
            guard.finish(deadline, &mut l, &mut r);
        }
        blocks += 1;
    }
    assert!(status.failed(), "still trying after {blocks} blocks");
    assert!(blocks <= 9, "{blocks} blocks to give up on a hung host");
    manager.forget(1);
    assert!(
        gone_within(pid, Duration::from_secs(10)),
        "the hung host is still running"
    );
}

#[test]
fn forgetting_and_reloading_leave_no_process_behind() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let first = linked(&mut manager, 1);
    let first_pid = first.pid;
    assert!(exists(first_pid));

    // Reload: a new process, the old one gone.
    assert!(manager.reload(1).unwrap());
    let second = linked(&mut manager, 1);
    assert_ne!(second.pid, first_pid);
    assert!(gone_within(first_pid, Duration::from_secs(10)));

    // Forget: nothing runs, and nothing more is said.
    manager.forget(1);
    assert!(gone_within(second.pid, Duration::from_secs(10)));
    assert!(!manager.has(1));
    assert!(!manager.reload(1).unwrap());
    std::thread::sleep(Duration::from_millis(100));
    assert!(manager.poll().is_empty());

    // The old link answers nothing now.
    let mut link = second.link;
    assert!(
        link.ended()
            || matches!(
                play(&mut link, 64, &[], &[]).2,
                Some(Reply::Late | Reply::Gone) | None
            )
    );
}

#[test]
fn shutting_down_ends_every_host() {
    let mut manager = manager();
    for device in 1..=3 {
        manager.want(device, instrument("")).unwrap();
    }
    let pids: Vec<u32> = linked_all(&mut manager, &[1, 2, 3])
        .iter()
        .map(|l| l.pid)
        .collect();
    manager.shutdown();
    for pid in pids {
        assert!(
            gone_within(pid, Duration::from_secs(10)),
            "{pid} still runs"
        );
    }
}

#[test]
fn many_devices_load_at_once_and_all_play() {
    let mut manager = manager();
    for device in 0..9 {
        manager
            .want(
                device,
                if device % 2 == 0 {
                    instrument("")
                } else {
                    effect("")
                },
            )
            .unwrap();
    }
    let mut seen = std::collections::BTreeSet::new();
    wait_for(
        &mut manager,
        Duration::from_secs(60),
        |id, happened| match happened {
            lumen_hosting::Happened::Linked(_) => {
                seen.insert(id);
                if seen.len() == 9 {
                    Ok(())
                } else {
                    Err(lumen_hosting::Happened::Failed(lumen_hosting::Failure {
                        step: Step::Start,
                        error: lumen_plugin_host::client::HostError::Timeout,
                        detail: None,
                    }))
                }
            }
            lumen_hosting::Happened::Failed(f) => panic!("device {id}: {f}"),
        },
    );
    assert_eq!(seen.len(), 9);
}

#[test]
fn messages_the_host_cannot_carry_yet_are_dropped_and_counted() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    let events = [
        note_on(0, 60),
        Event {
            offset: 4,
            msg: Msg::controller(0, 64, 127),
        },
        Event {
            offset: 8,
            msg: Msg::pitch_bend(0, 4000),
        },
        Event {
            offset: 12,
            msg: Msg::pressure(0, 30),
        },
        Event {
            offset: 16,
            msg: Msg::program(0, 3),
        },
        note_off(32, 60),
    ];
    let (left, _, reply) = play(&mut linked.link, 64, &[], &events);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
    assert!(left[..32].iter().any(|s| *s != 0.0), "the note sounded");
    assert_eq!(linked.link.counts().dropped_events(), 4);
}

#[test]
fn a_block_too_long_for_the_region_is_refused_and_counted_not_a_crash() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut linked = linked(&mut manager, 1);
    assert_eq!(linked.link.submit(9_000, &[], &[]), Err(Refused::Busy));
    assert_eq!(linked.link.counts().unfit_blocks(), 1);
    // The link is fine afterwards.
    let (_, _, reply) = play(&mut linked.link, 64, &[], &[note_on(0, 60)]);
    assert!(matches!(reply, Some(Reply::Ready { .. })));
}

#[test]
fn a_fresh_link_for_a_new_engine_plays_at_the_new_rate_and_the_old_one_is_stale() {
    let mut manager = manager();
    manager.want(1, instrument("")).unwrap();
    let mut first = linked(&mut manager, 1);
    let events = [note_on(0, 69)];
    let (at_48k, ..) = play(&mut first.link, 128, &[], &events);

    manager.relink_all(44_100.0, 128);
    let mut second = linked(&mut manager, 1);
    assert_eq!(second.pid, first.pid, "the same host process");
    assert_eq!(second.frames, 128, "promised the device's block");
    let (at_44k, ..) = play(&mut second.link, 128, &[], &events);
    assert_ne!(at_48k, at_44k, "the new rate changes the note's phase step");

    // The same as a plugin configured at that rate over the pipe.
    let mut piped = pipe(true);
    piped.configure(44_100.0, 512, 0, 2, SLOW).unwrap();
    let expected = piped
        .process(128, vec![], host_notes(&events), vec![], SLOW)
        .unwrap();
    assert_eq!(at_44k, expected.output[0]);

    // A block longer than promised (a driver that delivered more) is still
    // played: the host hands the plugin pieces it accepts.
    let (_, _, reply) = play(&mut second.link, 256, &[], &events);
    assert!(matches!(reply, Some(Reply::Ready { .. })), "{reply:?}");

    // A block on the old link is for an old configuration: no sound.
    let (_, _, reply) = play(&mut first.link, 128, &[], &events);
    assert!(
        !matches!(reply, Some(Reply::Ready { .. })),
        "the old link was answered"
    );
}
