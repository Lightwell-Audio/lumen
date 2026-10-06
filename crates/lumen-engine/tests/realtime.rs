//! The engine's audio side never touches the heap: not while playing,
//! looping, locating, swapping timelines or following a sample-rate change.
//! Built with `RTSAN_ENABLE=1` on Linux, RealtimeSanitizer also checks for
//! locks and blocking calls in the same sections (ADR-0010).

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{Command, Output, Region, RenderContext, Source, Timeline, engine};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

/// A source with some per-sample work and state, like a real one.
struct Tone {
    phase: f32,
}

impl Source for Tone {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        let step = 440.0 / cx.sample_rate as f32;
        for channel in 0..out.channels() {
            let mut phase = self.phase;
            for sample in out.channel(channel) {
                *sample += (phase * std::f32::consts::TAU).sin() * 0.1;
                phase = (phase + step).fract();
            }
            if channel + 1 == out.channels() {
                self.phase = phase;
            }
        }
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64) {
        self.phase = 0.0;
    }
}

#[test]
fn process_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let (mut handle, mut engine) = engine(Timeline::default(), Tone { phase: 0.0 }, 48_000);
    let mut data = vec![0.0; 2 * 128];
    let bar = |n: i64| Ticks::quarters(4 * n);

    for block in 0..5_000_u32 {
        // UI-side work happens outside the real-time section, as in the app.
        match block {
            0 => handle.send(Command::Play).unwrap(),
            10 => handle
                .send(Command::SetLoop(Region::new(bar(0), bar(1), true)))
                .unwrap(),
            20 => handle
                .send(Command::SetPunch(Region::new(bar(0), bar(1), true)))
                .unwrap(),
            _ if block % 500 == 250 => handle
                .set_timeline(Timeline::constant(
                    80.0 + f64::from(block / 50),
                    Meter::FOUR_FOUR,
                ))
                .unwrap(),
            _ if block % 700 == 300 => handle.send(Command::Locate(bar(3))).unwrap(),
            2_000 => handle.send(Command::Pause).unwrap(),
            2_100 => handle.send(Command::Play).unwrap(),
            3_000 => handle.send(Command::Stop).unwrap(),
            3_050 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        // The device changes rate halfway through.
        let rate = if block < 2_500 { 48_000 } else { 44_100 };
        let ((), heap) = count_rt_violations(|| engine.process(rate, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
        handle.collect_garbage();
    }
}
