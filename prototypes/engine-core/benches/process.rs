//! Engine overhead per block. Budget at 48 kHz: 64 frames = 1333 µs,
//! 128 frames = 2667 µs. Quality bar: whole project under 50% of budget.

use engine_core::{Engine, Graph, engine, test_source, test_tracks};

fn main() {
    divan::main();
}

fn engine_with(tracks: u32) -> Engine {
    let graph = Graph::new(
        48_000.0,
        test_source(48_000 * 10),
        &test_tracks(tracks, 4_801, 1_000),
    );
    engine(graph).1
}

#[divan::bench(args = [(64, 64), (64, 128), (256, 128)])]
fn process_block(bencher: divan::Bencher<'_, '_>, (tracks, frames): (u32, usize)) {
    let mut engine = engine_with(tracks);
    let (mut left, mut right) = (vec![0.0_f32; frames], vec![0.0_f32; frames]);
    bencher
        .bench_local(|| engine.process(divan::black_box(&mut left), divan::black_box(&mut right)));
}
