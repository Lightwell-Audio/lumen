//! The offline clip renderer (M7b slice E3): a render sounds like playback,
//! its outer edges are bare so a baked clip is not faded twice, and it can be
//! cancelled.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lumen_core::curve::Curve;
use lumen_engine::{
    Clip, ClipAudio, ClipRender, ClipShape, Command, Destination, FadeFrames, Finished, NodeSpec,
    Routing, Timeline, clip_player, engine, mixer, render_clips,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const TRACK: u64 = 1;

fn timeline() -> Timeline {
    Timeline::default()
}

/// Audio whose every sample differs.
fn ramp(frames: usize, mono: bool) -> Arc<ClipAudio> {
    let left: Vec<f32> = (0..frames)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect();
    let right = left.iter().map(|v| -v * 0.5).collect();
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: if mono { vec![left] } else { vec![left, right] },
    })
}

/// A clip at timeline sample `start`, playing `length` frames of `audio`.
fn clip(id: u64, start: i64, offset: i64, length: i64, audio: &Arc<ClipAudio>) -> Clip {
    Clip {
        id,
        track: TRACK,
        start: timeline().ticks_at(start, RATE),
        offset,
        length,
        gain: 1.0,
        audio: Arc::clone(audio),
        shape: ClipShape::default(),
        envelope: Default::default(),
    }
}

fn job(clips: Vec<Clip>, from: i64, frames: u64, bare_ends: bool, block: usize) -> ClipRender {
    ClipRender {
        clips,
        timeline: timeline(),
        sample_rate: RATE,
        from,
        frames,
        bare_ends,
        block,
    }
}

/// Renders to two vectors, returning how it ended too.
fn run(job: ClipRender) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let end = render_clips(job, &AtomicBool::new(false), &mut |_| {}, &mut |l, r| {
        left.extend_from_slice(l);
        right.extend_from_slice(r);
    });
    assert_eq!(end, Finished::Done);
    (left, right)
}

/// What the live engine plays for the same clips: the clip player under a
/// mixer, a track straight to the master, in blocks of `block`.
fn live(clips: Vec<Clip>, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    let routing = Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec::unity(TRACK, Destination::Node(MASTER)),
        ],
        ..Default::default()
    };
    assert!(routing_handle.set(&routing).unwrap());
    let (mut transport, mut engine) = engine(timeline(), source, RATE);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * block];
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE, &mut data[..2 * n], 2, n);
        left.extend_from_slice(&data[..n]);
        right.extend_from_slice(&data[n..2 * n]);
        done += n;
    }
    (left, right)
}

