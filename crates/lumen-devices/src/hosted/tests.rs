//! The hosted devices' tests.

use std::sync::Mutex;

use super::instrument::EVENTS;
use super::*;

/// What a fake host was asked, and the level it answers with (the test
/// may change it while the host is in use).
#[derive(Default)]
struct Seen {
    submits: u32,
    /// Submits still to refuse as busy, as a host on an earlier block.
    busy: u32,
    events: Vec<Vec<Event>>,
    /// Each block's parameter queues: id and points.
    params: Vec<Vec<(u32, Vec<(u32, f64)>)>>,
    value: f32,
    /// The deadline of the last collect.
    deadline: Option<Instant>,
}

/// A host that answers from a script and records what it is asked.
struct Fake {
    script: Vec<Reply>,
    next: usize,
    refuse: Option<Refused>,
    ended: bool,
    seen: Arc<Mutex<Seen>>,
}

impl Fake {
    fn new(script: Vec<Reply>) -> (Box<Self>, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen {
            value: 0.5,
            ..Seen::default()
        }));
        let fake = Box::new(Self {
            script,
            next: 0,
            refuse: None,
            ended: false,
            seen: seen.clone(),
        });
        (fake, seen)
    }
}

impl Hosted for Fake {
    fn submit(
        &mut self,
        _: usize,
        _: &[&[f32]],
        events: &[Event],
        params: &ParamBlock,
    ) -> Result<(), Refused> {
        if let Some(why) = self.refuse {
            return Err(why);
        }
        let mut seen = self.seen.lock().unwrap();
        if seen.busy > 0 {
            seen.busy -= 1;
            return Err(Refused::Busy);
        }
        seen.submits += 1;
        seen.events.push(events.to_vec());
        seen.params.push(
            (0..params.len())
                .map(|i| {
                    let (id, points) = params.queue(i);
                    (id, points.to_vec())
                })
                .collect(),
        );
        Ok(())
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        self.seen.lock().unwrap().deadline = Some(deadline);
        let reply = self.script[self.next.min(self.script.len() - 1)];
        self.next += 1;
        if matches!(reply, Reply::Ready { .. }) {
            let value = self.seen.lock().unwrap().value;
            left.fill(value);
            right.fill(-value);
        }
        reply
    }

    fn ended(&self) -> bool {
        self.ended
    }

    /// Auxiliary pair 1 plays a quarter of full scale.
    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        if pair != 1 {
            return false;
        }
        left.fill(0.25);
        right.fill(0.25);
        true
    }
}

const READY: Reply = Reply::Ready {
    latency: 0,
    tail: 0,
    restart: false,
    work_ns: 40_000,
};

fn submits(seen: &Arc<Mutex<Seen>>) -> u32 {
    seen.lock().unwrap().submits
}

fn run(guard: &mut Guard) -> (Outcome, [f32; 2]) {
    let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
    guard.begin(4, &[], &[], &ParamBlock::none());
    let outcome = guard.finish(Instant::now() + Duration::from_millis(1), &mut l, &mut r);
    (outcome, [l[0], r[0]])
}

#[test]
fn a_ready_block_is_the_hosts_output() {
    let (host, _) = Fake::new(vec![READY]);
    let mut guard = Guard::new(host, 0);
    assert_eq!(run(&mut guard), (Outcome::Ready, [0.5, -0.5]));
    assert!(!guard.failed());
}

#[test]
fn the_plugins_work_and_the_wait_for_it_are_counted_late_blocks_too() {
    let (host, _) = Fake::new(vec![READY, Reply::Late]);
    let mut guard = Guard::new(host, 0);
    let status = guard.status();
    run(&mut guard);
    run(&mut guard);
    let cost = status.take_cost();
    assert_eq!(cost.blocks, 2, "both blocks were waited for");
    assert_eq!(
        cost.work_mean,
        Duration::from_micros(40),
        "work of the answer"
    );
    assert_eq!(status.take_cost().blocks, 0, "taken");
}

