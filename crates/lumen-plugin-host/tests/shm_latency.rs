mod support;

use std::time::{Duration, Instant};

use lumen_plugin_host::protocol::ParamQueue;
use lumen_plugin_host::shm::{Layout, Outcome, Poll, Waiting, Wake};
use lumen_test_plugin::params::{LATENCY, MAX_LATENCY};
use support::{SLOW, effect};

lumen_rt::install_guarded_allocator!();

const FRAMES: usize = 64;
const BLOCKS: usize = 60;
const RAISE_AT: usize = 20;
const RAISED_TO: u32 = 24;
const LOWER_AT: usize = 40;

fn delay(samples: u32) -> [ParamQueue; 1] {
    [ParamQueue {
        id: LATENCY,
        points: vec![(0, f64::from(samples) / f64::from(MAX_LATENCY))],
    }]
}

#[derive(Clone, Copy, Default)]
struct Seen {
    latency: u32,
    restart: u32,
    repaired: u32,
    first: Option<usize>,
    nonzero: usize,
    answered: bool,
}

#[test]
fn a_latency_change_reaches_the_loop_in_the_same_block_and_costs_no_allocation() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is not installed"
    );
    let mut host = effect("");
    let wake = Wake {
        host: Waiting::Event,
        lumen: Waiting::Spin,
    };
    let mut link = host
        .attach(Layout::new(512, 2, 2), wake, SLOW)
        .expect("attach");
    let generation = host.generation();

    let mut impulse = [0.0_f32; FRAMES];
    impulse[0] = 1.0;
    let input: [&[f32]; 2] = [&impulse, &impulse];
    let raise = delay(RAISED_TO);
    let lower = delay(0);
    let mut out = [0.0_f32; FRAMES];
    let mut seen = [Seen::default(); BLOCKS];

    link.submit(generation, FRAMES as u32, &input, &[], &[])
        .expect("submit");
    assert!(matches!(link.wait(Instant::now() + SLOW), Poll::Done(_)));

    let (changes_noticed, violations) = lumen_rt::count_rt_violations(|| {
        let mut known = 0_u32;
        let mut noticed = 0_u32;
        for (block, slot) in seen.iter_mut().enumerate() {
            let params: &[ParamQueue] = match block {
                RAISE_AT => &raise,
                LOWER_AT => &lower,
                _ => &[],
            };
            if link
                .submit(generation, FRAMES as u32, &input, &[], params)
                .is_err()
            {
                continue;
            }
            if let Poll::Done(answer) = link.wait(Instant::now() + Duration::from_millis(500))
                && answer.outcome == Outcome::Done
            {
                link.copy_output(0, &mut out);
                if answer.latency != known {
                    known = answer.latency;
                    noticed += 1;
                }
                *slot = Seen {
                    latency: answer.latency,
                    restart: answer.restart,
                    repaired: answer.repaired,
                    first: out.iter().position(|&s| s != 0.0),
                    nonzero: out.iter().filter(|&&s| s != 0.0).count(),
                    answered: true,
                };
            }
        }
        noticed
    });

    assert!(seen.iter().all(|s| s.answered), "every block was answered");
    assert_eq!(violations, 0, "heap operations noticing a latency change");
    assert_eq!(changes_noticed, 2, "one notice per change");

    for (block, s) in seen.iter().enumerate() {
        let expected = if (RAISE_AT..LOWER_AT).contains(&block) {
            RAISED_TO
        } else {
            0
        };
        assert_eq!(s.latency, expected, "block {block}: reported latency");
        assert_eq!(s.repaired, 0, "block {block}: output was repaired");
        assert_eq!(s.restart, 0, "block {block}: restart flag");
    }

    for (block, s) in seen.iter().enumerate() {
        if block == RAISE_AT || block == LOWER_AT {
            continue;
        }
        let at = if (RAISE_AT..LOWER_AT).contains(&block) {
            RAISED_TO as usize
        } else {
            0
        };
        assert_eq!(s.first, Some(at), "block {block}: impulse position");
        assert_eq!(s.nonzero, 1, "block {block}: only the impulse is heard");
    }
}

#[test]
fn a_flushed_block_forgets_what_the_effect_was_holding() {
    let mut host = effect("");
    let wake = Wake {
        host: Waiting::Event,
        lumen: Waiting::Spin,
    };
    let mut link = host
        .attach(Layout::new(512, 2, 2), wake, SLOW)
        .expect("attach");
    let generation = host.generation();
    let delayed = delay(96);
    let mut impulse = [0.0_f32; FRAMES];
    impulse[0] = 1.0;
    let silence = [0.0_f32; FRAMES];
    let mut out = [0.0_f32; FRAMES];

    let mut run = |link: &mut lumen_plugin_host::shm::BlockLink,
                   input: &[f32; FRAMES],
                   params: &[ParamQueue],
                   flush: bool| {
        if flush {
            link.flush_next();
        }
        link.submit(generation, FRAMES as u32, &[input, input], &[], params)
            .expect("submit");
        let Poll::Done(answer) = link.wait(Instant::now() + SLOW) else {
            panic!("no answer");
        };
        assert_eq!(answer.outcome, Outcome::Done);
        link.copy_output(0, &mut out);
        out.iter().filter(|&&s| s != 0.0).count()
    };

    for flush in [false, true] {
        assert_eq!(run(&mut link, &impulse, &delayed, false), 0);
        assert_eq!(run(&mut link, &silence, &[], flush), usize::from(!flush));
        run(&mut link, &silence, &[], false);
        assert_eq!(run(&mut link, &silence, &[], false), 0);
    }
}