fn same(a: &[f32], b: &[f32], tolerance: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

fn fade(frames: i64) -> FadeFrames {
    FadeFrames {
        frames,
        span: frames,
        curve: Curve::S_CURVE,
        ..Default::default()
    }
}

#[test]
fn a_render_sounds_like_playback_with_fades_gain_and_a_gap() {
    // Past the engine's play fade-in and the mixer's first glide.
    let start = 12_000;
    let audio = ramp(60_000, false);
    let mut a = clip(1, start, 0, 20_000, &audio);
    a.gain = 0.5;
    a.shape.fade_in = fade(4_000);
    a.shape.fade_out = fade(2_500);
    let b = clip(2, start + 25_000, 20_000, 30_000, &audio);
    let clips = vec![a, b];
    let end = (start + 55_000) as usize;
    let (live_left, live_right) = live(clips.clone(), end, 128);
    let (left, right) = run(job(clips, 0, end as u64, false, 256));
    // The engine fades Play in over a few milliseconds before the first clip.
    assert!(same(
        &left[start as usize..],
        &live_left[start as usize..],
        1e-5
    ));
    assert!(same(
        &right[start as usize..],
        &live_right[start as usize..],
        1e-5
    ));
    assert!(left.iter().any(|v| v.abs() > 0.1), "something was rendered");
}

#[test]
fn the_block_size_does_not_change_a_render() {
    let audio = ramp(30_000, false);
    let mut a = clip(1, 500, 0, 9_000, &audio);
    a.shape.fade_in = fade(3_000);
    let b = clip(2, 12_000, 9_000, 15_000, &audio);
    let clips = vec![a, b];
    let whole = run(job(clips.clone(), 0, 30_000, false, 256));
    for block in [1, 7, 100, 255] {
        let split = run(job(clips.clone(), 0, 30_000, false, block));
        assert_eq!(split, whole, "blocks of {block}");
    }
}

#[test]
fn bare_ends_play_a_plain_clip_exactly_and_the_inside_edges_keep_their_fade() {
    let audio = ramp(30_000, false);
    // Two clips from one source with a gap between them.
    let a = clip(1, 1_000, 1_000, 8_000, &audio);
    let b = clip(2, 12_000, 12_000, 8_000, &audio);
    let clips = vec![a, b];
    let (left, right) = run(job(clips.clone(), 1_000, 19_000, true, 256));
    // The outer start and end are the source exactly.
    assert_eq!(left[0], audio.memory().unwrap()[0][1_000]);
    assert_eq!(right[0], audio.memory().unwrap()[1][1_000]);
    let last = 19_000 - 1;
    assert_eq!(left[last], audio.memory().unwrap()[0][1_000 + last]);
    // Only the inside end of the first clip is faded: its last 2 ms.
    let edge = (RATE / 500) as usize;
    assert_eq!(
        left[..8_000 - edge],
        audio.memory().unwrap()[0][1_000..9_000 - edge]
    );
    // The gap is silent, to the sample.
    assert!(left[8_000..11_000].iter().all(|v| *v == 0.0));
    assert!(right[8_000..11_000].iter().all(|v| *v == 0.0));
    // The inside edges are faded like playback's, so there is no click: the
    // first frame after the gap is below the source, the last before it too.
    let inside_start = 11_000;
    assert!(left[inside_start].abs() < audio.memory().unwrap()[0][12_000].abs());
    assert!(left[7_999].abs() < audio.memory().unwrap()[0][8_999].abs());
    // Without bare ends the outer start is faded too.
    let (faded, _) = run(job(clips, 1_000, 19_000, false, 256));
    assert!(faded[0].abs() < audio.memory().unwrap()[0][1_000].abs());
}

#[test]
fn a_baked_clip_played_back_equals_the_clips_it_came_from() {
    // The consolidation null test: render a stretch with bare ends, play the
    // result as one clip, and it matches the original clips played.
    let start = 12_000;
    let source = ramp(60_000, false);
    let mut a = clip(1, start, 0, 20_000, &source);
    a.gain = 0.7;
    a.shape.fade_in = fade(4_000);
    let b = clip(2, start + 25_000, 20_000, 30_000, &source);
    let clips = vec![a, b];
    let span = 55_000_u64;
    let (left, right) = run(job(clips.clone(), start, span, true, 256));
    let baked = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![left, right],
    });
    let new = clip(3, start, 0, span as i64, &baked);
    let total = (start as u64 + span) as usize;
    let original = run(job(clips, 0, total as u64, false, 256));
    let consolidated = run(job(vec![new], 0, total as u64, false, 256));
    assert!(same(&original.0, &consolidated.0, 1e-6));
    assert!(same(&original.1, &consolidated.1, 1e-6));
}

#[test]
fn consolidating_plain_adjacent_source_material_is_bit_identical() {
    // One long clip, no processing: the bake is the source and playing it
    // gives the same samples as the original clip does.
    let source = ramp(40_000, false);
    let a = clip(1, 5_000, 0, 30_000, &source);
    let (left, right) = run(job(vec![a.clone()], 5_000, 30_000, true, 256));
    assert_eq!(left, source.memory().unwrap()[0][..30_000]);
    assert_eq!(right, source.memory().unwrap()[1][..30_000]);
    let baked = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![left, right],
    });
    let new = clip(2, 5_000, 0, 30_000, &baked);
    assert_eq!(
        run(job(vec![new], 0, 36_000, false, 128)),
        run(job(vec![a], 0, 36_000, false, 128)),
    );
}

#[test]
fn a_mono_clip_is_on_both_sides() {
    let source = ramp(8_000, true);
    let a = clip(1, 0, 0, 8_000, &source);
    let (left, right) = run(job(vec![a], 0, 8_000, true, 256));
    assert_eq!(left, right);
    assert_eq!(left, source.memory().unwrap()[0]);
}

#[test]
fn a_render_can_be_cancelled_and_reports_progress() {
    let source = ramp(48_000, false);
    let a = clip(1, 0, 0, 48_000, &source);
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let mut frames = 0;
    let end = render_clips(
        job(vec![a], 0, 48_000, false, 256),
        &cancel,
        &mut |p| {
            seen.push(p);
            if p > 0.5 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &mut |l, _| frames += l.len(),
    );
    assert_eq!(end, Finished::Cancelled);
    assert!(frames < 48_000 && frames > 24_000);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn rendering_nothing_finishes_at_once() {
    let end = render_clips(
        job(Vec::new(), 0, 0, true, 256),
        &AtomicBool::new(false),
        &mut |_| panic!("no blocks"),
        &mut |_, _| panic!("no blocks"),
    );
    assert_eq!(end, Finished::Done);
    let (left, right) = run(job(Vec::new(), 0, 1_000, true, 256));
    assert!(left.iter().chain(&right).all(|v| *v == 0.0));
}
