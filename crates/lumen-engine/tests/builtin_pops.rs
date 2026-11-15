//! The built-in effects' clicks in the engine's path (M8, OR-4): the events a
//! device never sees on its own — its power switched, it inserted into or taken
//! out of a playing chain, the routing re-planned, the transport played,
//! stopped or located, and a key connected — measured the way a person hears
//! them.
//!
//! The measurement is the same as `lumen-devices/tests/pops.rs`: the worst bend
//! `|x[n] - 2x[n-1] + x[n-2]|` over the frames around an event, as a fraction of
//! the wave around it, against the same measure of a clean tone. A click is a
//! bend the wave cannot account for.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_devices::Compressor;
use lumen_devices::dynamics::compressor;
use lumen_engine::{
    Clip, ClipAudio, ClipsHandle, Command, Destination, Device, Engine, EngineHandle, Mixer,
    MixerHandle, NodeSpec, Routing, SidechainSpec, Slot, Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const TRACK: u64 = 1;
const KEYED: u64 = 2;
const DEVICE: u64 = 7;

/// A tone at −12 dBFS, off the grid, long enough for any render here.
fn tone(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|k| 0.25 * (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin())
        .collect()
}

/// A clip of `samples` on `track`.
fn clip(track: u64, id: u64, samples: &[f32]) -> Clip {
    Clip {
        envelope: Default::default(),
        id,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: samples.len() as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![samples.to_vec(), samples.to_vec()],
        }),
        shape: Default::default(),
    }
}

/// The master, and a track carrying a built-in device; and, when `keyed`, a
/// second track that feeds the device's sidechain and is heard nowhere.
fn routing(bypass: bool, keyed: bool) -> Routing {
    let mut track = NodeSpec::unity(TRACK, Destination::Node(MASTER));
    track.devices = vec![Device::Builtin {
        device: DEVICE,
        latency: 0,
        bypass,
    }];
    track.values = vec![(
        DEVICE,
        compressor::TABLE
            .iter()
            .map(|p| (p.id, p.normalised(p.default)))
            .collect(),
    )];
    if keyed {
        track.sidechains = vec![SidechainSpec {
            from: KEYED,
            tap: Default::default(),
        }];
    }
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), track];
    if keyed {
        nodes.push(NodeSpec::unity(KEYED, Destination::None));
    }
    Routing {
        nodes,
        cue: None,
        measure: None,
    }
}

/// The same chain with no device on the track.
fn plain() -> Routing {
    Routing {
        nodes: vec![
            NodeSpec::unity(MASTER, Destination::Hardware(0)),
            NodeSpec::unity(TRACK, Destination::Node(MASTER)),
        ],
        cue: None,
        measure: None,
    }
}

struct Rig {
    _clips: ClipsHandle,
    transport: EngineHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<lumen_engine::ClipPlayer>>,
}

fn rig(clip_list: Vec<Clip>, routing: &Routing) -> Rig {
    let (mut clips, player) = clip_player();
    clips.set(clip_list).unwrap();
    let (mut handle, source) = mixer(player);
    assert!(handle.set(routing).unwrap());
    let (transport, engine) = engine(Timeline::default(), source, RATE);
    Rig {
        _clips: clips,
        transport,
        routing: handle,
        engine,
    }
}

impl Rig {
    fn attach(&mut self) {
        self.routing
            .attach_builtin(DEVICE, Slot::new(Box::new(Compressor::new())))
            .unwrap_or_else(|_| panic!("room in the pool"));
    }

    fn play(&mut self) {
        self.transport.send(Command::Play).unwrap();
    }

    /// Renders `frames` in blocks of at most `block`, calling `between` before
    /// each block with the block's index.
    fn run(
        &mut self,
        frames: usize,
        block: usize,
        out: &mut Vec<f32>,
        mut between: impl FnMut(&mut Rig, usize),
    ) {
        let mut data = vec![0.0; 2 * block];
        let mut done = 0;
        let mut index = 0;
        while done < frames {
            let n = block.min(frames - done);
            between(self, index);
            self.engine.process(RATE, &mut data[..2 * n], 2, n);
            out.extend_from_slice(&data[..n]);
            done += n;
            index += 1;
        }
    }
}

