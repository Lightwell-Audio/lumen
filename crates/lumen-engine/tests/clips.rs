//! Golden renders of audio clips: every output sample is compared with the
//! value it must have, at several block sizes (M2 slice 6).

use std::sync::Arc;

use lumen_core::automation::Shape;
use lumen_core::curve::{Curve, Handle};
use lumen_core::fade::FadeShape;
use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipEnvelope, ClipPlayer, ClipShape, ClipsHandle,
    Command, Destination, Engine, EngineHandle, FadeFrames, Mixer, NodeSpec, Region, Routing,
    Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;
/// 2 ms at 48 kHz: the edge and jump fade.
const FADE: i64 = 96;

/// Audio whose every sample is distinct: left `(i + 1) / 1e7`, right negated.
fn ramp(frames: usize, rate: u32) -> Arc<ClipAudio> {
    let left: Vec<f32> = (0..frames).map(|i| (i + 1) as f32 / 1e7).collect();
    let right = left.iter().map(|v| -v).collect();
    Arc::new(ClipAudio {
        sample_rate: rate,
        channels: vec![left, right],
    })
}

fn bars(n: i64) -> Ticks {
    Ticks::quarters(4 * n)
}

type Player = Engine<Mixer<ClipPlayer>>;

/// Clips on track 0, which plays straight to the device at unity.
fn setup(clips: Vec<Clip>) -> (EngineHandle, ClipsHandle, Player) {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing, source) = mixer(player);
    let unity = NodeSpec::unity(0, Destination::Hardware(0));
    assert!(
        routing
            .set(&Routing {
                nodes: vec![unity],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (handle, engine) = engine(Timeline::default(), source, RATE);
    (handle, clips_handle, engine)
}

/// Renders `frames` in blocks of `block`; returns (left, right) per frame.
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

/// What a clip contributes at `pos` frames into it, with its edge fades.
fn expected(audio: &ClipAudio, offset: i64, length: i64, pos: i64) -> (f32, f32) {
    let edge = FADE.min(length / 2).max(1);
    let gain = ((pos + 1).min(edge).min(length - pos) as f32 / edge as f32).min(1.0);
    let i = (offset + pos) as usize;
    (audio.channels[0][i] * gain, audio.channels[1][i] * gain)
}

#[test]
fn a_clip_plays_on_its_exact_samples_at_every_block_size() {
    let audio = ramp(20_000, RATE);
    let (offset, length) = (100, 10_000);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: bars(1), // sample 96,000 at 120 BPM
        offset,
        length,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let mut reference = None;
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _clips, mut engine) = setup(vec![clip.clone()]);
        handle.send(Command::Play).unwrap();
        let out = run(&mut engine, 110_000, block);
        for (t, &got) in out.iter().enumerate() {
            let pos = t as i64 - 96_000;
            let want = if (0..length).contains(&pos) {
                expected(&audio, offset, length, pos)
            } else {
                (0.0, 0.0)
            };
            assert_eq!(got, want, "block {block}, sample {t}");
        }
        match &reference {
            None => reference = Some(out),
            Some(r) => assert!(r == &out, "block {block} differs"),
        }
    }
}

#[test]
fn splitting_contiguous_source_is_a_null_render_at_every_block_size() {
    let audio = ramp(60_000, RATE);
    let whole = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 0.75,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let mut left = whole.clone();
    left.length = 24_000;
    let mut right = whole.clone();
    right.id = 2;
    right.start = Ticks::quarters(1);
    right.offset = 24_000;
    right.length = 24_000;
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 50_000, block);
        let (mut handle, _, mut split) = setup(vec![left.clone(), right.clone()]);
        handle.send(Command::Play).unwrap();
        let actual = run(&mut split, 50_000, block);
        assert_eq!(actual, expected, "block {block}");
    }
}

#[test]
fn splitting_inside_the_outer_edge_fade_keeps_its_remaining_ramp() {
    let audio = ramp(60_000, RATE);
    let whole = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 0.75,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let mut left = whole.clone();
    left.length = 24;
    let mut right = whole.clone();
    right.id = 2;
    right.start = Ticks(lumen_core::time::PPQ * 24 / 24_000);
    right.offset = 24;
    right.length -= 24;
    assert_eq!(Timeline::default().sample_at(right.start, RATE), 24);
    for block in [1, 64, 128, 333] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 50_000, block);
        let (mut handle, _, mut split) = setup(vec![left.clone(), right.clone()]);
        handle.send(Command::Play).unwrap();
        assert_eq!(run(&mut split, 50_000, block), expected, "block {block}");
    }
}

