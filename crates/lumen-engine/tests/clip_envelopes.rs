//! Envelopes on audio clips (M7 slice A7, ADR-0090): golden renders of a gain
//! and a pan envelope, the same numbers at every block size, a looping clip
//! reading its envelope at every repeat, an edit under the playhead without a
//! step, and the audio side never touching the heap.

use std::sync::Arc;

use lumen_core::automation::Shape;
use lumen_core::pan::PanLaw;
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipEnvelope, ClipPlayer, ClipShape, ClipsHandle,
    Command, Destination, Engine, EngineHandle, Mixer, NodeSpec, Routing, Timeline, clip_player,
    db_gain, engine, mixer,
};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// Audio of `level` on both channels, long enough for any test here.
fn steady(level: f32) -> Arc<ClipAudio> {
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![vec![level; 400_000], vec![level; 400_000]],
    })
}

fn lane(points: &[(i64, f32, Shape)]) -> AutoLane {
    AutoLane::new(
        points
            .iter()
            .map(|&(at, value, shape)| AutoPoint { at, value, shape })
            .collect(),
    )
    .unwrap()
}

fn clip(audio: &Arc<ClipAudio>, length: i64, envelope: ClipEnvelope, period: Option<i64>) -> Clip {
    Clip {
        id: 1,
        track: 0,
        start: lumen_core::time::Ticks::ZERO,
        offset: 0,
        length,
        gain: 1.0,
        audio: Arc::clone(audio),
        shape: ClipShape {
            period,
            ..Default::default()
        },
        envelope,
    }
}

type Player = Engine<Mixer<ClipPlayer>>;

fn setup(clips: Vec<Clip>) -> (EngineHandle, ClipsHandle, Player) {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing, source) = mixer(player);
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (handle, engine) = engine(Timeline::default(), source, RATE);
    (handle, clips_handle, engine)
}

fn run(engine: &mut Player, frames: usize, block: usize) -> Vec<(f32, f32)> {
    let mut data = vec![0.0; 2 * block];
    let mut out = Vec::with_capacity(frames);
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE, &mut data[..2 * n], 2, n);
        out.extend((0..n).map(|i| (data[i], data[n + i])));
        done += n;
    }
    out
}

#[test]
fn a_gain_envelope_is_a_straight_line_in_db_at_every_block_size() {
    let audio = steady(0.5);
    // 0 dB at the start to −20 dB a second in, then held.
    let env = ClipEnvelope {
        gain: Some(lane(&[
            (0, 0.0, Shape::Linear),
            (48_000, -20.0, Shape::Linear),
        ])),
        pan: None,
    };
    let mut reference: Option<Vec<(f32, f32)>> = None;
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _clips, mut engine) =
            setup(vec![clip(&audio, 100_000, env.clone(), None)]);
        handle.send(Command::Play).unwrap();
        let out = run(&mut engine, 60_000, block);
        // Past the two-millisecond edge fade and before the clip's last one.
        for t in [200_usize, 12_000, 24_000, 47_000, 48_500, 59_000] {
            let db = -20.0 * (t as f32 / 48_000.0).min(1.0);
            let want = 0.5 * db_gain(db);
            assert!(
                (out[t].0 - want).abs() < 2e-3,
                "block {block}, sample {t}: {} against {want}",
                out[t].0
            );
            assert_eq!(out[t].0, out[t].1, "both sides carry the gain");
        }
        match &reference {
            None => reference = Some(out),
            Some(r) => assert!(r == &out, "block {block} differs from block 1"),
        }
    }
}

