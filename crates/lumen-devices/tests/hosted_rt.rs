//! A plugin instrument's side of a block never touches the heap: the host is
//! asked, its answer is copied, and nothing of the wrapper (the messages
//! waiting for a block, the notes and pedal it holds, the outputs) reaches the
//! heap. The host here stands in for the real one, which answers through a
//! shared region; it allocates, locks and blocks nothing, as a host's audio
//! side does once its region is set up. Built with `RTSAN_ENABLE=1` on Linux,
//! RealtimeSanitizer checks the same sections for locks and blocking calls
//! (ADR-0010).

use std::time::Instant;

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Guard, Hosted, HostedInstrument, Instrument, ParamBlock, Refused, Reply};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// A host that takes every block and answers silence.
struct Still;

impl Hosted for Still {
    fn submit(
        &mut self,
        _: usize,
        _: &[&[f32]],
        _: &[Event],
        _: &ParamBlock,
    ) -> Result<(), Refused> {
        Ok(())
    }

    fn collect(&mut self, _: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        left.fill(0.0);
        right.fill(0.0);
        Reply::Ready {
            latency: 0,
            tail: 0,
            restart: false,
            work_ns: 0,
        }
    }

    fn ended(&self) -> bool {
        false
    }
}

fn note_on(offset: u32, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_on(0, key, 100),
    }
}

fn note_off(key: u8) -> Event {
    Event {
        offset: 0,
        msg: Msg::note_off(0, key),
    }
}

#[test]
fn a_hosted_instruments_blocks_never_touch_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut synth = HostedInstrument::new(Guard::new(Box::new(Still), 0));
    let mut left = [0.0f32; 64];
    let mut right = [0.0f32; 64];
    let events = [note_on(0, 60), note_on(32, 64)];
    for block in 0..200 {
        let ((), heap) = count_rt_violations(|| {
            // Every fifth block a note lets go with no block of its own: the
            // message waits for the next one (the `pending` path).
            if block % 5 == 0 {
                synth.render(0, &[note_off(60)], &mut [], &mut []);
            }
            synth.render(RATE, &events, &mut left, &mut right);
            let _ = synth.active();
        });
        assert_eq!(
            heap, 0,
            "heap operation in the hosted instrument at block {block}"
        );
    }
}

#[test]
fn an_early_block_and_its_other_outputs_never_touch_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut synth = HostedInstrument::new(Guard::new(Box::new(Still), 0));
    let (mut left, mut right) = ([0.0f32; 64], [0.0f32; 64]);
    for _ in 0..50 {
        assert!(synth.begin(RATE, 64, &[note_on(0, 67)]));
        let ((), heap) = count_rt_violations(|| {
            synth.finish(Instant::now(), &mut left, &mut right);
            let _ = synth.output(0, &mut left, &mut right);
        });
        assert_eq!(heap, 0, "heap operation in begin/finish");
    }
}

#[test]
fn a_silenced_placeholder_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut placeholder = HostedInstrument::placeholder();
    let (mut left, mut right) = ([0.0f32; 64], [0.0f32; 64]);
    for _ in 0..50 {
        let ((), heap) = count_rt_violations(|| {
            placeholder.render(RATE, &[note_on(0, 60)], &mut left, &mut right);
            placeholder.silence();
            let _ = placeholder.active();
        });
        assert_eq!(heap, 0, "heap operation in a placeholder");
    }
}