#[test]
fn split_fade_windows_continue_the_original_curve_without_a_seam_dip() {
    let audio = ramp(60_000, RATE);
    let whole = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 0.75,
        track: 0,
        audio: Arc::clone(&audio),
        shape: ClipShape {
            fade_in: FadeFrames {
                frames: 30_000,
                span: 30_000,
                curve: Curve::S_CURVE,
                ..Default::default()
            },
            fade_out: FadeFrames {
                frames: 12_000,
                span: 12_000,
                curve: Curve::S_CURVE,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let mut left = whole.clone();
    left.length = 24_000;
    left.shape.fade_in.frames = 24_000;
    left.shape.fade_out = FadeFrames::default();
    let mut right = whole.clone();
    right.id = 2;
    right.start = Ticks::quarters(1);
    right.offset = 24_000;
    right.length = 24_000;
    right.shape.fade_in = FadeFrames {
        frames: 6_000,
        offset: 24_000,
        span: 30_000,
        curve: Curve::S_CURVE,
    };
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 50_000, block);
        let (mut handle, _, mut split) = setup(vec![left.clone(), right.clone()]);
        handle.send(Command::Play).unwrap();
        let actual = run(&mut split, 50_000, block);
        assert_eq!(actual, expected, "block {block}");
    }
}

#[test]
fn splitting_at_the_end_of_a_fade_in_keeps_its_full_level() {
    let audio = ramp(60_000, RATE);
    let whole = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 0.75,
        track: 0,
        audio: Arc::clone(&audio),
        shape: ClipShape {
            fade_in: FadeFrames {
                frames: 24_000,
                span: 24_000,
                curve: Curve::S_CURVE,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let mut left = whole.clone();
    left.length = 24_000;
    let mut right = whole.clone();
    right.id = 2;
    right.start = Ticks::quarters(1);
    right.offset = 24_000;
    right.length = 24_000;
    right.shape.fade_in = FadeFrames::default();
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 50_000, block);
        let (mut handle, _, mut split) = setup(vec![left.clone(), right.clone()]);
        handle.send(Command::Play).unwrap();
        assert_eq!(run(&mut split, 50_000, block), expected, "block {block}");
    }
}

#[test]
fn unrelated_join_and_intentional_seam_fades_keep_the_edge_dip() {
    let audio = steady(60_000);
    let mut left = steady_clip(&audio, 1.0);
    left.length = 24_000;
    let mut right = left.clone();
    right.id = 8;
    right.start = Ticks::quarters(1);
    right.offset = 25_000; // source discontinuity
    let (mut handle, _, mut engine) = setup(vec![left, right]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_000, 128);
    assert_eq!(out[24_000].0, 0.5 * (1.0 / 96.0), "unrelated join dips");

    let mut continuous = steady_clip(&audio, 1.0);
    continuous.length = 24_000;
    let mut faded = continuous.clone();
    faded.id = 9;
    faded.start = Ticks::quarters(1);
    faded.offset = 24_000;
    faded.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![continuous, faded]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_000, 128);
    assert!(out[24_000].0 < 1e-5, "explicit fade remains in force");

    let lane = |points: &[(i64, f32, Shape)]| {
        AutoLane::new(
            points
                .iter()
                .map(|&(at, value, shape)| AutoPoint { at, value, shape })
                .collect(),
        )
        .unwrap()
    };
    let mut whole = steady_clip(&audio, 1.0);
    whole.length = 48_000;
    whole.envelope = ClipEnvelope {
        gain: Some(lane(&[
            (0, -6.0, Shape::Curve(Curve::S_CURVE)),
            (24_000, -3.0, Shape::Curve(Curve::S_CURVE)),
            (48_000, 0.0, Shape::Linear),
        ])),
        pan: Some((
            lane(&[(0, -1.0, Shape::Linear), (24_000, 0.0, Shape::Linear)]),
            lumen_core::pan::PanLaw::Balance,
        )),
    };
    let mut enveloped_left = whole.clone();
    enveloped_left.length = 24_000;
    let mut enveloped_right = whole.clone();
    enveloped_right.id = 10;
    enveloped_right.start = Ticks::quarters(1);
    enveloped_right.offset = 24_000;
    enveloped_right.length = 24_000;
    enveloped_right.envelope = ClipEnvelope {
        gain: Some(lane(&[
            (-24_000, -6.0, Shape::Curve(Curve::S_CURVE)),
            (0, -3.0, Shape::Curve(Curve::S_CURVE)),
            (24_000, 0.0, Shape::Linear),
        ])),
        pan: Some((
            lane(&[(-24_000, -1.0, Shape::Linear), (0, 0.0, Shape::Linear)]),
            lumen_core::pan::PanLaw::Balance,
        )),
    };
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 50_000, block);
        let (mut handle, _, mut split) =
            setup(vec![enveloped_left.clone(), enveloped_right.clone()]);
        handle.send(Command::Play).unwrap();
        let actual = run(&mut split, 50_000, block);
        assert_eq!(actual, expected, "enveloped split, block {block}");
    }

    // A discontinuity in the baked gain envelope is an intentional edge.
    enveloped_right.envelope.gain = Some(lane(&[(0, -30.0, Shape::Step)]));
    let (mut handle, _, mut engine) = setup(vec![enveloped_left, enveloped_right]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_000, 128);
    assert!(
        out[24_000].0.abs() < 0.001,
        "envelope jump retains edge dip"
    );
}

