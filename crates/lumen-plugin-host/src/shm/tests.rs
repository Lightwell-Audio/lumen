//! The link with a stand-in host: a thread that plays the host's part so the
//! protocol (hand-off, stale answers, deadlines) is tested without a plugin
//! and without timing luck. Real hosts are in `tests/shm.rs`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::sys::remove_names;
use super::{
    Answer, BlockLink, BlockScratch, HostLink, Layout, Next, Outcome, Poll, Stopper, SubmitError,
    Waiting, Wake,
};
use crate::protocol::{Event, EventKind, ParamQueue};

fn unique() -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!(
        "t{:x}-{:x}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// What the stand-in host does with a block.
type Behaviour = Box<dyn FnMut(u64, &mut Answer, &mut BlockScratch) -> Hold + Send>;

/// Whether the stand-in answers, and after how long.
enum Hold {
    Answer(Duration),
    /// Never answers this block.
    Never,
}

struct Pair {
    link: BlockLink,
    stopper: Stopper,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Pair {
    fn drop(&mut self) {
        self.stopper.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn pair(layout: Layout, wake: Wake, mut behave: Behaviour) -> Pair {
    let base = unique();
    let (link, names) = BlockLink::create(&base, layout, wake).expect("create");
    let (mut host, stopper) = HostLink::open(&base, layout, wake).expect("open");
    remove_names(&names);
    let thread = std::thread::spawn(move || {
        let mut scratch = BlockScratch::new(&layout);
        host.mark_ready();
        while let Next::Block(seq) = host.wait_for_block() {
            let mut answer = Answer {
                seq,
                generation: 0,
                outcome: Outcome::Done,
                status: 0,
                latency: 0,
                restart: 0,
                repaired: 0,
            };
            let Ok((generation, frames)) = host.region().read_request(&mut scratch) else {
                answer.outcome = Outcome::Invalid;
                host.finish(&answer, 0, &scratch.buffers.output);
                continue;
            };
            answer.generation = generation;
            match behave(seq, &mut answer, &mut scratch) {
                Hold::Answer(after) => {
                    std::thread::sleep(after);
                    host.finish(&answer, frames, &scratch.buffers.output);
                }
                Hold::Never => {}
            }
        }
    });
    Pair {
        link,
        stopper,
        thread: Some(thread),
    }
}

/// Output is the input at half gain; the answer carries counts of what the
/// block held, so a test sees events and parameters arrive.
fn halve() -> Behaviour {
    Box::new(|_, answer, scratch| {
        let (buffers, events, params) = scratch.parts();
        for (out, input) in buffers.output.iter_mut().zip(&buffers.input) {
            for (o, i) in out.iter_mut().zip(input) {
                *o = i * 0.5;
            }
        }
        answer.status = events.len() as i32;
        answer.latency = params.len() as u32;
        answer.restart = params.iter().map(|q| q.points.len() as u32).sum();
        Hold::Answer(Duration::ZERO)
    })
}

fn in_a_second() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

fn modes() -> Vec<(&'static str, Wake)> {
    let micros = |n| Waiting::Hybrid(Duration::from_micros(n));
    vec![
        ("spin/spin", Wake::SPIN),
        ("event/event", Wake::EVENT),
        (
            "hybrid/hybrid",
            Wake {
                host: micros(200),
                lumen: micros(200),
            },
        ),
        (
            "host events, lumen polls",
            Wake {
                host: Waiting::Event,
                lumen: Waiting::Spin,
            },
        ),
        (
            "host polls, lumen events",
            Wake {
                host: Waiting::Spin,
                lumen: Waiting::Event,
            },
        ),
    ]
}

fn ramp(n: usize, from: f32) -> Vec<f32> {
    (0..n).map(|i| from + i as f32 * 0.01).collect()
}

fn note(offset: u32) -> Event {
    Event {
        offset,
        kind: EventKind::NoteOn {
            channel: 0,
            pitch: 64,
            velocity: 1.0,
        },
    }
}

#[test]
fn blocks_round_trip_in_every_wake_mode() {
    for (name, wake) in modes() {
        let layout = Layout::new(64, 2, 2);
        let mut p = pair(layout, wake, halve());
        let params = [ParamQueue {
            id: 5,
            points: vec![(0, 0.2), (10, 0.4), (20, 0.6)],
        }];
        for block in 0..200_u32 {
            let left = ramp(48, block as f32);
            let right = ramp(48, -(block as f32));
            let events = [note(block % 48), note(0)];
            let seq = p
                .link
                .submit(1, 48, &[&left, &right], &events, &params)
                .unwrap_or_else(|e| panic!("{name}: block {block}: {e}"));
            assert_eq!(seq, u64::from(block) + 1, "{name}");
            let Poll::Done(answer) = p.link.wait(in_a_second()) else {
                panic!("{name}: block {block} was not answered");
            };
            assert_eq!(answer.outcome, Outcome::Done, "{name}");
            assert_eq!((answer.seq, answer.generation), (seq, 1), "{name}");
            // The host saw two events, one queue of three points.
            assert_eq!(
                (answer.status, answer.latency, answer.restart),
                (2, 1, 3),
                "{name}"
            );
            let mut out = [0.0_f32; 48];
            assert_eq!(p.link.copy_output(0, &mut out), 0);
            assert!(
                out.iter().zip(&left).all(|(o, i)| *o == i * 0.5),
                "{name}: left channel of block {block}"
            );
            p.link.copy_output(1, &mut out);
            assert!(out.iter().zip(&right).all(|(o, i)| *o == i * 0.5), "{name}");
        }
        assert_eq!((p.link.stale(), p.link.invalid()), (0, 0), "{name}");
    }
}

#[test]
fn polling_never_waits() {
    let layout = Layout::new(16, 0, 1);
    let mut p = pair(layout, Wake::EVENT, halve());
    assert_eq!(p.link.poll(), Poll::Idle);
    p.link.submit(1, 8, &[], &[], &[]).expect("submit");
    // Eventually done; each poll is a single look.
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match p.link.poll() {
            Poll::Pending => assert!(Instant::now() < end, "no answer"),
            Poll::Done(a) => {
                assert_eq!(a.outcome, Outcome::Done);
                break;
            }
            Poll::Idle => panic!("the block vanished"),
        }
    }
    assert_eq!(p.link.poll(), Poll::Idle);
}

#[test]
fn a_late_answer_is_dropped_as_stale_and_never_taken_for_the_next_block() {
    for (name, wake) in modes() {
        let layout = Layout::new(16, 0, 1);
        // The first block takes 300 ms (long enough that a thread kept off the
        // CPU by a busy machine still finds it unanswered); the others are
        // immediate and write 7.
        let behave: Behaviour = Box::new(|seq, _answer, scratch| {
            scratch.buffers.output[0].fill(if seq == 1 { 1.0 } else { 7.0 });
            Hold::Answer(if seq == 1 {
                Duration::from_millis(300)
            } else {
                Duration::ZERO
            })
        });
        let mut p = pair(layout, wake, behave);
        p.link.submit(1, 8, &[], &[], &[]).expect("submit");
        // Missed the deadline: given up on.
        assert_eq!(
            p.link.wait(Instant::now() + Duration::from_millis(2)),
            Poll::Pending,
            "{name}"
        );
        // Still on it: nothing more can be handed over.
        assert_eq!(
            p.link.submit(1, 8, &[], &[], &[]),
            Err(SubmitError::Busy),
            "{name}"
        );
        assert!(p.link.in_flight());
        // It finishes while nobody waits: its answer is stale, and the next
        // block goes through. Try until it does, not after a fixed time.
        let started = Instant::now();
        let seq = loop {
            match p.link.submit(1, 8, &[], &[], &[]) {
                Ok(seq) => break seq,
                Err(SubmitError::Busy) => {
                    assert!(
                        started.elapsed() < Duration::from_secs(30),
                        "{name}: the late answer never came"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(other) => panic!("{name}: submit after the late answer: {other:?}"),
            }
        };
        assert_eq!(seq, 2, "{name}");
        assert_eq!(p.link.stale(), 1, "{name}");
        let Poll::Done(answer) = p.link.wait(in_a_second()) else {
            panic!("{name}: block 2 was not answered");
        };
        assert_eq!(answer.seq, 2, "{name}");
        let mut out = [0.0_f32; 8];
        p.link.copy_output(0, &mut out);
        assert_eq!(
            out, [7.0; 8],
            "{name}: the late block's audio must not appear"
        );
    }
}

#[test]
fn a_host_that_never_answers_keeps_its_block_in_flight() {
    let layout = Layout::new(16, 0, 1);
    let mut p = pair(layout, Wake::EVENT, Box::new(|_, _, _| Hold::Never));
    p.link.submit(1, 8, &[], &[], &[]).expect("submit");
    for _ in 0..3 {
        let started = Instant::now();
        assert_eq!(
            p.link.wait(started + Duration::from_millis(5)),
            Poll::Pending
        );
        // The deadline holds: the wait ends on time, not when the host does
        // (it never does). The bound is far past the 5 ms asked for, so a
        // thread a busy machine keeps off the CPU for a while does not fail it.
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(p.link.submit(1, 8, &[], &[], &[]), Err(SubmitError::Busy));
    }
    assert!(p.link.in_flight());
    assert_eq!(p.link.stale(), 0);
}

#[test]
fn an_answer_for_another_generation_or_block_is_invalid() {
    // The stand-in writes a generation that is not the block's.
    let behave: Behaviour = Box::new(|_, answer, _| {
        answer.generation += 1;
        Hold::Answer(Duration::ZERO)
    });
    let mut p = pair(Layout::new(16, 0, 1), Wake::EVENT, behave);
    p.link.submit(4, 8, &[], &[], &[]).expect("submit");
    let Poll::Done(answer) = p.link.wait(in_a_second()) else {
        panic!("no answer");
    };
    assert_eq!(answer.outcome, Outcome::Invalid);
    assert_eq!(p.link.invalid(), 1);
    // The link is usable afterwards.
    p.link.submit(4, 8, &[], &[], &[]).expect("submit");
}

#[test]
fn a_stale_configuration_answer_is_counted() {
    let behave: Behaviour = Box::new(|_, answer, _| {
        answer.outcome = Outcome::Stale;
        Hold::Answer(Duration::ZERO)
    });
    let mut p = pair(Layout::new(16, 0, 1), Wake::SPIN, behave);
    p.link.submit(1, 8, &[], &[], &[]).expect("submit");
    let Poll::Done(answer) = p.link.wait(in_a_second()) else {
        panic!("no answer");
    };
    assert_eq!(answer.outcome, Outcome::Stale);
    assert_eq!(p.link.stale(), 1);
}

#[test]
fn a_block_that_does_not_fit_is_refused_and_leaves_the_link_idle() {
    let mut p = pair(Layout::new(16, 1, 1), Wake::EVENT, halve());
    assert_eq!(
        p.link.submit(1, 17, &[&[0.0; 17]], &[], &[]),
        Err(SubmitError::Invalid("frame count"))
    );
    assert!(!p.link.in_flight());
    assert_eq!(p.link.poll(), Poll::Idle);
    // The next honest block takes sequence number 1: nothing was consumed.
    assert_eq!(p.link.submit(1, 8, &[&[0.5; 8]], &[], &[]), Ok(1));
}

#[test]
fn stopping_ends_the_host_loop_in_every_mode() {
    for (name, wake) in modes() {
        let started = Instant::now();
        let p = pair(Layout::new(16, 0, 1), wake, halve());
        drop(p);
        // Dropping stops and joins the thread; a loop that missed the quit
        // flag would hang the test, so the bound is generous but real.
        assert!(started.elapsed() < Duration::from_secs(2), "{name}");
    }
}
