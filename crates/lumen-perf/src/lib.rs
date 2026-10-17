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
use lumen_engine::testing::{latent, routed};
use lumen_engine::{
    ClickSound, Clip, ClipAudio, ClipPlayer, Command, Destination, Engine, MetronomeSettings,
    Mixer, MixerHandle, NodeSpec, Region, Routing, Timeline, clip_player, engine, mixer,
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
        "engine_routed_64",
        "1,000 blocks of 128 frames: 64 tracks in 8 groups, each with two sends",
    ),
    (
        "engine_latent_64",
        "1,000 blocks of 128 frames: the routed mix with delay compensation and a sidechain",
    ),
    (
        "engine_latent_swaps_64",
        "1,000 blocks of 128 frames: the compensated mix with new levels every block (a fader drag)",
    ),
    (
        "engine_synth_64",
        "1,000 blocks of 128 frames: 64 MIDI tracks each playing a four-note chord on the synth, through the routed mix",
    ),
    (
        "engine_hosted_64",
        "1,000 blocks of 128 frames: 64 MIDI tracks, each a hosted instrument and a hosted effect (fake hosts that answer inside submit), through the routed mix",
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
            let mut engine = playing_clips(64, Mix::Direct);
            measured_engine_clips_64(&mut engine);
        }
        "engine_routed_64" => {
            let mut engine = playing_clips(64, Mix::Routed);
            measured_engine_routed_64(&mut engine);
        }
        "engine_latent_64" => {
            let mut engine = playing_clips(64, Mix::Latent);
            measured_engine_latent_64(&mut engine);
        }
        "engine_latent_swaps_64" => {
            let (mut routing, mut engine) = playing_clips_routed(64, Mix::Latent);
            // Two plans with the same shape and different levels
            // (`latent`'s variants 0 and 6), compiled outside the measure.
            let plans = [latent(64, 0), latent(64, 6)];
            for block in 0..1_000 {
                assert!(routing.set(&plans[block % 2]).expect("a valid routing"));
                measured_engine_latent_swaps_64(&mut engine);
                routing.collect_garbage();
            }
        }
        "engine_synth_64" => {
            let mut engine = playing_synths(64);
            measured_engine_synth_64(&mut engine);
        }
        "engine_hosted_64" => {
            let (_routing, mut engine) = playing_hosted(64);
            measured_engine_hosted_64(&mut engine);
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

type Player = Engine<Mixer<ClipPlayer>>;
/// The engine with the MIDI instruments beside the audio clips.
type SynthPlayer = Engine<Mixer<lumen_engine::Layered<ClipPlayer, lumen_engine::MidiPlayer>>>;

/// How the clips of a scenario are mixed.
#[derive(Clone, Copy, PartialEq)]
enum Mix {
    /// All on one track, straight to the device.
    Direct,
    /// Each on its own track in [`routed`].
    Routed,
    /// Each on its own track in [`latent`].
    Latent,
}

/// `clips` clips playing, mixed as `mix`.
fn playing_clips(clips: usize, mix: Mix) -> Player {
    playing_clips_routed(clips, mix).1
}

/// As [`playing_clips`], with the handle that sends it new routings.
fn playing_clips_routed(clips: usize, mix: Mix) -> (MixerHandle, Player) {
    let routed_mix = mix != Mix::Direct;
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
                    track: if routed_mix { i as u64 } else { 0 },
                    audio: Arc::clone(&audio),
                    shape: Default::default(),
                })
                .collect(),
        )
        .expect("room for one clip list");
    let (mut routing, source) = mixer(player);
    let plan = match mix {
        Mix::Routed => routed(clips as u64, 0),
        Mix::Latent => latent(clips as u64, 0),
        Mix::Direct => Routing {
            nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
            cue: None,
            measure: None,
        },
    };
    assert!(routing.set(&plan).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in, into steady playback.
    for _ in 0..10 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    (routing, engine)
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

/// One block that takes a new plan, as during a fader drag. Called once
/// per block: Callgrind adds up every call.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn measured_engine_latent_swaps_64(engine: &mut Player) {
    let mut data = [0.0; 2 * FRAMES];
    engine.process(RATE, &mut data, 2, FRAMES);
    black_box((&data, engine.transport().playing));
}

