//! Golden renders of audio clips: every output sample is compared with the
//! value it must have, at several block sizes (M2 slice 6).

use std::sync::Arc;

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Engine, EngineHandle, Region, Timeline,
    clip_player, engine,
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

fn setup(clips: Vec<Clip>) -> (EngineHandle, ClipsHandle, Engine<ClipPlayer>) {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (handle, engine) = engine(Timeline::default(), player, RATE);
    (handle, clips_handle, engine)
}

/// Renders `frames` in blocks of `block`; returns (left, right) per frame.
fn run(engine: &mut Engine<ClipPlayer>, frames: usize, block: usize) -> Vec<(f32, f32)> {
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
        id: 1,
        start: bars(1), // sample 96,000 at 120 BPM
        offset,
        length,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
fn the_loop_wrap_crossfades_and_stays_on_the_exact_sample() {
    let audio = ramp(400_000, RATE);
    let clip = Clip {
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 400_000,
        gain: 0.5,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 10_000,
        gain: 0.5,
        pan: 0.0,
        audio: mono,
    };
    let (mut handle, _clips, mut engine) = setup(vec![clip]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 2_000, 256);
    assert_eq!(out[1_000], (0.25, 0.25));
}

#[test]
fn clips_at_another_sample_rate_stay_silent() {
    let clip = Clip {
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 10_000,
        gain: 1.0,
        pan: 0.0,
        audio: ramp(10_000, 44_100),
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
        id: 1,
        start: bars(1),
        offset: 0,
        length: 1_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start,
        offset: 0,
        length: 1_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 20_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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
        id: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: 40_000,
        gain: 1.0,
        pan: 0.0,
        audio: Arc::clone(&audio),
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

fn steady_clip(audio: &Arc<ClipAudio>, gain: f32, pan: f32) -> Clip {
    Clip {
        id: 7,
        start: Ticks::ZERO,
        offset: 0,
        length: audio.frames() as i64,
        gain,
        pan,
        audio: Arc::clone(audio),
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
fn volume_pan_and_mute_changes_glide_instead_of_stepping() {
    // Owner: dragging volume or pan crackled during playback, and muting
    // clicked: each new clip list changed the level at once.
    let audio = steady(48_000);
    let (mut handle, mut clips, mut engine) = setup(vec![steady_clip(&audio, 1.0, 0.0)]);
    handle.send(Command::Play).unwrap();
    let mut out = run(&mut engine, 1_000, 128);
    // A drag: a new level every 16 ms (one screen frame).
    // The app frees replaced lists every frame; so does this test.
    let mut send = |gain, pan| {
        clips.collect_garbage();
        clips.set(vec![steady_clip(&audio, gain, pan)]).unwrap();
    };
    for (gain, pan) in [(0.8, 0.0), (0.6, -0.3), (0.6, -0.9), (1.0, 0.5)] {
        send(gain, pan);
        out.extend(run(&mut engine, 768, 128));
    }
    // Mute (gain 0), then unmute.
    send(0.0, 0.0);
    out.extend(run(&mut engine, 2_000, 128));
    assert_eq!(out.last(), Some(&(0.0, 0.0)), "muted reaches silence");
    send(1.0, 0.0);
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
    clips.set(vec![steady_clip(&audio, 1.0, 0.0)]).unwrap();
    out.extend(run(&mut engine, 2_000, 128));
    assert!(steepest(&out) <= MAX_STEP, "steepest {}", steepest(&out));
    assert_eq!(out.last(), Some(&(0.5, 0.5)));
}

#[test]
fn a_clip_heard_from_its_start_keeps_its_attack() {
    // A new clip that starts on its own first sample is not slowed by the
    // glide: only its 2 ms edge fade applies.
    let audio = steady(48_000);
    let mut late = steady_clip(&audio, 1.0, 0.0);
    late.start = Ticks::quarters(1); // sample 24,000
    let (mut handle, _clips, mut engine) = setup(vec![late]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 24_200, 128);
    assert_eq!(out[24_000 + FADE as usize], (0.5, 0.5));
}
