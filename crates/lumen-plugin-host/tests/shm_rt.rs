//! Lumen's side of the audio path must not allocate: `submit`, `poll`, `wait`
//! and `copy_output` run inside real-time sections under the allocation
//! guard, against real host processes.
//!
//! An operating-system wait (the event and hybrid modes) blocks by design, so
//! under RealtimeSanitizer, which flags every blocking call, only the polling
//! mode is run. Whether the audio callback may ever make such a wait is the
//! open question in ADR-0005; these tests do not answer it.

mod support;

use std::time::{Duration, Instant};

use lumen_plugin_host::shm::{Layout, Outcome, Poll, Waiting, Wake};
use support::{SLOW, instrument, note_off, note_on};

lumen_rt::install_guarded_allocator!();

#[test]
fn lumens_side_of_the_audio_path_does_not_allocate() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is not installed"
    );
    let under_rtsan = std::env::var_os("RTSAN_ENABLE").is_some();
    let mut modes = vec![
        ("spin/spin", Wake::SPIN),
        (
            "host events, lumen polls",
            Wake {
                host: Waiting::Event,
                lumen: Waiting::Spin,
            },
        ),
    ];
    if !under_rtsan {
        modes.push(("event/event", Wake::EVENT));
        modes.push((
            "hybrid/hybrid",
            Wake {
                host: Waiting::Hybrid(Duration::from_micros(200)),
                lumen: Waiting::Hybrid(Duration::from_micros(200)),
            },
        ));
    }
    for (name, wake) in modes {
        let mut host = instrument("");
        let mut link = host
            .attach(Layout::new(512, 0, 2), wake, SLOW)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let generation = host.generation();
        let events = [note_on(3, 60), note_off(50, 60)];
        let params = [lumen_plugin_host::protocol::ParamQueue {
            id: lumen_test_plugin::params::LEVEL,
            points: vec![(0, 0.5), (32, 1.0)],
        }];
        let mut out = vec![0.0_f32; 128];

        // Warm up outside the section so thread start-up allocations on
        // either side do not count.
        link.submit(generation, 128, &[], &events, &params)
            .expect("submit");
        assert!(matches!(link.wait(Instant::now() + SLOW), Poll::Done(_)));

        let (done, violations) = lumen_rt::count_rt_violations(|| {
            let mut done = 0_u32;
            for _ in 0..500 {
                if link.submit(generation, 128, &[], &events, &params).is_err() {
                    continue;
                }
                if let Poll::Done(answer) = link.wait(Instant::now() + Duration::from_millis(500))
                    && answer.outcome == Outcome::Done
                {
                    link.copy_output(0, &mut out);
                    link.copy_output(1, &mut out);
                    done += 1;
                }
                let _ = link.poll();
            }
            done
        });
        assert_eq!(done, 500, "{name}: every block was answered");
        assert_eq!(violations, 0, "{name}: heap operations on the audio side");
    }
}