// ------------------------------------------------------------- the measures

const LOCAL: usize = 64;

/// A local level for each frame: the largest sample in its own block of
/// [`LOCAL`] and the blocks either side, so a bend is judged against the wave
/// around it rather than against a stretch of another level.
fn local_level(x: &[f32]) -> Vec<f32> {
    let blocks: Vec<f32> = x
        .chunks(LOCAL)
        .map(|b| b.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();
    let mut out = Vec::with_capacity(x.len());
    for k in 0..blocks.len() {
        let from = k.saturating_sub(1);
        let to = (k + 2).min(blocks.len());
        let most = blocks[from..to].iter().fold(0.0f32, |m, s| m.max(*s));
        out.extend(std::iter::repeat_n(most, LOCAL));
    }
    out.truncate(x.len());
    out
}

/// The worst bend over `from..to` as a fraction of the wave around it.
fn worst_bend_over_level(x: &[f32], from: usize, to: usize) -> f32 {
    let local = local_level(x);
    let floor = 0.01 * local.iter().fold(0.0f32, |m, s| m.max(*s)).max(1e-9);
    let to = to.min(x.len());
    (from.max(2)..to).fold(0.0f32, |m, k| {
        m.max((x[k] - 2.0 * x[k - 1] + x[k - 2]).abs() / local[k].max(floor))
    })
}

/// What a click at `at` measures against a clean tone over the same span.
fn click_db(out: &[f32], clean: &[f32], at: usize, span: usize) -> f32 {
    let from = at.saturating_sub(span);
    let to = at + span;
    let worse = worst_bend_over_level(out, from, to);
    let clean = worst_bend_over_level(clean, from.min(clean.len()), to.min(clean.len()));
    20.0 * (worse.max(1e-9) / clean.max(1e-9)).log10()
}

/// Where the worst bend over `from..to` is, and how big it is next to the wave.
fn worst_at(x: &[f32], from: usize, to: usize) -> (usize, f32) {
    let local = local_level(x);
    let floor = 0.01 * local.iter().fold(0.0f32, |m, s| m.max(*s)).max(1e-9);
    let to = to.min(x.len());
    let mut worst = (from.max(2), 0.0f32);
    for k in from.max(2)..to {
        let bend = (x[k] - 2.0 * x[k - 1] + x[k - 2]).abs() / local[k].max(floor);
        if bend > worst.1 {
            worst = (k, bend);
        }
    }
    worst
}

fn cell(db: f32) -> String {
    if !db.is_finite() {
        "—".to_owned()
    } else if db.abs() < 0.05 {
        "0.0".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

/// Frames the runs play; the event lands at [`AT`].
const FRAMES: usize = 24_000;
const AT: usize = 12_000;
/// How far either side of the event a click is looked for.
const SPAN: usize = 2_400;

// The harness is a measurement; these are the checks it is read from.

/// The events of OR-4's (c) to (g), each at three block sizes, with Gravity in
/// the chain: none of them may bend the wave by more than six decibels (a click
/// was twenty-six to thirty-two). Six is the transport's own floor: its
/// two-millisecond crossfade bends the wave by four, with or without a device.
#[test]
fn nothing_the_engine_does_to_a_chain_clicks() {
    let input = tone(FRAMES);
    let reference = tone(FRAMES);
    for block in [64usize, 128, 256] {
        let event = AT / block;
        let mut cases: Vec<(&str, Vec<f32>)> = Vec::new();
        // The power switched off, and back on.
        for (name, bypass) in [("power off", true), ("power on", false)] {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(!bypass, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(bypass, false)).unwrap());
                }
            });
            cases.push((name, out));
        }
        // The device taken out of a playing chain, and put back.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&plain()).unwrap());
                }
            });
            cases.push(("removed", out));
        }
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &plain());
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(false, false)).unwrap());
                }
            });
            cases.push(("inserted", out));
        }
        // The interface rewriting the same routing every block.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            let same = routing(false, false);
            rig.run(FRAMES, block, &mut out, move |rig, _| {
                let _ = rig.routing.set(&same);
            });
            cases.push(("rewritten", out));
        }
        // Stop, play again, and a locate.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    rig.transport.send(Command::Stop).unwrap();
                }
                if index == event + 1 {
                    rig.transport.send(Command::Play).unwrap();
                }
            });
            cases.push(("stop+play", out));
        }
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    rig.transport
                        .send(Command::Locate(Ticks::quarters(1)))
                        .unwrap();
                }
            });
            cases.push(("locate", out));
        }
        for (name, out) in cases {
            let db = click_db(&out, &reference, AT, SPAN);
            assert!(db < 6.0, "{name} at {block}: {db:+.1} dB above the wave");
        }
    }
}