#[test]
fn joined_crossfade_matches_equal_power_reference_and_clamps_missing_tail() {
    let audio = steady(32_000);
    let mut left = steady_clip(&audio, 0.8);
    left.length = 24_000;
    left.shape.fade_out = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let mut right = steady_clip(&audio, 0.6);
    right.id = 8;
    right.start = Ticks::quarters(1);
    right.offset = 28_000;
    right.length = 4_000;
    right.shape.crossfade = true;
    right.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![left.clone(), right.clone()]);
    handle
        .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 26_000, 128);
    assert!(
        (out[23_999].0 - 0.4).abs() < 1e-6,
        "join overrides outgoing fade-out"
    );
    for i in 0..1_000 {
        let p = (i as f32 + 0.5) / 1_000.0;
        let want = 0.4 * (p * std::f32::consts::FRAC_PI_2).cos()
            + 0.3 * (p * std::f32::consts::FRAC_PI_2).sin();
        assert!(
            (out[24_000 + i].0 - want).abs() < 3e-5,
            "frame {i}: got {}, want {want}",
            out[24_000 + i].0
        );
    }

    // An incoming fade-out may begin before this crossfade ends; its own
    // envelope continues smoothly after the outgoing tail reaches zero.
    let mut fading_right = right.clone();
    fading_right.shape.fade_out = FadeFrames {
        frames: 3_500,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![left.clone(), fading_right]);
    handle
        .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_100, 128);
    assert!((out[25_001].0 - out[25_000].0).abs() < 0.002);

    // No samples remain after the predecessor's visible clip. The engine
    // skips the crossfade and retains its regular edge declick.
    let short = steady(24_000);
    let mut left = steady_clip(&short, 1.0);
    left.length = 24_000;
    let mut right = steady_clip(&short, 1.0);
    right.id = 9;
    right.start = Ticks::quarters(1);
    right.shape.crossfade = true;
    right.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![left, right]);
    handle
        .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_000, 128);
    assert_eq!(out[24_000].0, 0.5 / FADE as f32);

    // A short available source tail shortens both sides of the fade. The
    // incoming fade reaches unity as the truncated outgoing tail reaches 0.
    let short_tail = steady(24_300);
    let mut left = steady_clip(&short_tail, 1.0);
    left.length = 24_000;
    let mut right = steady_clip(&short_tail, 1.0);
    right.id = 11;
    right.start = Ticks::quarters(1);
    right.shape.crossfade = true;
    right.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![left, right]);
    handle
        .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 24_500, 128);
    for i in 0..300 {
        let p = (i as f32 + 0.5) / 300.0;
        let want = 0.5
            * ((p * std::f32::consts::FRAC_PI_2).cos() + (p * std::f32::consts::FRAC_PI_2).sin());
        assert!(
            (out[24_000 + i].0 - want).abs() < 3e-5,
            "short tail frame {i}"
        );
    }
    assert!((out[24_300].0 - 0.5).abs() < 1e-6);

    // A one-frame gap breaks exact adjacency, so the predecessor tail is
    // ignored even when its source has plenty of audio left.
    let audio = steady(32_000);
    let mut left = steady_clip(&audio, 1.0);
    left.length = 24_000;
    let mut right = steady_clip(&audio, 1.0);
    right.id = 10;
    right.start = Ticks(59 + Ticks::quarters(1).0);
    assert_eq!(
        Timeline::constant(120.0, Meter::FOUR_FOUR).sample_at(right.start, RATE),
        24_001
    );
    right.shape.crossfade = true;
    right.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let (mut handle, _, mut engine) = setup(vec![left, right]);
    handle
        .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 25_000, 128);
    assert_eq!(out[24_000].0, 0.0);
    assert_eq!(out[24_001].0, 0.5 / FADE as f32);
}

