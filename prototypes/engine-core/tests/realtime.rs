//! P-ENGINE pass criteria: no heap traffic on the audio side, bit-identical
//! output for any block size, inaudible swaps of an unchanged graph.

use engine_core::{Command, Graph, engine, test_source, test_tracks};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const SAMPLE_RATE: f32 = 48_000.0;

fn session(tracks: u32) -> Graph {
    Graph::new(
        SAMPLE_RATE,
        test_source(48_000 * 4),
        &test_tracks(tracks, 4_801, 40),
    )
}

/// Renders `frames` samples in blocks of `block` and returns interleaved output.
fn render(graph: Graph, frames: usize, block: usize) -> Vec<f32> {
    let (_handle, mut engine) = engine(graph);
    let mut out = Vec::with_capacity(frames * 2);
    let (mut left, mut right) = (vec![0.0; block], vec![0.0; block]);
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(&mut left[..n], &mut right[..n]);
        for i in 0..n {
            out.push(left[i]);
            out.push(right[i]);
        }
        done += n;
    }
    out
}

#[test]
fn process_never_touches_the_heap() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let (mut handle, mut engine) = engine(session(64));
    let (mut left, mut right) = (vec![0.0; 128], vec![0.0; 128]);

    for block in 0..2_000 {
        // UI-side work happens outside the real-time section, as in the app.
        if block % 100 == 50 {
            handle.replace_graph(session(64)).expect("graph queue full");
        }
        if block % 10 == 0 {
            handle
                .send(Command::SetTrackGain {
                    track_id: block % 64,
                    gain: 0.5,
                })
                .unwrap();
        }
        if block == 1_500 {
            handle.send(Command::Seek { position: 12_345 }).unwrap();
        }
        let ((), heap_ops) = count_rt_violations(|| engine.process(&mut left, &mut right));
        assert_eq!(heap_ops, 0, "heap operation in process() at block {block}");
        handle.collect_garbage();
    }
}

#[test]
fn output_is_identical_for_every_block_size() {
    let frames = 48_000 * 3;
    let reference = render(session(16), frames, 64);
    assert!(
        reference.iter().any(|&s| s != 0.0),
        "reference render is silent"
    );
    for block in [1, 3, 128, 333, 512, 4096, 10_000] {
        let output = render(session(16), frames, block);
        let first_difference = reference
            .iter()
            .zip(&output)
            .position(|(a, b)| a.to_bits() != b.to_bits());
        assert_eq!(
            first_difference, None,
            "block size {block} differs from block size 64"
        );
    }
}

#[test]
fn swapping_in_an_identical_graph_is_inaudible() {
    let frames = 48_000;
    let reference = render(session(8), frames, 128);

    let (mut handle, mut engine) = engine(session(8));
    let (mut left, mut right) = (vec![0.0; 128], vec![0.0; 128]);
    let mut output = Vec::with_capacity(frames * 2);
    for block in 0..frames / 128 {
        if block % 37 == 5 {
            handle.replace_graph(session(8)).unwrap();
        }
        engine.process(&mut left, &mut right);
        for i in 0..128 {
            output.push(left[i]);
            output.push(right[i]);
        }
        handle.collect_garbage();
    }
    assert_eq!(output.len(), reference.len());
    assert!(
        output
            .iter()
            .zip(&reference)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn retired_graphs_are_freed_off_the_audio_thread() {
    let (mut handle, mut engine) = engine(session(4));
    let (mut left, mut right) = (vec![0.0; 64], vec![0.0; 64]);
    handle.replace_graph(session(4)).unwrap();
    handle.replace_graph(session(4)).unwrap();
    engine.process(&mut left, &mut right);
    engine.process(&mut left, &mut right);
    assert_eq!(handle.collect_garbage(), 2);
}

#[test]
fn live_gain_command_reaches_the_track() {
    let (mut handle, mut engine) = engine(Graph::new(
        SAMPLE_RATE,
        test_source(4_800),
        &test_tracks(1, u64::MAX / 4, 0),
    ));
    let (mut left, mut right) = (vec![0.0; 4_800], vec![0.0; 4_800]);
    engine.process(&mut left, &mut right);
    let loud: f32 = left.iter().map(|s| s * s).sum();
    handle
        .send(Command::SetTrackGain {
            track_id: 0,
            gain: 0.0,
        })
        .unwrap();
    engine.process(&mut left, &mut right); // smoother settles within this block
    engine.process(&mut left, &mut right);
    let quiet: f32 = left.iter().map(|s| s * s).sum();
    assert!(
        quiet < loud * 1e-6,
        "gain command had no effect: {quiet} vs {loud}"
    );
}