#[test]
fn a_changed_latency_is_news_once_and_a_restart_is_asked_once() {
    let change = Reply::Ready {
        latency: 128,
        tail: 9,
        restart: true,
        work_ns: 0,
    };
    let (host, _) = Fake::new(vec![READY, change, change]);
    let mut guard = Guard::new(host, 0);
    let status = guard.status();
    run(&mut guard);
    assert_eq!(status.take_news(), None, "unchanged is not news");
    run(&mut guard);
    assert_eq!(status.take_news(), Some(128));
    assert_eq!(status.take_news(), None, "told once");
    assert!(status.take_restart());
    assert!(!status.take_restart());
    assert_eq!((status.latency(), status.tail()), (128, 9));
    run(&mut guard);
    assert_eq!(
        status.take_news(),
        None,
        "the same latency again is not news"
    );
}

#[test]
fn a_second_of_late_audio_in_a_row_fails_the_host_and_an_answer_in_between_resets_it() {
    // Blocks of 4 frames: the span is counted in audio, not blocks.
    let per_second = (FAILED_AFTER_FRAMES / 4) as usize;
    let mut script = vec![Reply::Late; per_second - 1];
    script.push(READY);
    script.extend(vec![Reply::Late; per_second]);
    let (host, seen) = Fake::new(script);
    let mut guard = Guard::new(host, 0);
    for _ in 0..per_second - 1 {
        assert_eq!(run(&mut guard).0, Outcome::Silent);
    }
    assert!(!guard.failed(), "a few milliseconds late is a glitch");
    assert_eq!(run(&mut guard).0, Outcome::Ready);
    for _ in 0..per_second - 1 {
        assert_eq!(run(&mut guard).0, Outcome::Silent);
    }
    assert!(
        !guard.failed(),
        "an answer in between started the count again"
    );
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert!(guard.failed(), "a second late in a row");
    assert_eq!(guard.late_blocks(), (2 * per_second - 1) as u32);
    let asked = submits(&seen);
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert_eq!(
        submits(&seen),
        asked,
        "a failed host is never submitted to again"
    );
}

#[test]
fn a_host_that_is_busy_gives_silence_and_counts() {
    let (mut host, _) = Fake::new(vec![READY]);
    host.refuse = Some(Refused::Busy);
    let mut guard = Guard::new(host, 0);
    for _ in 0..FAILED_AFTER_FRAMES / 4 - 1 {
        assert_eq!(run(&mut guard).0, Outcome::Silent);
    }
    assert!(!guard.failed());
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert!(guard.failed());
}

#[test]
fn an_ended_or_gone_host_fails_at_once() {
    let (mut host, _) = Fake::new(vec![READY]);
    host.ended = true;
    let mut guard = Guard::new(host, 0);
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert!(guard.failed());

    let (mut host, _) = Fake::new(vec![READY]);
    host.refuse = Some(Refused::Gone);
    let mut guard = Guard::new(host, 0);
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert!(guard.failed());

    let (host, _) = Fake::new(vec![Reply::Gone]);
    let mut guard = Guard::new(host, 0);
    assert_eq!(run(&mut guard).0, Outcome::Silent);
    assert!(guard.failed());
}

#[test]
fn finishing_without_a_block_in_flight_is_silence() {
    let (host, seen) = Fake::new(vec![READY]);
    let mut guard = Guard::new(host, 0);
    let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
    let outcome = guard.finish(Instant::now(), &mut l, &mut r);
    assert_eq!(outcome, Outcome::Silent);
    assert_eq!(submits(&seen), 0);
}

fn on(offset: u32, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_on(0, key, 100),
    }
}

fn instrument(script: Vec<Reply>) -> (HostedInstrument, Arc<Mutex<Seen>>) {
    let (host, seen) = Fake::new(script);
    (HostedInstrument::new(Guard::new(host, 0)), seen)
}