#[test]
fn join_tail_is_safe_after_seek_and_during_a_clip_list_swap() {
    let audio = steady(40_000);
    let mut left = steady_clip(&audio, 1.0);
    left.length = 24_000;
    let mut right = steady_clip(&audio, 1.0);
    right.id = 20;
    right.start = Ticks::quarters(1);
    right.offset = 30_000;
    right.length = 4_000;
    right.shape.crossfade = true;
    right.shape.fade_in = FadeFrames {
        frames: 1_000,
        curve: Curve::LINEAR,
        ..Default::default()
    };
    let timeline = Timeline::constant(120.0, Meter::FOUR_FOUR);

    // Seeking directly into the tail must use the clip's target level even
    // though its visible body has not been rendered to initialise that level.
    let (mut handle, _, mut engine) = setup(vec![left.clone(), right.clone()]);
    handle.set_timeline(timeline.clone()).unwrap();
    handle
        .send(Command::Locate(timeline.ticks_at(24_100, RATE)))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 128, 128);
    let p = 196.5 / 1_000.0;
    let want =
        0.5 * ((p * std::f32::consts::FRAC_PI_2).cos() + (p * std::f32::consts::FRAC_PI_2).sin());
    assert!(
        (out[96].0 - want).abs() < 2e-5,
        "seek tail got {}, want {want}",
        out[96].0
    );

    // Removing the incoming clip while its crossfade is active fades both
    // its body and the predecessor tail with the list-swap ramp.
    let (mut handle, mut clips, mut engine) = setup(vec![left.clone(), right.clone()]);
    handle.set_timeline(timeline.clone()).unwrap();
    handle.send(Command::Play).unwrap();
    let before = run(&mut engine, 24_100, 128);
    clips.set(vec![steady_clip(&audio, 1.0)]).unwrap();
    let after = run(&mut engine, 128, 128);
    assert!(
        (before.last().unwrap().0 - after[0].0).abs() < 0.01,
        "clip-list edit stepped across join tail: {} to {}",
        before.last().unwrap().0,
        after[0].0
    );

    // Moving the predecessor while leaving the incoming clip in place must
    // ramp the replacement tail against the retired tail instead of adding
    // both at full level.
    let (mut handle, mut clips, mut engine) = setup(vec![left.clone(), right.clone()]);
    handle.set_timeline(timeline.clone()).unwrap();
    handle.send(Command::Play).unwrap();
    let _ = run(&mut engine, 24_100, 128);
    let mut moved_left = left.clone();
    moved_left.offset = 1_000;
    clips.set(vec![moved_left, right.clone()]).unwrap();
    let after_move = run(&mut engine, 128, 128);
    assert!(
        after_move[0].0 < 0.75,
        "replacement tail doubled during predecessor move: {}",
        after_move[0].0
    );

    // A gain change while the predecessor body is already past its visible
    // end glides on the tail instead of resetting to the new target.
    let mut long_right = right;
    long_right.shape.fade_in.frames = 2_000;
    let (mut handle, mut clips, mut engine) = setup(vec![left.clone(), long_right.clone()]);
    handle.set_timeline(timeline).unwrap();
    handle.send(Command::Play).unwrap();
    let before = run(&mut engine, 24_100, 128);
    left.gain = 0.0;
    clips.set(vec![left, long_right]).unwrap();
    let after_gain = run(&mut engine, 128, 128);
    assert!(
        (before.last().unwrap().0 - after_gain[0].0).abs() < 0.002,
        "tail gain stepped: {} to {}",
        before.last().unwrap().0,
        after_gain[0].0
    );
}

#[test]
fn the_loop_wrap_crossfades_and_stays_on_the_exact_sample() {
    let audio = ramp(400_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle
        .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle.send(Command::Locate(bars(1))).unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 200_000, 128);
    let left = |frame: usize| out[frame].0;
    let source = |sample: usize| audio.channels[0][sample];

    // Starting playback fades in over 2 ms...
    assert!(left(0) < source(96_000) / 10.0, "no fade-in at play");
    assert_eq!(left(FADE as usize), source(96_000 + FADE as usize));
    // At the wrap from bar 3 back to bar 2 (96,000 frames in), the loop start
    // fades in over 2 ms while what would have followed bar 3 fades out:
    // a crossfade, so a loop that does not line up does not click.
    assert_eq!(left(95_999), source(191_999));
    for k in 0..FADE as usize {
        let fade_in = (k + 1) as f32 / FADE as f32;
        let want = source(96_000 + k) * fade_in + source(192_000 + k) * (1.0 - fade_in);
        assert!((left(96_000 + k) - want).abs() < 1e-9, "wrap frame {k}");
    }
    // After the crossfade the loop plays its exact samples again.
    assert_eq!(left(96_000 + FADE as usize), source(96_000 + FADE as usize));
    assert_eq!(left(150_000), source(150_000));
}

