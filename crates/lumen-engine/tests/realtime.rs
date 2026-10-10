//! The engine's audio side never touches the heap: not while playing,
//! looping, locating, swapping timelines or following a sample-rate change.
//! Built with `RTSAN_ENABLE=1` on Linux, RealtimeSanitizer also checks for
//! locks and blocking calls in the same sections (ADR-0010).

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{
    Command, Jump, Output, Region, RenderContext, ScrubStyle, Source, Timeline, engine,
};
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

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
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

#[test]
fn scrubbing_never_touches_the_heap() {
    let (mut handle, mut engine) = engine(Timeline::default(), Tone { phase: 0.0 }, 48_000);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_i64 {
        // Both styles, moving like a hand at 60 frames a second, released
        // and grabbed again; then Play over a scrub.
        let at = Ticks(Ticks::quarters(1).0 * ((block / 8) % 40));
        let style = if block < 1_500 {
            ScrubStyle::Loop {
                length: Ticks(Ticks::quarters(1).0 / 4),
            }
        } else {
            ScrubStyle::Tape
        };
        match block % 700 {
            0..=599 if block % 6 == 0 => handle.send(Command::Scrub { at, style }).unwrap(),
            600 => handle.send(Command::EndScrub).unwrap(),
            _ => {}
        }
        if block == 2_900 {
            handle.send(Command::Play).unwrap();
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
    }
}

#[test]
fn delay_compensation_never_touches_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::latent;
    use lumen_engine::{Clip, ClipAudio, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 96_000], vec![0.1; 96_000]],
    });
    let clips: Vec<Clip> = (0..64)
        .map(|i| Clip {
            id: i,
            start: Ticks(i as i64 * 997),
            offset: 0,
            length: 96_000,
            gain: 0.5,
            track: i,
            audio: Arc::clone(&audio),
            shape: Default::default(),
        })
        .collect();
    let (mut clips_handle, player) = clip_player();
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips_handle.set(clips).unwrap();
    assert!(routing.set(&latent(64, 0)).unwrap());
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(2),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_i64 {
        // Latencies change while playing: plans of a new shape carry what
        // is in flight in their delay lines, and the engine follows.
        if block % 200 == 100 {
            let _ = routing.set(&latent(64, (block / 200) as u32));
        }
        match block % 700 {
            350 => handle.send(Command::Locate(Ticks::quarters(1))).unwrap(),
            500 => handle.send(Command::Pause).unwrap(),
            520 => handle.send(Command::Continue).unwrap(),
            600 => handle.send(Command::Stop).unwrap(),
            610 => handle.send(Command::Play).unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
        routing.collect_garbage();
        handle.collect_garbage();
    }
}

#[test]
fn routed_clips_metronome_and_tone_never_touch_the_heap() {
    use std::sync::Arc;

    use lumen_engine::testing::routed;
    use lumen_engine::{Clip, ClipAudio, clip_player, mixer};

    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 96_000], vec![0.1; 96_000]],
    });
    let clips = |offset: i64| -> Vec<Clip> {
        (0..64)
            .map(|i| Clip {
                id: i as u64,
                start: Ticks((i * 7_919 + offset) % (lumen_core::time::PPQ * 8)),
                offset: 0,
                length: 96_000,
                gain: 0.5,
                track: i as u64,
                audio: Arc::clone(&audio),
                // A third fade in and out along curves (read from tables).
                shape: if i % 3 == 0 {
                    lumen_engine::ClipShape {
                        fade_in: lumen_engine::FadeFrames {
                            frames: 4_800,
                            curve: lumen_core::curve::Curve::SLOW,
                        },
                        fade_out: lumen_engine::FadeFrames {
                            frames: 9_600,
                            curve: lumen_core::curve::Curve::S_CURVE,
                        },
                        period: None,
                    }
                } else {
                    Default::default()
                },
            })
            .collect()
    };
    // With a cue bus on channels 3 and 4, a few tracks cued (M4 slice 4).
    let routed = |tracks: u64, variant: u32| {
        let mut r = routed(tracks, variant);
        r.cue = Some(lumen_engine::CueOut {
            channel: 2,
            volume: 0.8,
        });
        for node in r
            .nodes
            .iter_mut()
            .filter(|n| n.id % 9 == u64::from(variant) % 9)
        {
            node.cue = true;
        }
        r
    };
    let (mut clips_handle, player) = clip_player();
    // 64 tracks in groups, with sends to two returns (M4 slice 1).
    let (mut routing, source) = mixer(player);
    let (mut handle, mut engine) = engine(Timeline::default(), source, 48_000);
    clips_handle.set(clips(0)).unwrap();
    assert!(routing.set(&routed(64, 0)).unwrap());
    handle.send(Command::Play).unwrap();
    // The metronome and the test tone run on the audio thread too.
    handle
        .send(Command::SetMetronome(lumen_engine::MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: lumen_engine::ClickSound::Wood,
            channel: 2,
        }))
        .unwrap();
    handle.send(Command::TestTone(true)).unwrap();
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(8),
            true,
        )))
        .unwrap();
    let mut data = vec![0.0; 4 * 128];
    for block in 0..3_000_i64 {
        // Now and then, and as fast as a drag (moved clips crossfade).
        let dragging = (1_000..1_400).contains(&block) && block % 6 == 0;
        if block % 300 == 150 || dragging {
            // Built and sent from the UI side, outside the real-time section.
            let _ = clips_handle.set(clips(block));
        }
        // Levels change as fast as a fader drag; every other one reroutes
        // a track, which crossfades between plans.
        if (1_500..2_000).contains(&block) && block % 6 == 0 {
            let _ = routing.set(&routed(64, (block / 6) as u32));
        }
        if block % 700 == 350 {
            handle.send(Command::Locate(Ticks::quarters(3))).unwrap();
        }
        // Stopped for a while, with edits arriving: taken while idle (M4
        // slice 7).
        match block {
            2_200 => handle.send(Command::Stop).unwrap(),
            2_600 => handle.send(Command::Play).unwrap(),
            2_201..2_600 if block % 10 == 0 => {
                let _ = clips_handle.set(clips(block));
                let _ = routing.set(&routed(64, (block / 10) as u32));
            }
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| engine.process(48_000, &mut data, 4, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        clips_handle.collect_garbage();
        routing.collect_garbage();
        handle.collect_garbage();
    }
}