#[test]
fn begin_hands_over_the_events_at_their_frames_and_finish_takes_the_output() {
    let (mut synth, seen) = instrument(vec![READY]);
    let events = [on(0, 60), on(37, 64)];
    assert!(synth.begin(48_000, 64, &events));
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.finish(Instant::now() + Duration::from_millis(5), &mut l, &mut r);
    assert_eq!(seen.lock().unwrap().events, vec![events.to_vec()]);
    assert_eq!((l[63], r[63]), (0.5, -0.5));
}

#[test]
fn render_without_an_early_begin_does_the_same_in_one_call() {
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.render(48_000, &[on(5, 60)], &mut l, &mut r);
    assert_eq!(seen.lock().unwrap().events, vec![vec![on(5, 60)]]);
    assert_eq!((l[0], r[0]), (0.5, -0.5));
}

#[test]
fn a_message_with_no_block_rides_on_the_next_one() {
    let (mut synth, seen) = instrument(vec![READY]);
    let off = Event {
        offset: 33,
        msg: Msg::note_off(0, 60),
    };
    synth.render(0, &[off], &mut [], &mut []);
    assert_eq!(submits(&seen), 0, "nothing is submitted for no frames");
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.render(48_000, &[on(7, 62)], &mut l, &mut r);
    let sent = seen.lock().unwrap().events.clone();
    assert_eq!(
        sent,
        vec![vec![
            Event {
                offset: 0,
                msg: off.msg
            },
            on(7, 62)
        ]],
        "the carried message goes first, at the block's start"
    );
}

#[test]
fn the_other_outputs_are_those_of_this_block_never_the_last_ones() {
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.render(48_000, &[on(0, 60)], &mut l, &mut r);
    let (mut al, mut ar) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(
        synth.output(1, &mut al, &mut ar),
        "a finished block has them"
    );
    // The host is still on an earlier block: this one is refused, and the
    // main output is silent; the other outputs are not the last block's.
    seen.lock().unwrap().busy = 1;
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.render(48_000, &[on(0, 62)], &mut l, &mut r);
    assert_eq!(l[0], 0.0, "a refused block is silent");
    assert!(
        !synth.output(1, &mut al, &mut ar),
        "and has no other outputs to repeat"
    );
}

#[test]
fn a_block_waited_for_in_render_waits_until_the_callbacks_deadline_when_it_has_one() {
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    let shared = Instant::now() + Duration::from_millis(3);
    synth.render_by(48_000, &[on(0, 60)], &mut l, &mut r, Some(shared));
    assert_eq!(seen.lock().unwrap().deadline, Some(shared));
    // With none it waits for a share of the block of its own (75% of
    // 64 frames at 48 kHz is 1 ms).
    let before = Instant::now();
    synth.render(48_000, &[on(0, 62)], &mut l, &mut r);
    let own = seen.lock().unwrap().deadline.unwrap();
    assert!(own >= before + Duration::from_micros(900));
    assert!(own <= Instant::now() + Duration::from_millis(1));
}

/// Blocks the instrument is given, at 1 kHz in blocks of 100 frames, until
/// it is left alone (at most `most`).
fn rung(synth: &mut HostedInstrument, most: usize) -> usize {
    let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
    let mut blocks = 0;
    while synth.begin(1000, 100, &[]) {
        synth.finish(Instant::now(), &mut l, &mut r);
        blocks += 1;
        assert!(blocks <= most, "it must stop being rung");
    }
    blocks
}

/// At 1 kHz, blocks of 100 frames: the quiet that ends a plugin's turn.
const QUIET_BLOCKS: usize = (QUIET_SECONDS * 10) as usize;

fn quiet(seen: &Arc<Mutex<Seen>>) {
    seen.lock().unwrap().value = 0.0;
}

