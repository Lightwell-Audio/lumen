//! Meter golden values (M4 slice 5): levels are measured on what a node
//! plays on, after fader, pan and the solo gate; a full-scale sine reads
//! 0.0 dB; the master's loudness reads EBU reference tones within 0.1 LU.

use std::sync::Arc;

use lumen_core::loudness::Loudness;
use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, Command, Destination, Engine, EngineHandle, Level, Mixer,
    MixerHandle, NodeSpec, Routing, SendSpec, Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const RETURN: u64 = 50;

struct Rig {
    _transport: EngineHandle,
    _clips: lumen_engine::ClipsHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<ClipPlayer>>,
}

/// A stereo sine at `dbfs`, `seconds` long, starting at phase zero.
fn sine(frequency: f64, dbfs: f64, seconds: f64) -> Arc<ClipAudio> {
    let amplitude = 10f64.powf(dbfs / 20.0);
    let side: Vec<f32> = (0..(seconds * f64::from(RATE)) as usize)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            (amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin()) as f32
        })
        .collect();
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![side.clone(), side],
    })
}

fn clip(track: u64, audio: &Arc<ClipAudio>) -> Clip {
    Clip {
        envelope: Default::default(),
        id: track,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: audio.frames() as i64,
        gain: 1.0,
        audio: Arc::clone(audio),
        shape: Default::default(),
    }
}

fn routing(nodes: Vec<NodeSpec>) -> Routing {
    let mut all = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    all.extend(nodes);
    Routing {
        nodes: all,
        cue: None,
        measure: Some(MASTER),
    }
}

fn to_master(id: u64) -> NodeSpec {
    NodeSpec::unity(id, Destination::Node(MASTER))
}

fn rig(clips: Vec<Clip>, routing: &Routing) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(routing).unwrap());
    let (mut transport, engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    Rig {
        _transport: transport,
        _clips: clips_handle,
        routing: routing_handle,
        engine,
    }
}

impl Rig {
    fn run(&mut self, frames: usize) {
        let mut data = vec![0.0; 2 * 128];
        for _ in 0..frames.div_ceil(128) {
            self.engine.process(RATE, &mut data, 2, 128);
        }
    }

    /// Plays `seconds`, then takes node `id`'s level.
    fn level_after(&mut self, id: u64, seconds: f64) -> Level {
        self.run((seconds * f64::from(RATE)) as usize);
        self.routing.take_level(id).expect("routed")
    }
}

fn db(linear: f32) -> f32 {
    20.0 * linear.log10()
}

#[test]
fn a_full_scale_sine_reads_zero_db_peak_and_minus_3_rms() {
    // 1 kHz at 48 kHz: every 12th sample lands on the crest.
    let tone = sine(1_000.0, 0.0, 2.0);
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![to_master(1)]));
    let level = r.level_after(1, 1.5);
    for side in 0..2 {
        assert!(db(level.peak[side]).abs() < 0.01, "{level:?}");
        assert!((db(level.rms[side]) + 3.01).abs() < 0.05, "{level:?}");
    }
    assert!(!level.clipped);
    // The master carries the same.
    let master = r.routing.take_level(MASTER).unwrap();
    assert!(db(master.peak[0]).abs() < 0.01);
}

#[test]
fn levels_are_read_after_the_fader_and_pan() {
    let tone = sine(1_000.0, 0.0, 2.0);
    let mut track = to_master(1);
    track.volume = 0.5;
    track.pan = -1.0;
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![track]));
    let level = r.level_after(1, 1.0);
    // Balance law: hard left keeps the left side and silences the right.
    assert!((db(level.peak[0]) + 6.02).abs() < 0.02, "{level:?}");
    assert_eq!(level.peak[1], 0.0);
}

#[test]
fn a_muted_node_reads_silence_and_its_rms_falls() {
    let tone = sine(1_000.0, -6.0, 4.0);
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![to_master(1)]));
    let playing = r.level_after(1, 1.0).rms[0];
    assert!(playing > 0.1);
    let mut muted = to_master(1);
    muted.audible = false;
    muted.sends_audible = false;
    assert!(r.routing.set(&routing(vec![muted])).unwrap());
    // The 300 ms RMS falls about 14.5 dB a second.
    let level = r.level_after(1, 2.0);
    assert!(db(level.rms[0] / playing) < -25.0, "{level:?}");
    // Read again: no peak since.
    let after = r.routing.take_level(1).unwrap();
    assert_eq!(after.peak, [0.0, 0.0]);
}

