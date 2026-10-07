//! Cost of playing audio clips: 64 stereo clips all sounding at once, the
//! M2 slice 6 target ("well inside the CPU budget" at 128 frames).

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipAudio, ClipPlayer, Command, Engine, Timeline, clip_player, engine};

fn main() {
    divan::main();
}

fn playing(clips: usize) -> Engine<ClipPlayer> {
    let audio = Arc::new(ClipAudio {
        sample_rate: 48_000,
        channels: vec![vec![0.1; 480_000], vec![-0.1; 480_000]],
    });
    let (mut handle, player) = clip_player();
    handle
        .set(
            (0..clips)
                .map(|_| Clip {
                    start: Ticks::ZERO,
                    offset: 0,
                    length: 480_000,
                    gain: 0.5,
                    pan: 0.0,
                    audio: Arc::clone(&audio),
                })
                .collect(),
        )
        .unwrap();
    let (mut transport, mut engine) = engine(Timeline::default(), player, 48_000);
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
    let mut engine = playing(clips);
    let mut data = vec![0.0; 256];
    bencher.bench_local(|| engine.process(48_000, divan::black_box(&mut data), 2, 128));
}