#[test]
fn a_host_is_left_alone_while_nothing_sounds_and_rung_out_after_a_note_ends() {
    // A plugin that reports no tail and plays nothing after the note: it is
    // given blocks until two seconds of quiet have gone by.
    let (mut synth, seen) = instrument(vec![READY]);
    quiet(&seen);
    assert!(!synth.begin(1000, 100, &[]), "idle: no host work");
    assert!(!synth.active());
    let off = Event {
        offset: 50,
        msg: Msg::note_off(0, 60),
    };
    assert!(synth.begin(1000, 100, &[on(0, 60), off]));
    let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
    synth.finish(Instant::now(), &mut l, &mut r);
    assert!(synth.active());
    assert_eq!(
        rung(&mut synth, 100),
        QUIET_BLOCKS - 1,
        "two seconds of quiet, counted from the block with the event"
    );
    assert!(!synth.active());
    assert_eq!(submits(&seen), QUIET_BLOCKS as u32);
}

#[test]
fn a_host_whose_output_still_sounds_is_given_blocks_until_it_goes_quiet() {
    // The owner's report: a synth's release, and a delay in it, were cut a
    // second after the last note. Here the plugin keeps sounding for ten
    // seconds after the note ends, far past any guessed tail.
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
    let off = Event {
        offset: 50,
        msg: Msg::note_off(0, 60),
    };
    assert!(synth.begin(1000, 100, &[on(0, 60), off]));
    synth.finish(Instant::now(), &mut l, &mut r);
    for block in 0..100 {
        assert!(synth.begin(1000, 100, &[]), "block {block}: still sounding");
        synth.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(l[99], 0.5, "block {block}: the host's output is heard");
    }
    quiet(&seen);
    assert_eq!(
        rung(&mut synth, 100),
        QUIET_BLOCKS,
        "two seconds after the last sound, then left alone"
    );
    assert!(!synth.active());
}

#[test]
fn a_reported_tail_longer_than_the_quiet_is_waited_through_up_to_ten_seconds() {
    for (tail, blocks) in [(5_000, 50), (u32::MAX, 100)] {
        let (mut synth, seen) = instrument(vec![Reply::Ready {
            latency: 0,
            tail,
            restart: false,
            work_ns: 0,
        }]);
        quiet(&seen);
        let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
        assert!(synth.begin(1000, 100, &[on(0, 60)]));
        synth.finish(Instant::now(), &mut l, &mut r);
        let off = Event {
            offset: 0,
            msg: Msg::note_off(0, 60),
        };
        assert!(synth.begin(1000, 100, &[off]));
        synth.finish(Instant::now(), &mut l, &mut r);
        assert_eq!(rung(&mut synth, 200), blocks - 1, "a tail of {tail} frames");
    }
}

#[test]
fn a_guard_follows_quiet_while_its_node_does_not_run() {
    let (host, seen) = Fake::new(vec![READY]);
    let mut guard = Guard::new(host, 0);
    assert!(!guard.sounding(1000), "a new host has played nothing");
    run(&mut guard);
    assert!(guard.sounding(1000));
    // The block it was asked for is counted by its output, not again.
    guard.age(4);
    guard.age(1_999);
    assert!(guard.sounding(1000));
    guard.age(1);
    assert!(!guard.sounding(1000), "two seconds unasked");
    guard.wake();
    assert!(guard.sounding(1000));
    guard.hush();
    assert!(!guard.sounding(1000));
    quiet(&seen);
    run(&mut guard);
    assert!(!guard.sounding(1000), "a quiet block does not wake it");
}

#[test]
fn a_held_note_keeps_the_host_asked_for_blocks_and_it_rings_out_after_the_release() {
    // Hold the note for thirty blocks, longer than the quiet, with no event
    // in any of them after the first and nothing heard from the plugin.
    let (mut synth, seen) = instrument(vec![READY]);
    quiet(&seen);
    let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
    assert!(synth.begin(1000, 100, &[on(0, 60)]));
    synth.finish(Instant::now(), &mut l, &mut r);
    for block in 0..30 {
        assert!(
            synth.begin(1000, 100, &[]),
            "block {block}: the note is held"
        );
        synth.finish(Instant::now(), &mut l, &mut r);
        assert!(synth.active(), "block {block}");
    }
    let off = Event {
        offset: 0,
        msg: Msg::note_off(0, 60),
    };
    assert!(synth.begin(1000, 100, &[off]));
    synth.finish(Instant::now(), &mut l, &mut r);
    assert_eq!(
        rung(&mut synth, 100),
        QUIET_BLOCKS - 1,
        "the quiet is counted from the release"
    );
}