/// A key connected to a playing compressor does not step: it is an input to the
/// detector, and the detector's ballistics take it up.
#[test]
fn connecting_a_key_does_not_click() {
    let input = tone(FRAMES);
    let mut key = vec![0.0f32; FRAMES];
    for (k, s) in key.iter_mut().enumerate() {
        *s = if (k / 2_400) % 2 == 0 { 0.9 } else { 0.0 };
    }
    let reference = tone(FRAMES);
    for block in [64usize, 128, 256] {
        let event = AT / block;
        for (name, connect) in [("key on", true), ("key off", false)] {
            let clips = vec![clip(TRACK, 1, &input), clip(KEYED, 2, &key)];
            let mut rig = rig(clips, &routing(false, !connect));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(false, connect)).unwrap());
                }
            });
            let db = click_db(&out, &reference, AT, SPAN);
            assert!(db < 6.0, "{name} at {block}: {db:+.1} dB above the wave");
        }
    }
}

#[test]
#[ignore = "a measurement, not a check: prints the table"]
fn the_engine_pop_table() {
    let input = tone(FRAMES);
    let reference = tone(FRAMES);
    println!(
        "\n{:<24} {:>6} {:>10} {:>10}",
        "event", "block", "with dev", "no device"
    );
    for block in [64usize, 128, 256] {
        let event = AT / block;
        let mut rows: Vec<(&str, Vec<f32>, Vec<f32>)> = Vec::new();
        // (a) steady: nothing happens at all.
        {
            let mut with = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            with.attach();
            with.play();
            let mut out = Vec::new();
            with.run(FRAMES, block, &mut out, |_, _| {});
            let mut bare_rig = rig(vec![clip(TRACK, 1, &input)], &plain());
            bare_rig.play();
            let mut plain_out = Vec::new();
            bare_rig.run(FRAMES, block, &mut plain_out, |_, _| {});
            rows.push(("steady", out, plain_out));
        }
        // (c) the power switched off.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(true, false)).unwrap());
                }
            });
            rows.push(("power off (c)", out, Vec::new()));
        }
        // (d) the device taken out of a playing chain.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&plain()).unwrap());
                }
            });
            rows.push(("removed (d)", out, Vec::new()));
        }
        // (d) and put back into a playing chain.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &plain());
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(false, false)).unwrap());
                }
            });
            rows.push(("inserted (d)", out, Vec::new()));
        }
        // (g) the interface rewriting the same routing every block.
        {
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            let same = routing(false, false);
            rig.run(FRAMES, block, &mut out, move |rig, _| {
                let _ = rig.routing.set(&same);
            });
            rows.push(("rewritten (g)", out, Vec::new()));
        }
        // (e) stop, then play again. With and without a device: the transport
        // is the engine's own, and a click there is not the built-in's.
        for (name, with) in [("stop+play (e)", true), ("stop+play, no dev", false)] {
            let route = if with { routing(false, false) } else { plain() };
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &route);
            if with {
                rig.attach();
            }
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    rig.transport.send(Command::Stop).unwrap();
                }
                if index == event + 1 {
                    rig.transport.send(Command::Play).unwrap();
                }
            });
            rows.push((name, out, Vec::new()));
        }
        // (e) a locate while playing.
        for (name, with) in [("locate (e)", true), ("locate, no dev", false)] {
            let route = if with { routing(false, false) } else { plain() };
            let mut rig = rig(vec![clip(TRACK, 1, &input)], &route);
            if with {
                rig.attach();
            }
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    rig.transport
                        .send(Command::Locate(Ticks::quarters(1)))
                        .unwrap();
                }
            });
            rows.push((name, out, Vec::new()));
        }
        for (name, out, bare) in rows {
            let bare_cell = if bare.is_empty() {
                "—".to_owned()
            } else {
                cell(click_db(&bare, &reference, AT, SPAN))
            };
            println!(
                "{name:<24} {block:>6} {:>10} {:>10}",
                cell(click_db(&out, &reference, AT, SPAN)),
                bare_cell
            );
            if std::env::var_os("POPS_DUMP").is_some() && name.starts_with("stop") {
                let (where_at, how) = worst_at(&out, AT - SPAN, AT + SPAN);
                let from = where_at.saturating_sub(8);
                println!("   at {where_at} ({how:.4}): {:?}", &out[from..from + 16]);
            }
        }
    }
}

