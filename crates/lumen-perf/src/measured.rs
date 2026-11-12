//! The measured functions: everything Callgrind counts, one per scenario.
//! Each one is kept out of the line (`no_mangle`, `inline(never)`) so
//! `--toggle-collect=measured_<scenario>` finds it.

use std::hint::black_box;

use lumen_core::time::TempoMap;
use lumen_engine::EngineHandle;
use lumen_engine::stream_plan::{Horizon, Position, Scene};
use lumen_project::model::Project;
use lumen_project::{from_bytes, to_bytes};

use super::scenarios::{FRAMES, Player, RATE, SynthPlayer};

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_crossfades_64(engine: &mut Player) {
    let mut data = [0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box(&data);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_clips_64(engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box(&data);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_routed_64(engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Not the same code as `measured_engine_clips_64`, or the linker folds
    // the two into one symbol and Callgrind finds no routed scenario.
    black_box((&data, engine.transport().sample));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_latent_64(engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().ticks));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_automated_64(engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().punching));
}

/// One block that takes a new plan, as during a fader drag. Called once
/// per block: Callgrind adds up every call.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_latent_swaps_64(engine: &mut Player) {
    let mut data = [0.0; 2 * FRAMES];
    engine.process(RATE, &mut data, 2, FRAMES);
    black_box((&data, engine.transport().playing));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_synth_64(engine: &mut SynthPlayer) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().marker));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_plugin_lanes_64(engine: &mut SynthPlayer) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box((
        &data,
        engine.transport().marker,
        engine.transport().ticks,
        1u8,
    ));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_hosted_64(engine: &mut SynthPlayer) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().marker, engine.transport().ticks));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_transport(mut handle: EngineHandle, engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for block in 0..2_000 {
        if block % 500 == 250 {
            handle
                .send(lumen_engine::Command::Locate(
                    lumen_core::time::Ticks::quarters(4),
                ))
                .expect("room for a command");
        }
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box(&data);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_streamed_64(engine: &mut Player) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().playing));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_stream_plan_64(scene: &Scene) {
    let mut out = Vec::new();
    let mut wanted = 0;
    // A plan every 100 ms of playback, half of them inside the loop.
    for step in 0..50i64 {
        let at = Position {
            sample: 4_800 * step + if step % 2 == 0 { 0 } else { 8 * 48_000 },
            playing: true,
            cue: (step % 10 == 0).then_some(30 * 48_000),
            marker: Some(0),
        };
        scene.plan(at, Horizon::at(RATE), &mut out);
        wanted += out.len();
    }
    black_box(wanted);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_tempo_conversions(map: &TempoMap) {
    let mut sum = 0i64;
    for i in 0..50_000i64 {
        let t = lumen_core::time::Ticks::quarters(i % 512);
        let sample = map.sample_at(t, RATE);
        sum = sum.wrapping_add(sample);
        sum = sum.wrapping_add(map.ticks_at_sample(sample + i, RATE).0);
    }
    black_box(sum);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_project_serialize(project: &Project) {
    black_box(to_bytes(project).expect("valid project"));
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_project_parse(bytes: &[u8]) {
    black_box(from_bytes(bytes).expect("valid project"));
}