#[test]
fn stopping_fades_out_instead_of_cutting() {
    let audio = ramp(400_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    run(&mut engine, 10_000, 128);
    for stop in [Command::Stop, Command::Pause] {
        handle.send(Command::Play).unwrap();
        run(&mut engine, 1_000, 128);
        let at = handle.transport().sample as usize;
        handle.send(stop).unwrap();
        let out = run(&mut engine, 256, 128);
        // The audio that was playing ramps down to silence over 2 ms...
        for (k, &(left, _)) in out.iter().enumerate().take(FADE as usize) {
            let want = audio.channels[0][at + k] * (1.0 - (k + 1) as f32 / FADE as f32);
            assert!((left - want).abs() < 1e-9, "{stop:?} frame {k}");
        }
        // ...then silence, and the transport is where it should be.
        assert!(
            out[FADE as usize..].iter().all(|&s| s == (0.0, 0.0)),
            "{stop:?}"
        );
        assert!(!handle.transport().playing);
    }
}

#[test]
fn a_locate_lands_on_the_exact_sample_and_fades_in() {
    let audio = ramp(400_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 0.5,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    run(&mut engine, 1_024, 128);
    handle.send(Command::Locate(bars(2))).unwrap();
    let out = run(&mut engine, 1_024, 128);
    let full = |i: usize| audio.channels[0][192_000 + i] * 0.5;
    // Rising over 2 ms from the exact sample, then exact.
    assert!(out[0].0 > 0.0 && out[0].0 < full(0));
    assert!(out.windows(2).take(95).all(|w| w[1].0 > w[0].0));
    assert_eq!(out[FADE as usize].0, full(FADE as usize));
    assert_eq!(out[500].1, -full(500));
}

#[test]
fn mono_plays_on_both_sides_and_gain_applies() {
    let mono = Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![vec![0.5; 10_000]],
    });
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 10_000,
        gain: 0.5,
        track: 0,
        audio: mono,
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 2_000, 256);
    assert_eq!(out[1_000], (0.25, 0.25));
}

#[test]
fn clips_at_another_sample_rate_stay_silent() {
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 10_000,
        gain: 1.0,
        track: 0,
        audio: ramp(10_000, 44_100),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 5_000, 256);
    assert!(out.iter().all(|&s| s == (0.0, 0.0)));
}

#[test]
fn clips_follow_tempo_changes() {
    let audio = ramp(20_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: bars(1),
        offset: 0,
        length: 1_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle
        .set_timeline(Timeline::constant(60.0, Meter::FOUR_FOUR))
        .unwrap();
    handle.send(Command::Play).unwrap();
    // At 60 BPM bar 2 starts at 192,000 samples, not 96,000.
    let out = run(&mut engine, 193_000, 1_000);
    assert_eq!(out[96_500], (0.0, 0.0));
    assert_eq!(out[192_500], expected(&audio, 0, 1_000, 500));
}

#[test]
fn a_new_clip_list_replaces_the_old_and_the_old_is_handed_back() {
    let audio = ramp(10_000, RATE);
    let clip = |start| Clip {
        envelope: Default::default(),
        id: 1,
        start,
        offset: 0,
        length: 1_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, mut clips, mut engine) = setup(vec![clip(Ticks::ZERO)]);
    handle.send(Command::Play).unwrap();
    run(&mut engine, 256, 256);
    clips.set(vec![clip(Ticks::quarters(1))]).unwrap();
    run(&mut engine, 256, 256);
    assert_eq!(
        clips.collect_garbage(),
        2,
        "the empty initial list and the first list"
    );
    // 512 frames have played. The new clip starts at sample 24,000 (one
    // quarter at 120 BPM), which is frame 23,488 of this run.
    let out = run(&mut engine, 24_000, 512);
    assert_eq!(out[23_487], (0.0, 0.0));
    assert_eq!(out[23_488 + 500], expected(&audio, 0, 1_000, 500));
}

#[test]
fn resending_the_same_timeline_while_playing_changes_nothing() {
    // The app resends the timeline after edits; one that does not move the
    // playhead must not touch the audio (owner: arming a track popped, and
    // dragging pan crackled, from a fade-in on every resend).
    let audio = ramp(20_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 20_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip.clone()]);
    handle.send(Command::Play).unwrap();
    let reference = run(&mut engine, 12_000, 128);

    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 4_000, 128);
    for _ in 0..8 {
        handle.set_timeline(Timeline::default()).unwrap();
        out.extend(run(&mut engine, 1_000, 128));
    }
    assert_eq!(out, reference);
}

#[test]
fn a_tempo_change_while_playing_crossfades() {
    // Halving the tempo keeps the bar and doubles the sample position, so
    // the audio jumps; like every jump, it crossfades rather than cutting.
    let audio = ramp(40_000, RATE);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 40_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 10_000, 100);
    handle
        .set_timeline(Timeline::constant(60.0, Meter::FOUR_FOUR))
        .unwrap();
    out.extend(run(&mut engine, 1_000, 100));
    // The ramp rises 1e-7 a frame; a cut to silence would step by 1e-3.
    let steepest = out[9_900..10_400]
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).abs())
        .fold(0.0, f32::max);
    assert!(steepest < 2e-5, "steepest step {steepest}");
    // After the fade it plays from the new position: frame 20,000 of the file.
    assert_eq!(out[10_200], expected(&audio, 0, 40_000, 20_200));
}

/// Steady audio, so any step in the output is a change of level.
fn steady(frames: usize) -> Arc<ClipAudio> {
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![vec![0.5; frames], vec![0.5; frames]],
    })
}

fn steady_clip(audio: &Arc<ClipAudio>, gain: f32) -> Clip {
    Clip {
        envelope: Default::default(),
        id: 7,
        start: Ticks::ZERO,
        offset: 0,
        length: audio.frames() as i64,
        gain,
        track: 0,
        audio: Arc::clone(audio),
        shape: Default::default(),
    }
}