#[test]
fn a_return_soloed_wet_only_shows_its_senders_as_silent() {
    let tone = sine(1_000.0, -6.0, 3.0);
    let mut sender = to_master(1);
    sender.audible = false;
    sender.sends.push(SendSpec {
        to: RETURN,
        gain: 1.0,
        pre_fader: false,
    });
    let mut r = rig(
        vec![clip(1, &tone)],
        &routing(vec![sender, to_master(RETURN)]),
    );
    r.run(RATE as usize);
    let sender = r.routing.take_level(1).unwrap();
    let wet = r.routing.take_level(RETURN).unwrap();
    assert_eq!(sender.peak, [0.0, 0.0], "the sender's output is not heard");
    assert!((db(wet.peak[0]) + 6.0).abs() < 0.05, "{wet:?}");
}

#[test]
fn going_over_full_scale_latches_the_clip_until_read() {
    let tone = sine(1_000.0, 0.0, 2.0);
    let mut hot = to_master(1);
    hot.volume = 1.5;
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![hot]));
    let level = r.level_after(1, 0.5);
    assert!(level.clipped);
    assert!(db(level.peak[0]) > 3.0);
    // Back under: read once more, then no clip.
    assert!(r.routing.set(&routing(vec![to_master(1)])).unwrap());
    r.run(RATE as usize / 2);
    let _ = r.routing.take_level(1);
    assert!(!r.level_after(1, 0.5).clipped);
}

#[test]
fn the_master_reads_an_ebu_reference_tone_at_minus_23_lufs() {
    // EBU Tech 3341 case 1: a stereo 1 kHz tone at −23 dBFS.
    let tone = sine(1_000.0, -23.0, 12.0);
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![to_master(1)]));
    let mut loudness = Loudness::new();
    for _ in 0..10 {
        r.run(RATE as usize);
        r.routing.take_loudness(|z| loudness.push(z));
    }
    assert!(
        (loudness.momentary() + 23.0).abs() < 0.1,
        "{}",
        loudness.momentary()
    );
    assert!((loudness.short_term() + 23.0).abs() < 0.1);
    assert!((loudness.integrated() + 23.0).abs() < 0.1);
    let true_peak = db(r.routing.take_true_peak());
    assert!((true_peak + 23.0).abs() < 0.2, "{true_peak}");
    // Read: the true peak starts again.
    assert_eq!(r.routing.take_true_peak(), 0.0);
}

#[test]
fn stopped_meters_fall_to_rest_without_rendering() {
    let tone = sine(1_000.0, -6.0, 4.0);
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![to_master(1)]));
    let playing = r.level_after(1, 1.0).rms[0];
    r._transport.send(Command::Stop).unwrap();
    // Two seconds stopped: the 300 ms RMS falls about 29 dB.
    let stopped = r.level_after(1, 2.0);
    assert_eq!(stopped.peak, [0.0, 0.0]);
    let fallen = db(stopped.rms[0] / playing);
    assert!((-31.0..-27.0).contains(&fallen), "{fallen}");
    // Long enough, it reaches silence.
    assert_eq!(r.level_after(1, 30.0).rms[0], 0.0);
}

#[test]
fn a_node_the_routing_does_not_have_has_no_level() {
    let r = rig(Vec::new(), &routing(Vec::new()));
    assert!(r.routing.take_level(42).is_none());
    assert!(r.routing.take_level(MASTER).is_some());
}

#[test]
fn many_nodes_get_a_bigger_bank_and_keep_reading() {
    let tone = sine(1_000.0, 0.0, 3.0);
    let mut r = rig(vec![clip(1, &tone)], &routing(vec![to_master(1)]));
    // Far more nodes than the first bank holds, added and removed.
    for round in 0..4 {
        let mut nodes = vec![to_master(1)];
        nodes.extend((0..100).map(|i| to_master(1_000 + round * 100 + i)));
        assert!(r.routing.set(&routing(nodes)).unwrap());
        r.run(4_800);
    }
    let level = r.level_after(1, 0.5);
    assert!(db(level.peak[0]).abs() < 0.01, "{level:?}");
}