#[test]
fn a_looping_clip_plays_its_envelope_again_at_every_repeat() {
    let audio = steady(0.5);
    let env = ClipEnvelope {
        gain: Some(lane(&[
            (0, 0.0, Shape::Linear),
            (24_000, -30.0, Shape::Linear),
        ])),
        pan: None,
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip(&audio, 96_000, env, Some(24_000))]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 90_000, 128);
    // The same place in each repeat has the same gain (clear of the seams'
    // edge fades).
    for place in [500_usize, 6_000, 12_000, 23_000] {
        let first = out[place].0;
        for repeat in 1..3 {
            let got = out[place + repeat * 24_000].0;
            assert!(
                (got - first).abs() < 1e-6,
                "repeat {repeat} at {place}: {got} against {first}"
            );
        }
    }
    assert!(out[23_000].0 < out[500].0, "the envelope falls in a repeat");
}

#[test]
fn a_pan_envelope_balances_a_stereo_clip() {
    let audio = steady(0.5);
    // Hard right for the whole clip.
    let env = ClipEnvelope {
        gain: None,
        pan: Some((lane(&[(0, 1.0, Shape::Linear)]), PanLaw::Balance)),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip(&audio, 20_000, env, None)]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 12_000, 128);
    assert_eq!(out[6_000], (0.0, 0.5), "the left side is turned off");
    // Hard left on the next clip.
    let env = ClipEnvelope {
        gain: None,
        pan: Some((lane(&[(0, -1.0, Shape::Linear)]), PanLaw::Balance)),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip(&audio, 20_000, env, None)]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 12_000, 128);
    assert_eq!(out[6_000], (0.5, 0.0));
}

#[test]
fn a_constant_power_pan_drops_a_centred_mono_clip_by_three_db() {
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![vec![0.5; 400_000]],
    });
    let env = ClipEnvelope {
        gain: None,
        pan: Some((lane(&[(0, 0.0, Shape::Linear)]), PanLaw::ConstantPower)),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip(&audio, 20_000, env, None)]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 12_000, 128);
    let want = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((out[6_000].0 - want).abs() < 1e-4, "{:?}", out[6_000]);
    assert!((out[6_000].1 - want).abs() < 1e-4);
}

#[test]
fn an_edit_under_the_playhead_never_steps() {
    let audio = steady(0.5);
    let flat = ClipEnvelope::default();
    let quiet = ClipEnvelope {
        gain: Some(lane(&[(0, -30.0, Shape::Linear)])),
        pan: None,
    };
    let (mut handle, mut clips, mut engine) = setup(vec![clip(&audio, 200_000, flat, None)]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 20_000, 128);
    // The envelope arrives while the clip plays.
    clips.set(vec![clip(&audio, 200_000, quiet, None)]).unwrap();
    out.extend(run(&mut engine, 20_000, 128));
    // Clear of the clip's own two-millisecond fade in at its start.
    let biggest = out[200..]
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).abs())
        .fold(0.0_f32, f32::max);
    // A 5 ms crossfade from 0.5 to 0.016 steps by about 0.002 a sample.
    assert!(biggest < 0.005, "a step of {biggest}");
    assert!(
        (out[39_000].0 - 0.5 * db_gain(-30.0)).abs() < 1e-3,
        "the new envelope plays"
    );
}

#[test]
fn enveloped_clips_never_touch_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let audio = steady(0.5);
    let env = ClipEnvelope {
        gain: Some(lane(&[
            (0, 0.0, Shape::Linear),
            (30_000, -12.0, Shape::Curve(Default::default())),
            (60_000, 0.0, Shape::Step),
        ])),
        pan: Some((
            lane(&[(0, -1.0, Shape::Linear), (50_000, 1.0, Shape::Linear)]),
            PanLaw::ConstantPower,
        )),
    };
    let clips: Vec<Clip> = (0..16)
        .map(|i| Clip {
            id: i,
            track: 0,
            ..clip(&audio, 300_000, env.clone(), Some(40_000 + i as i64 * 7))
        })
        .collect();
    let (mut handle, _clips, mut engine) = setup(clips);
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * 128];
    for block in 0..2_000 {
        let ((), heap) = count_rt_violations(|| engine.process(RATE, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        let _ = handle.transport();
    }
}