/// The largest change from one frame to the next, on either side.
fn steepest(out: &[(f32, f32)]) -> f32 {
    out.windows(2)
        .map(|w| (w[1].0 - w[0].0).abs().max((w[1].1 - w[0].1).abs()))
        .fold(0.0, f32::max)
}

/// Full scale in 20 ms, on audio at 0.5.
const MAX_STEP: f32 = 0.5 * 50.0 / RATE as f32 + 1e-6;

#[test]
fn clip_gain_and_mute_changes_glide_instead_of_stepping() {
    // Owner: dragging volume or pan crackled during playback, and muting
    // clicked: each new clip list changed the level at once.
    let audio = steady(48_000);
    let (mut handle, mut clips, mut engine) = setup(vec![steady_clip(&audio, 1.0)]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 1_000, 128);
    // A drag: a new level every 16 ms (one screen frame).
    // The app frees replaced lists every frame; so does this test.
    let mut send = |gain| {
        clips.collect_garbage();
        clips.set(vec![steady_clip(&audio, gain)]).unwrap();
    };
    for gain in [0.8, 0.6, 0.2, 1.0] {
        send(gain);
        out.extend(run(&mut engine, 768, 128));
    }
    // Mute (gain 0), then unmute.
    send(0.0);
    out.extend(run(&mut engine, 2_000, 128));
    assert_eq!(out.last(), Some(&(0.0, 0.0)), "muted reaches silence");
    send(1.0);
    out.extend(run(&mut engine, 2_000, 128));
    assert_eq!(out.last(), Some(&(0.5, 0.5)), "unmuted reaches full level");
    assert!(
        steepest(&out[200..]) <= MAX_STEP,
        "steepest {}",
        steepest(&out[200..])
    );
}

#[test]
fn a_clip_that_appears_partway_fades_in() {
    // Owner: the first play after opening a project popped. A clip whose
    // audio finishes loading during playback joins partway through.
    let audio = steady(48_000);
    let (mut handle, mut clips, mut engine) = setup(Vec::new());
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 5_000, 128);
    clips.set(vec![steady_clip(&audio, 1.0)]).unwrap();
    out.extend(run(&mut engine, 2_000, 128));
    assert!(steepest(&out) <= MAX_STEP, "steepest {}", steepest(&out));
    assert_eq!(out.last(), Some(&(0.5, 0.5)));
}

#[test]
fn a_clip_heard_from_its_start_keeps_its_attack() {
    // A new clip that starts on its own first sample is not slowed by the
    // glide: only its 2 ms edge fade applies.
    let audio = steady(48_000);
    let mut late = steady_clip(&audio, 1.0);
    late.start = Ticks::quarters(1); // sample 24,000
    let (mut handle, _clips, mut engine) = setup(vec![late]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 24_200, 128);
    assert_eq!(out[24_000 + FADE as usize], (0.5, 0.5));
}

/// A smooth 200 Hz tone at 0.5: from one frame to the next it moves by at
/// most 0.5 × 2π × 200 / 48,000 ≈ 0.013.
fn tone(frames: usize) -> Arc<ClipAudio> {
    let wave: Vec<f32> = (0..frames)
        .map(|i| 0.5 * (i as f32 * std::f32::consts::TAU * 200.0 / RATE as f32).sin())
        .collect();
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![wave.clone(), wave],
    })
}

/// The tone's own steepest step, plus what a 5 ms crossfade between two
/// unrelated points of it can add (at most 1.0 over 240 frames).
const TONE_STEP: f32 = 0.0131 + 1.0 / 240.0;

#[test]
fn dragging_a_playing_clip_crossfades_instead_of_clicking() {
    // Owner (after M3 slice 6c): dragging a clip while it played crackled.
    // Each new position made the audio under the playhead jump to another
    // point of the file, once per screen frame.
    let audio = tone(200_000);
    let clip = |start: Ticks| Clip {
        envelope: Default::default(),
        id: 3,
        start,
        offset: 0,
        length: 200_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, mut clips, mut engine) = setup(vec![clip(Ticks::ZERO)]);
    // From bar 2 (sample 96,000), well inside the clip however it moves.
    handle.send(Command::Locate(bars(1))).unwrap();
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 4_000, 128);
    // The app frees replaced lists every frame; so does this test.
    let mut send = |list: Vec<Clip>| {
        clips.collect_garbage();
        clips.set(list).unwrap();
    };
    // A drag: a new position every screen frame (768 frames), each landing
    // on another part of the wave.
    for step in 1..=12 {
        send(vec![clip(Ticks(Ticks::quarters(1).0 * step / 7))]);
        out.extend(run(&mut engine, 768, 128));
    }
    let settled = out.len();
    // Dragged off the playhead (it fades out), back under it, then deleted.
    send(vec![clip(bars(8))]);
    out.extend(run(&mut engine, 768, 128));
    assert!(
        out[out.len() - 400..].iter().all(|&s| s == (0.0, 0.0)),
        "silent once the clip has left the playhead"
    );
    send(vec![clip(Ticks::ZERO)]);
    out.extend(run(&mut engine, 768, 128));
    send(Vec::new());
    out.extend(run(&mut engine, 768, 128));
    assert_eq!(out.last(), Some(&(0.0, 0.0)), "deleted, it is silent");
    assert!(
        steepest(&out) <= TONE_STEP,
        "steepest step {}",
        steepest(&out)
    );

    // After each crossfade the clip plays its exact samples at its new place.
    let start = Timeline::default().sample_at(Ticks(Ticks::quarters(12).0 / 7), RATE);
    let at = settled - 1;
    let want = audio.channels[0][(96_000 + at as i64 - start) as usize];
    assert!((out[at].0 - want).abs() < 1e-6, "{} vs {want}", out[at].0);
}

