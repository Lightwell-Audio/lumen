//! The link is what the audio thread calls: `submit`, `collect` and `ended`
//! must not allocate, against a real host process. Under RealtimeSanitizer
//! only the polling wait is run, because an operating-system wait is flagged
//! as blocking by design (ADR-0069 puts that one wait on trial).

mod support;

use std::time::{Duration, Instant};

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Hosted, ParamBlock, Reply};
use lumen_hosting::Manager;
use lumen_plugin_host::shm::{Waiting, Wake};
use support::*;

lumen_rt::install_guarded_allocator!();

#[test]
fn the_link_does_not_allocate_on_the_audio_thread() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is not installed"
    );
    let under_rtsan = std::env::var_os("RTSAN_ENABLE").is_some();
    let mut wakes = vec![(
        "host events, lumen polls",
        Wake {
            host: Waiting::Event,
            lumen: Waiting::Spin,
        },
    )];
    if !under_rtsan {
        wakes.push(("event/event", Wake::EVENT));
    }
    for (name, wake) in wakes {
        for (is_instrument, start) in [(true, instrument("")), (false, effect(""))] {
            let mut start = start;
            start.wake = wake;
            let mut manager = Manager::new(|| {});
            manager.want(1, start).unwrap();
            let mut linked = linked(&mut manager, 1);
            let events = [
                note_on(3, 60),
                Event {
                    offset: 5,
                    msg: Msg::controller(0, 64, 127),
                },
                note_off(50, 60),
            ];
            let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.05).sin()).collect();
            let stereo: [&[f32]; 2] = [&input, &input];
            let audio: &[&[f32]] = if is_instrument { &[] } else { &stereo };
            let (mut left, mut right) = (vec![0.0_f32; 128], vec![0.0_f32; 128]);
            // Parameters change in every block: a stepped one and a ramped
            // one (M7 slice A3), written into the region like the events.
            let mut params = ParamBlock::new();
            params.start(128);
            params.open(0);
            for (at, value) in [(0, 0.4), (63, 0.4), (64, 0.8)] {
                params.point(at, value);
            }
            params.open(1);
            params.point(0, 0.0);
            params.point(127, 1.0);

            // Warm up outside the section, so start-up work does not count.
            linked.link.submit(128, audio, &events, &params).unwrap();
            linked.link.collect(
                Instant::now() + Duration::from_secs(5),
                &mut left,
                &mut right,
            );

            let (ready, violations) = lumen_rt::count_rt_violations(|| {
                let mut ready = 0_u32;
                for _ in 0..500 {
                    if linked.link.ended()
                        || linked.link.submit(128, audio, &events, &params).is_err()
                    {
                        continue;
                    }
                    let deadline = Instant::now() + Duration::from_millis(500);
                    if let Reply::Ready { .. } =
                        linked.link.collect(deadline, &mut left, &mut right)
                    {
                        ready += 1;
                    }
                }
                ready
            });
            assert_eq!(violations, 0, "{name}, instrument: {is_instrument}");
            assert!(ready > 400, "{name}: only {ready} blocks answered");
        }
    }
}
