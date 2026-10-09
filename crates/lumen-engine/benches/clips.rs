//! Cost of playing audio clips: 64 stereo clips all sounding at once, the
//! M2 slice 6 target ("well inside the CPU budget" at 128 frames); and the
//! same through a production-sized routing (M4 slice 1): a track each, in
//! groups, with sends to two returns. The quality bar: under half of the
//! 2.67 ms a 128-frame block lasts at 48 kHz.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::testing::routed;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, Command, Destination, Engine, Mixer, NodeSpec, Routing, Timeline,
    clip_player, engine, mixer,
};

fn main() {
    divan::main();
}

/// `clips` clips playing; through the routed mix, each on its own track,
/// or else all on one track straight to the device.
fn playing(clips: usize, routed_mix: bool) -> Engine<Mixer<ClipPlayer>> {
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 480_000], vec![-0.1; 480_000]],
    });
    let (mut handle, player) = clip_player();
    handle
        .set(
            (0..clips)
                .map(|i| Clip {
                    id: i as u64,
                    start: Ticks::ZERO,
                    offset: 0,
                    length: 480_000,
                    gain: 0.5,
                    track: if routed_mix { i as u64 } else { 0 },
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .unwrap();
    let (mut routing, source) = mixer(player);
    let plan = if routed_mix {
        routed(clips as u64, 0)
    } else {
        Routing {
            nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
        }
    };
    assert!(routing.set(&plan).unwrap());
    let (mut transport, mut engine) = engine(Timeline::default(), source, 48_000);
    transport.send(Command::Play).unwrap();
    let mut data = vec![0.0; 256];
    // Past the fade-in, into steady playback.
    for _ in 0..10 {
        engine.process(48_000, &mut data, 2, 128);
    }
    engine
}

#[divan::bench(args = [1, 16, 64])]
fn stereo_clips_at_128_frames(bencher: divan::Bencher<'_, '_>, clips: usize) {
    let mut engine = playing(clips, false);
    let mut data = vec![0.0; 256];
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}

#[divan::bench(args = [16, 64, 200])]
fn routed_tracks_at_128_frames(bencher: divan::Bencher<'_, '_>, tracks: usize) {
    let mut engine = playing(tracks, true);
    let mut data = vec![0.0; 256];
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}