#[test]
fn lists_sent_while_stopped_never_play_late() {
    // Found by the 6d E2E: each frame of a fade drag sent a list, which
    // queued while stopped; on Play the engine stepped through them, a
    // crossfade each, so the clip played without its new fade at first.
    let audio = tone(48_000);
    let clip = |fade: i64| Clip {
        envelope: Default::default(),
        id: 3,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: ClipShape {
            fade_in: FadeFrames {
                frames: fade,
                curve: Curve::LINEAR,
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let (mut handle, mut clips, mut engine) = setup(vec![clip(0)]);
    for fade in [100, 1_000, 10_000] {
        clips.set(vec![clip(fade)]).unwrap();
    }
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 2_000, 128);
    // The newest list, with its 10,000-frame fade, from the first frame.
    for (t, &(left, _)) in out.iter().enumerate() {
        let level = (t as f32 + 0.5) / 10_000.0;
        assert!(left.abs() <= 0.5 * level + 1e-6, "frame {t}: {left}");
    }
    assert_eq!(clips.collect_garbage(), 4, "every older list came back");
}

#[test]
fn a_list_that_moves_no_clip_does_not_crossfade() {
    // Volume drags resend every frame; those must not cost a crossfade.
    let audio = tone(48_000);
    let clip = Clip {
        envelope: Default::default(),
        id: 3,
        start: Ticks::ZERO,
        offset: 0,
        length: 48_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: Default::default(),
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip.clone()]);
    handle.send(Command::Play).unwrap();
    let reference = run(&mut engine, 8_000, 128);
    let (mut handle, mut resent, mut engine) = setup(vec![clip.clone()]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 1_000, 128);
    for _ in 0..7 {
        resent.collect_garbage();
        resent.set(vec![clip.clone()]).unwrap();
        out.extend(run(&mut engine, 1_000, 128));
    }
    assert_eq!(out, reference);
}

/// Plays `clip` from the start at several block sizes and checks every
/// frame against `want(pos)` (`pos` frames into the clip at bar 1).
fn golden(clip: &Clip, frames: usize, want: impl Fn(i64) -> (f32, f32)) {
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _clips, mut engine) = setup(vec![clip.clone()]);
        handle.send(Command::Play).unwrap();
        let out = run(&mut engine, frames, block);
        for (t, &(left, right)) in out.iter().enumerate() {
            let pos = t as i64 - 96_000;
            let (l, r) = if (0..clip.length).contains(&pos) {
                want(pos)
            } else {
                (0.0, 0.0)
            };
            let close = |a: f32, b: f32| (a - b).abs() <= 1e-6 * b.abs().max(1e-7);
            assert!(
                close(left, l) && close(right, r),
                "block {block}, frame {pos}: got ({left}, {right}), want ({l}, {r})"
            );
        }
    }
}

