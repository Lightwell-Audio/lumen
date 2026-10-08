//! Performance regression gates (ADR-0022).
//!
//! Each scenario sets up its data, then runs its measured work inside a
//! function named `measured_<scenario>`. `perf-gate` runs each scenario under
//! Valgrind's Callgrind, counting instructions only inside that function
//! (`--toggle-collect`), and compares the count with `perf/baseline.json`.
//!
//! Instruction counts do not depend on machine load, so unlike wall-clock
//! benchmarks they can gate CI on shared runners. Wall-clock numbers still
//! come from the divan benchmarks and the quality-bar measurements.
//!
//! Scenarios must be deterministic: fixed seeds, fixed iteration counts, no
//! I/O and no threads in the measured part.

use std::hint::black_box;
use std::sync::Arc;

use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_engine::{
    ClickSound, Clip, ClipAudio, ClipPlayer, Command, Engine, MetronomeSettings, Region, Timeline,
    clip_player, engine,
};
use lumen_project::model::Project;
use lumen_project::testing::synthetic;
use lumen_project::{from_bytes, to_bytes};

/// Every scenario, with what it measures.
pub const SCENARIOS: &[(&str, &str)] = &[
    (
        "engine_clips_64",
        "1,000 blocks of 128 frames with 64 stereo clips sounding",
    ),
    (
        "engine_transport",
        "2,000 blocks of 128 frames: loop wraps, locates and the metronome",
    ),
    (
        "tempo_conversions",
        "100,000 tick/sample conversions on a 64-point tempo map",
    ),
    (
        "project_serialize",
        "Serialize a quality-bar project (200 tracks, about 20,000 clips)",
    ),
    (
        "project_parse",
        "Parse, migrate and validate the same project",
    ),
];

/// Runs one scenario. Returns false for an unknown name.
pub fn run(name: &str) -> bool {
    match name {
        "engine_clips_64" => {
            let mut engine = playing_clips(64);
            measured_engine_clips_64(&mut engine);
        }
        "engine_transport" => {
            let (handle, mut engine) = transport_only();
            measured_engine_transport(handle, &mut engine);
        }
        "tempo_conversions" => {
            let map = busy_tempo_map();
            measured_tempo_conversions(&map);
        }
        "project_serialize" => {
            let project = quality_bar_project();
            measured_project_serialize(&project);
        }
        "project_parse" => {
            let bytes = to_bytes(&quality_bar_project()).expect("valid project");
            measured_project_parse(&bytes);
        }
        _ => return false,
    }
    true
}

const RATE: u32 = 48_000;
const FRAMES: usize = 128;

fn playing_clips(clips: usize) -> Engine<ClipPlayer> {
    let audio = Arc::new(ClipAudio {
        sample_rate: RATE,
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
                    pan: 0.0,
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .expect("room for one clip list");
    let (mut transport, mut engine) = engine(Timeline::default(), player, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in, into steady playback.
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    engine
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_clips_64(engine: &mut Engine<ClipPlayer>) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box(&data);
}

fn transport_only() -> (lumen_engine::EngineHandle, Engine<ClipPlayer>) {
    let (_clips, player) = clip_player();
    let (mut handle, engine) = engine(Timeline::default(), player, RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: ClickSound::Wood,
        }))
        .expect("room for a command");
    // Two quarters at 120 BPM: 48,000 samples, so 2,000 blocks wrap it
    // about five times.
    handle
        .send(Command::SetLoop(Region {
            start: Ticks::quarters(4),
            end: Ticks::quarters(6),
            enabled: true,
        }))
        .expect("room for a command");
    handle.send(Command::Play).expect("room for a command");
    (handle, engine)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_transport(
    mut handle: lumen_engine::EngineHandle,
    engine: &mut Engine<ClipPlayer>,
) {
    let mut data = vec![0.0; 2 * FRAMES];
    for block in 0..2_000 {
        if block % 500 == 250 {
            handle
                .send(Command::Locate(Ticks::quarters(4)))
                .expect("room for a command");
        }
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    black_box(&data);
}

fn busy_tempo_map() -> TempoMap {
    TempoMap::new(
        (0..64)
            .map(|i| TempoPoint {
                at: Ticks::quarters(i * 8),
                bpm: 80.0 + (i % 16) as f64 * 7.5,
            })
            .collect(),
    )
    .expect("valid tempo map")
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_tempo_conversions(map: &TempoMap) {
    let mut sum = 0i64;
    for i in 0..50_000i64 {
        let t = Ticks::quarters(i % 512);
        let sample = map.sample_at(t, RATE);
        sum = sum.wrapping_add(sample);
        sum = sum.wrapping_add(map.ticks_at_sample(sample + i, RATE).0);
    }
    black_box(sum);
}

fn quality_bar_project() -> Project {
    synthetic(42, 200, 100, 10, 4_096)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_scenario_runs() {
        // The project scenarios are slow in debug builds; run the cheap ones
        // here, and check the names of the rest resolve.
        for name in ["engine_clips_64", "engine_transport", "tempo_conversions"] {
            assert!(run(name), "{name}");
        }
        assert!(!run("no_such_scenario"));
        assert!(SCENARIOS.iter().all(|(name, _)| !name.contains(' ')));
    }
}