/// `tracks` MIDI tracks, each a four-note chord on the synth that repeats every
/// beat and rings on past it (so voices overlap while they release), mixed as
/// in [`routed`], playing.
fn playing_synths(tracks: u64) -> SynthPlayer {
    use lumen_devices::{Preset, Synth, synth};
    use lumen_engine::{InstrumentSpec, Layered, MidiClip, MidiNote, MidiTrack, Target};
    let (_clips, players) = clip_player();
    let (mut midi, player) = lumen_engine::midi_player();
    let q = Ticks::quarters;
    let list = (0..tracks)
        .map(|id| {
            let notes: Vec<MidiNote> = [0_u8, 4, 7, 11]
                .iter()
                .map(|&interval| MidiNote {
                    start: Ticks(id as i64 * 17),
                    length: Ticks(q(1).0 * 3 / 4),
                    pitch: 48 + (id % 12) as u8 + interval,
                    velocity: 70 + (id % 40) as u8,
                })
                .collect();
            MidiTrack::new(
                id,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: synth::KIND,
                    preset: Preset::Poly.number(),
                    instrument: Box::new(Synth::new(Preset::Poly)),
                }),
                vec![MidiClip {
                    id,
                    start: Ticks::ZERO,
                    length: q(64),
                    period: Some(q(1)),
                    seams: Default::default(),
                    notes: lumen_engine::note_list(notes),
                    controls: lumen_engine::control_list(Vec::new()),
                }],
            )
        })
        .collect();
    midi.set(list).ok().expect("room for one track list");
    let (mut routing, source) = mixer(Layered(players, player));
    assert!(routing.set(&routed(tracks, 0)).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
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
pub fn measured_engine_synth_64(engine: &mut SynthPlayer) {
    let mut data = vec![0.0; 2 * FRAMES];
    for _ in 0..1_000 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // Distinct from the other scenarios, so the linker keeps it apart.
    black_box((&data, engine.transport().marker));
}

/// A host that is not one: it answers inside `submit`, with no thread (the
/// scenarios may not use any). An instrument rings a level that each note-on
/// resets and each block lets fall; an effect halves its input.
struct Echo {
    effect: bool,
    level: f32,
    out: [[f32; FRAMES]; 2],
    frames: usize,
}

impl Echo {
    fn boxed(effect: bool) -> Box<Self> {
        Box::new(Self {
            effect,
            level: 0.0,
            out: [[0.0; FRAMES]; 2],
            frames: 0,
        })
    }
}

impl lumen_devices::Hosted for Echo {
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        events: &[lumen_core::midi::Event],
    ) -> Result<(), lumen_devices::Refused> {
        self.frames = frames;
        if self.effect {
            for (out, from) in self.out.iter_mut().zip(input) {
                for (to, sample) in out[..frames].iter_mut().zip(*from) {
                    *to = sample * 0.5;
                }
            }
        } else {
            if !events.is_empty() {
                self.level = 1.0;
            }
            for out in &mut self.out {
                out[..frames].fill(self.level * 0.1);
            }
            self.level *= 0.99;
        }
        Ok(())
    }

    fn collect(
        &mut self,
        _: std::time::Instant,
        left: &mut [f32],
        right: &mut [f32],
    ) -> lumen_devices::Reply {
        left.copy_from_slice(&self.out[0][..self.frames]);
        right.copy_from_slice(&self.out[1][..self.frames]);
        lumen_devices::Reply::Ready {
            latency: 0,
            tail: 0,
            restart: false,
            work_ns: 0,
        }
    }

    fn ended(&self) -> bool {
        false
    }
}

/// [`playing_synths`], with every instrument and a hosted effect on every
/// track in plugin hosts (fake ones): the engine's own cost of hosting.
fn playing_hosted(tracks: u64) -> (MixerHandle, SynthPlayer) {
    use lumen_devices::{Guard, HostedInstrument};
    use lumen_engine::{Device, InstrumentSpec, Layered, MidiClip, MidiNote, MidiTrack, Target};
    let (_clips, players) = clip_player();
    let (mut midi, player) = lumen_engine::midi_player();
    let q = Ticks::quarters;
    let list = (0..tracks)
        .map(|id| {
            let notes: Vec<MidiNote> = [0_u8, 4, 7, 11]
                .iter()
                .map(|&interval| MidiNote {
                    start: Ticks(id as i64 * 17),
                    length: Ticks(q(1).0 * 3 / 4),
                    pitch: 48 + (id % 12) as u8 + interval,
                    velocity: 70 + (id % 40) as u8,
                })
                .collect();
            let guard = Guard::new(Echo::boxed(false), 0);
            MidiTrack::new(
                id,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: 77,
                    preset: 0,
                    instrument: Box::new(HostedInstrument::new(guard)),
                }),
                vec![MidiClip {
                    id,
                    start: Ticks::ZERO,
                    length: q(64),
                    period: Some(q(1)),
                    seams: Default::default(),
                    notes: lumen_engine::note_list(notes),
                    controls: lumen_engine::control_list(Vec::new()),
                }],
            )
        })
        .collect();
    midi.set(list).ok().expect("room for one track list");
    let (mut routing, source) = mixer(Layered(players, player));
    let mut plan = routed(tracks, 0);
    for node in plan.nodes.iter_mut().filter(|n| n.id < tracks) {
        node.devices = vec![Device::Hosted {
            host: 1_000 + node.id,
            latency: 0,
        }];
    }
    let mut statuses = Vec::new();
    for id in 0..tracks {
        let guard = Guard::new(Echo::boxed(true), 0);
        statuses.push(guard.status());
        assert!(routing.attach_host(1_000 + id, guard).is_ok());
    }
    assert!(routing.set(&plan).expect("a valid routing"));
    let (mut transport, mut engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).expect("room for a command");
    let mut data = vec![0.0; 2 * FRAMES];
    // Past the fade-in and the glide from each fallback to its host, into
    // steady playback.
    for _ in 0..400 {
        engine.process(RATE, &mut data, 2, FRAMES);
    }
    // The scenario must really use its hosts: none failed or missed a block.
    assert!(statuses.iter().all(|s| !s.failed() && s.late_blocks() == 0));
    (routing, engine)
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

fn transport_only() -> (lumen_engine::EngineHandle, Player) {
    let (_clips, player) = clip_player();
    let (_routing, source) = mixer(player);
    let (mut handle, engine) = engine(Timeline::default(), source, RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: ClickSound::Wood,
            channel: 0,
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
pub fn measured_engine_transport(mut handle: lumen_engine::EngineHandle, engine: &mut Player) {
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
        for name in [
            "engine_clips_64",
            "engine_routed_64",
            "engine_latent_64",
            "engine_latent_swaps_64",
            "engine_synth_64",
            "engine_hosted_64",
            "engine_transport",
            "tempo_conversions",
        ] {
            assert!(run(name), "{name}");
        }
        assert!(!run("no_such_scenario"));
        assert!(SCENARIOS.iter().all(|(name, _)| !name.contains(' ')));
    }
}