#[test]
fn fades_sound_as_drawn_in_every_shape_and_free_curve() {
    let audio = ramp(30_000, RATE);
    // A curve bent freely by the owner's C3 handle.
    let free = Curve::from_handle(Handle {
        dot: [0.35, 0.7],
        s: 0.4,
    });
    for (fade_in_curve, fade_out_curve) in [
        (Curve::LINEAR, Curve::LINEAR),
        (FadeShape::Slow.curve(), FadeShape::Fast.curve()),
        (FadeShape::Fast.curve(), FadeShape::SCurve.curve()),
        (FadeShape::SCurve.curve(), free),
        (free, FadeShape::Slow.curve()),
    ] {
        let (length, fade_in, fade_out) = (20_000, 4_800, 9_600);
        let clip = Clip {
            envelope: Default::default(),
            id: 1,
            start: bars(1),
            offset: 0,
            length,
            gain: 1.0,
            track: 0,
            audio: Arc::clone(&audio),
            shape: ClipShape {
                fade_in: FadeFrames {
                    frames: fade_in,
                    curve: fade_in_curve,
                    ..Default::default()
                },
                fade_out: FadeFrames {
                    frames: fade_out,
                    curve: fade_out_curve,
                    ..Default::default()
                },
                crossfade: false,
                period: None,
                phase: 0,
            },
        };
        // The curve the arrangement draws (`Curve::at`), heard to within
        // 2·10⁻⁵ of the level (the engine reads it from a table).
        for block in [1, 64, 333, 4_096] {
            let (mut handle, _clips, mut engine) = setup(vec![clip.clone()]);
            handle.send(Command::Play).unwrap();
            let out = run(&mut engine, 120_000, block);
            for (t, &(left, _)) in out.iter().enumerate() {
                let pos = t as i64 - 96_000;
                if !(0..length).contains(&pos) {
                    assert_eq!(left, 0.0);
                    continue;
                }
                let (full, _) = expected(&audio, 0, length, pos);
                let mut g = 1.0;
                if pos < fade_in {
                    g *= fade_in_curve.at((pos as f32 + 0.5) / fade_in as f32);
                }
                if length - pos <= fade_out {
                    g *= fade_out_curve.at(((length - pos) as f32 - 0.5) / fade_out as f32);
                }
                assert!(
                    (left - full * g).abs() <= 2e-5 * full.abs() + 1e-12,
                    "block {block}, frame {pos}: got {left}, want {}",
                    full * g
                );
            }
        }
    }
}

#[test]
fn a_looping_clip_repeats_its_content_with_clean_seams() {
    let audio = ramp(30_000, RATE);
    let (offset, period, length) = (100, 3_000, 10_000);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        start: bars(1),
        offset,
        length,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: ClipShape {
            period: Some(period),
            ..Default::default()
        },
    };
    golden(&clip, 110_000, |pos| {
        // Each repeat is the content from the offset again, with its own
        // edge fades; the last one is cut short by the clip's end.
        let repeat = pos / period * period;
        expected(&audio, offset, period.min(length - repeat), pos - repeat)
    });
}

#[test]
fn splitting_inside_a_loop_preserves_source_and_envelope_phase() {
    let audio = ramp(12_000, RATE);
    let envelope = ClipEnvelope {
        gain: Some(
            AutoLane::new(vec![
                AutoPoint {
                    at: 0,
                    value: -12.0,
                    shape: Shape::Curve(Curve::S_CURVE),
                },
                AutoPoint {
                    at: 3_000,
                    value: 0.0,
                    shape: Shape::Linear,
                },
            ])
            .unwrap(),
        ),
        pan: None,
    };
    let period_ticks = Ticks(lumen_core::time::PPQ * 3_000 / 24_000);
    let split_ticks = Ticks(lumen_core::time::PPQ * 1_000 / 24_000);
    let whole = Clip {
        envelope,
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 12_000,
        gain: 1.0,
        track: 0,
        audio: Arc::clone(&audio),
        shape: ClipShape {
            period: Some(3_000),
            ..Default::default()
        },
    };
    let mut left = whole.clone();
    left.length = 1_000;
    let mut right = whole.clone();
    right.id = 2;
    right.start = split_ticks;
    right.length = 11_000;
    right.shape.phase = 1_000;
    for block in [1, 64, 128, 333, 4_096] {
        let (mut handle, _, mut original) = setup(vec![whole.clone()]);
        handle
            .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
            .unwrap();
        handle.send(Command::Play).unwrap();
        let expected = run(&mut original, 16_000, block);
        let (mut handle, _, mut split) = setup(vec![left.clone(), right.clone()]);
        handle
            .set_timeline(Timeline::constant(120.0, Meter::FOUR_FOUR))
            .unwrap();
        handle.send(Command::Play).unwrap();
        let actual = run(&mut split, 16_000, block);
        assert!(
            actual == expected,
            "loop phase, period {period_ticks:?}, block {block}, first mismatch {:?}",
            actual
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .map(|i| (i, actual[i], expected[i]))
        );
    }
}

#[test]
fn a_clip_with_no_channels_plays_silence_and_does_not_stop_the_others() {
    let audio = ramp(1_000, RATE);
    let make = |id, track, audio: Arc<ClipAudio>| Clip {
        envelope: Default::default(),
        id,
        start: Ticks::ZERO,
        offset: 0,
        length: 1_000,
        gain: 1.0,
        track,
        audio,
        shape: Default::default(),
    };
    let empty = Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![],
    });
    let (mut handle, _clips, mut engine) =
        setup(vec![make(1, 0, empty), make(2, 0, Arc::clone(&audio))]);
    handle.send(Command::Play).unwrap();
    // Past the 2 ms the engine reads ahead (the pre-roll), the second clip
    // plays and the first adds nothing.
    let out = run(&mut engine, 100_000, 128);
    assert!(out.iter().any(|&(l, _)| l != 0.0), "the other clip plays");
}