#[test]
#[ignore = "a measurement, not a check: prints the table"]
fn the_key_pop_table() {
    // A key that is loud and pulsing: connecting it takes the input far down.
    let input = tone(FRAMES);
    let mut key = vec![0.0f32; FRAMES];
    for (k, s) in key.iter_mut().enumerate() {
        *s = if (k / 2_400) % 2 == 0 { 0.9 } else { 0.0 };
    }
    let reference = tone(FRAMES);
    println!("\n{:<24} {:>6} {:>10}", "event", "block", "bend/dry");
    for block in [64usize, 128, 256] {
        let event = AT / block;
        for (name, connect) in [("key on (f)", true), ("key off (f)", false)] {
            let clips = vec![clip(TRACK, 1, &input), clip(KEYED, 2, &key)];
            let mut rig = rig(clips, &routing(false, !connect));
            rig.attach();
            rig.play();
            let mut out = Vec::new();
            rig.run(FRAMES, block, &mut out, move |rig, index| {
                if index == event {
                    assert!(rig.routing.set(&routing(false, connect)).unwrap());
                }
            });
            println!(
                "{name:<24} {block:>6} {:>10}",
                cell(click_db(&out, &reference, AT, SPAN))
            );
        }
    }
}

/// A chain with a device that does nothing: the shape of a plan with a device
/// and without one, for the harness's own check.
#[test]
fn the_harness_sees_a_click_it_is_given() {
    // A tone, and the same tone with one sample stepped: the detector must call
    // that a click, or none of its quiet numbers mean anything.
    let clean = tone(4_800);
    let mut clicked = clean.clone();
    let at = 2_400;
    for s in &mut clicked[at..] {
        *s += 0.02;
    }
    let quiet = click_db(&clean, &clean, at, 1_200);
    let loud = click_db(&clicked, &clean, at, 1_200);
    assert!(quiet.abs() < 1.0, "a clean tone reads {quiet} dB");
    assert!(loud > 12.0, "a stepped tone reads only {loud} dB");
}

/// The engine's fallback for a device that is not attached is the dry signal:
/// nothing here is the harness mistaking its own plumbing for a device.
#[test]
fn an_unattached_device_plays_the_wave_through() {
    let input = tone(FRAMES);
    let mut rig = rig(vec![clip(TRACK, 1, &input)], &routing(false, false));
    rig.play();
    let mut out = Vec::new();
    rig.run(FRAMES, 128, &mut out, |_, _| {});
    let at = 16_000;
    for k in at..at + 1_000 {
        assert!(
            (out[k] - input[k]).abs() < 1e-6,
            "frame {k}: {} against {}",
            out[k],
            input[k]
        );
    }
}