#[test]
fn a_held_pedal_and_all_notes_off_are_followed_too() {
    let (mut synth, seen) = instrument(vec![READY]);
    quiet(&seen);
    let (mut l, mut r) = ([0.0f32; 100], [0.0f32; 100]);
    let msg = |msg| Event { offset: 0, msg };
    let pedal = msg(Msg::controller(0, cc::SUSTAIN, 127));
    assert!(synth.begin(1000, 100, &[on(0, 60), pedal]));
    synth.finish(Instant::now(), &mut l, &mut r);
    let off = msg(Msg::note_off(0, 60));
    assert!(synth.begin(1000, 100, &[off]));
    synth.finish(Instant::now(), &mut l, &mut r);
    for block in 0..30 {
        assert!(
            synth.begin(1000, 100, &[]),
            "block {block}: the pedal is down"
        );
        synth.finish(Instant::now(), &mut l, &mut r);
    }
    // All notes off lets go of everything the plugin was holding.
    assert!(synth.begin(1000, 100, &[on(0, 62)]));
    synth.finish(Instant::now(), &mut l, &mut r);
    let all_off = msg(Msg::controller(0, cc::ALL_NOTES_OFF, 0));
    assert!(synth.begin(1000, 100, &[all_off]));
    synth.finish(Instant::now(), &mut l, &mut r);
    assert_eq!(
        rung(&mut synth, 100),
        QUIET_BLOCKS - 1,
        "nothing is held after all notes off: the release rings out"
    );
}

#[test]
fn a_failed_host_is_silent_and_not_active_and_a_placeholder_does_nothing() {
    let (mut synth, _) = instrument(vec![Reply::Gone]);
    assert!(synth.begin(48_000, 64, &[on(0, 60)]));
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.finish(Instant::now(), &mut l, &mut r);
    assert_eq!(l, [0.0; 64], "silence");
    assert!(!synth.active(), "a failed host rings no more");
    assert!(
        !synth.begin(48_000, 64, &[on(0, 60)]),
        "and is not asked again"
    );

    let mut placeholder = HostedInstrument::placeholder();
    assert!(!placeholder.hosted());
    assert!(!placeholder.begin(48_000, 64, &[on(0, 60)]));
    placeholder.render(48_000, &[on(0, 60)], &mut l, &mut r);
    assert!(!placeholder.active());
}

#[test]
fn silence_sends_all_sound_off_with_the_next_block() {
    let (mut synth, seen) = instrument(vec![READY]);
    synth.silence();
    assert!(synth.active(), "the message must still reach the host");
    assert!(synth.begin(48_000, 64, &[]));
    assert!(!synth.active(), "and once it has, nothing is left to do");
    let sent = seen.lock().unwrap().events.clone();
    assert_eq!(
        sent,
        vec![vec![Event {
            offset: 0,
            msg: Msg::controller(0, cc::ALL_SOUND_OFF, 0)
        }]]
    );
}

#[test]
fn what_a_busy_host_was_not_given_goes_with_the_next_block() {
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(synth.begin(48_000, 64, &[on(0, 60)]));
    synth.finish(Instant::now(), &mut l, &mut r);
    // The host is still on a block when the note-off is due.
    seen.lock().unwrap().busy = 1;
    let off = Event {
        offset: 37,
        msg: Msg::note_off(0, 60),
    };
    assert!(!synth.begin(48_000, 64, &[off]), "refused");
    synth.finish(Instant::now(), &mut l, &mut r);
    assert!(synth.active(), "the note-off still has to go");
    assert!(synth.begin(48_000, 64, &[on(5, 64)]));
    let sent = seen.lock().unwrap().events.clone();
    assert_eq!(
        sent.last().unwrap(),
        &vec![
            Event {
                offset: 0,
                msg: Msg::note_off(0, 60)
            },
            on(5, 64)
        ],
        "late, at the start of the block, before what is new"
    );
}

#[test]
fn a_block_refused_early_and_tried_again_by_render_is_sent_once() {
    // The engine hands an instrument its block early (`begin`) and, when
    // that is refused, renders it with the same events (review H3): what
    // was refused is in `pending`, and must not be added a second time.
    let (mut synth, seen) = instrument(vec![READY, READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(synth.begin(48_000, 64, &[on(0, 60)]));
    synth.finish(Instant::now(), &mut l, &mut r);
    seen.lock().unwrap().busy = 1;
    let off = Event {
        offset: 37,
        msg: Msg::note_off(0, 60),
    };
    assert!(!synth.begin(48_000, 64, &[off]), "refused early");
    // The host is free again when render tries.
    synth.render(48_000, &[off], &mut l, &mut r);
    let sent = seen.lock().unwrap().events.clone();
    assert_eq!(
        sent.last().unwrap(),
        &vec![Event {
            offset: 0,
            msg: Msg::note_off(0, 60)
        }],
        "one note-off, once"
    );
}

#[test]
fn a_block_refused_early_and_again_by_render_is_one_missed_block() {
    let (mut synth, seen) = instrument(vec![READY]);
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    assert!(synth.begin(48_000, 64, &[on(0, 60)]));
    synth.finish(Instant::now(), &mut l, &mut r);
    seen.lock().unwrap().busy = 2;
    assert!(!synth.begin(48_000, 64, &[on(0, 64)]));
    synth.render(48_000, &[on(0, 64)], &mut l, &mut r);
    let late = synth.guard.as_ref().unwrap().late_blocks();
    assert_eq!(late, 1, "the same block, refused twice, counted once");
    // And what was said still waits, once.
    assert_eq!(synth.pending.len(), 1);
}

#[test]
fn a_storm_past_the_limit_is_dropped_and_counted() {
    let (mut synth, seen) = instrument(vec![READY]);
    let storm: Vec<Event> = (0..EVENTS + 5).map(|i| on(i as u32 % 64, 60)).collect();
    assert!(synth.begin(48_000, 64, &storm));
    assert_eq!(synth.dropped_events(), 5);
    assert_eq!(seen.lock().unwrap().events[0].len(), EVENTS);
}

#[test]
fn a_hosted_instrument_gives_its_host_the_blocks_parameters_with_the_block() {
    let (host, seen) = Fake::new(vec![READY]);
    let mut synth = HostedInstrument::new(Guard::new(host, 0));
    let mut block = ParamBlock::new();
    block.start(64);
    block.open(5);
    block.point(0, 0.25);
    block.point(63, 0.75);
    synth.automate(&block);
    let on = Event {
        offset: 0,
        msg: Msg::note_on(0, 60, 100),
    };
    assert!(synth.begin(48_000, 64, &[on]));
    let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
    synth.finish(Instant::now() + Duration::from_millis(5), &mut l, &mut r);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.params.len(), 1);
    assert_eq!(seen.params[0], vec![(5, vec![(0, 0.25), (63, 0.75)])]);
}

#[test]
fn a_block_with_no_automation_carries_none() {
    let (host, seen) = Fake::new(vec![READY]);
    let mut synth = HostedInstrument::new(Guard::new(host, 0));
    let on = Event {
        offset: 0,
        msg: Msg::note_on(0, 60, 100),
    };
    assert!(synth.begin(48_000, 64, &[on]));
    assert_eq!(seen.lock().unwrap().params, vec![vec![]]);
}
