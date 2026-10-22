//! Hosted effects through the mixer (M6 slice 2b, ADR-0069). A fake effect
//! stands for the plugin's process: it halves its input, after a delay of its
//! own latency, and answers at once or never. The mixer hands the hosts their
//! blocks before its node loop, collects them against one deadline, and falls
//! back to the input delayed by the reported latency while a host is
//! bypassed, failed or late, so time never shifts.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use lumen_core::automation::Shape;
use lumen_core::midi::Event;
use lumen_core::time::Ticks;
use lumen_devices::{Guard, Hosted, ParamBlock, Refused, Reply, Status};
use lumen_engine::{
    AutoLane, AutoPoint, Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Destination, Device,
    Engine, EngineHandle, Mixer, MixerHandle, NodeAutomation, NodeSpec, ParamLane, Routing,
    ScrubStyle, SidechainSpec, Tap, Timeline, clip_player, engine, mixer,
};
use lumen_rt::count_rt_violations;

lumen_rt::install_guarded_allocator!();

const MASTER: u64 = 100;

/// What the fake effect does with a block.
#[derive(Clone)]
enum Mode {
    /// Half the input, delayed by the latency: answers at once.
    Halve,
    /// Never answers in time.
    Late,
    /// Is always still on an earlier block: refuses every one.
    Busy,
    /// Halves, but a worker takes this long first (each block is counted in
    /// `overlap`).
    Slow(Duration, Arc<Overlap>),
    /// Halves and adds a level of its own (the bits of an `f32`), as an amp
    /// with a long reverb sounds on after its input has stopped.
    Hum(Arc<AtomicU32>),
    /// Multiplies its input by the value of parameter `id`, held from the
    /// sample the block says it changed on, as a plugin applies automation.
    Follow(u32),
    /// Plays the left channel of its sidechain input (the third channel it
    /// is given) and nothing of its audio; silence when it is given none.
    Key,
}

#[derive(Default)]
struct Overlap {
    /// Blocks handed to slow hosts so far.
    submitted: AtomicU32,
    /// How many had been handed over when the first host was collected (0 until then).
    before_first_collect: AtomicU32,
}

struct FakeEffect {
    mode: Mode,
    latency: u32,
    /// The input, per channel, `latency` frames deep, for the delay.
    ring: Vec<f32>,
    at: usize,
    out: [Vec<f32>; 2],
    done: Option<Arc<AtomicBool>>,
    submits: Arc<AtomicU32>,
    /// Times the engine asked it to forget what it holds.
    flushes: Arc<AtomicU32>,
    /// The value of the followed parameter now.
    level: f32,
}

impl FakeEffect {
    fn new(mode: Mode, latency: u32) -> (Box<Self>, Arc<AtomicU32>) {
        let submits = Arc::new(AtomicU32::new(0));
        let effect = Box::new(Self {
            mode,
            latency,
            ring: vec![0.0; 2 * latency.max(1) as usize],
            at: 0,
            out: [Vec::with_capacity(256), Vec::with_capacity(256)],
            done: None,
            submits: Arc::clone(&submits),
            flushes: Arc::new(AtomicU32::new(0)),
            level: 1.0,
        });
        (effect, submits)
    }
}

impl Hosted for FakeEffect {
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        _: &[Event],
        params: &ParamBlock,
    ) -> Result<(), Refused> {
        self.submits.fetch_add(1, Ordering::Relaxed);
        if matches!(self.mode, Mode::Busy) {
            return Err(Refused::Busy);
        }
        if let Mode::Follow(id) = self.mode {
            let mut queue = None;
            for i in 0..params.len() {
                if params.queue(i).0 == id {
                    queue = Some(params.queue(i).1);
                }
            }
            let points = queue.unwrap_or(&[]);
            let mut next = 0;
            for (channel, out) in self.out.iter_mut().enumerate() {
                out.clear();
                let (mut level, mut next_point) = (self.level, 0);
                for (k, &sample) in input[channel][..frames].iter().enumerate() {
                    while next_point < points.len() && points[next_point].0 as usize <= k {
                        level = points[next_point].1 as f32;
                        next_point += 1;
                    }
                    out.push(sample * level);
                }
                next = next_point;
                if channel == 1 {
                    self.level = level;
                }
            }
            let _ = next;
            return Ok(());
        }
        let depth = self.latency as usize;
        for (channel, out) in self.out.iter_mut().enumerate() {
            out.clear();
            if matches!(self.mode, Mode::Key) {
                match input.get(2) {
                    Some(key) => out.extend_from_slice(&key[..frames]),
                    None => out.resize(frames, 0.0),
                }
                continue;
            }
            for (k, &sample) in input[channel][..frames].iter().enumerate() {
                let heard = if depth == 0 {
                    sample
                } else {
                    let slot = channel * depth + (self.at + k) % depth;
                    std::mem::replace(&mut self.ring[slot], sample)
                };
                out.push(heard * 0.5);
            }
            if let Mode::Hum(level) = &self.mode {
                let level = f32::from_bits(level.load(Ordering::Relaxed));
                out.iter_mut().for_each(|s| *s += level);
            }
        }
        self.at = (self.at + frames) % depth.max(1);
        if let Mode::Slow(delay, overlap) = self.mode.clone() {
            overlap.submitted.fetch_add(1, Ordering::AcqRel);
            let done = Arc::new(AtomicBool::new(false));
            self.done = Some(Arc::clone(&done));
            thread::spawn(move || {
                thread::sleep(delay);
                done.store(true, Ordering::Release);
            });
        }
        Ok(())
    }

    fn collect(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        if matches!(self.mode, Mode::Late) {
            return Reply::Late;
        }
        if let Mode::Slow(_, overlap) = &self.mode {
            let submitted = overlap.submitted.load(Ordering::Acquire);
            let _ = overlap.before_first_collect.compare_exchange(
                0,
                submitted,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        if let Some(done) = &self.done {
            while !done.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    return Reply::Late;
                }
                std::hint::spin_loop();
            }
        }
        left.copy_from_slice(&self.out[0]);
        right.copy_from_slice(&self.out[1]);
        Reply::Ready {
            latency: self.latency,
            tail: 0,
            restart: false,
            work_ns: 0,
        }
    }

    fn ended(&self) -> bool {
        false
    }

    fn flush(&mut self) {
        self.flushes.fetch_add(1, Ordering::Relaxed);
        self.ring.fill(0.0);
    }
}

const RATE: u32 = 48_000;

/// Audio whose every sample differs, to catch any misplaced frame.
fn ramp() -> Vec<f32> {
    (0..480_000)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect()
}

fn audio() -> Arc<ClipAudio> {
    let samples = ramp();
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![samples.clone(), samples],
    })
}

fn clip(id: u64, track: u64, audio: &Arc<ClipAudio>) -> Clip {
    Clip {
        envelope: Default::default(),
        id,
        track,
        start: Ticks::ZERO,
        offset: 0,
        length: audio.frames() as i64,
        gain: 1.0,
        audio: Arc::clone(audio),
        shape: Default::default(),
    }
}

/// A track into the master with these devices.
fn track(id: u64, devices: Vec<Device>) -> NodeSpec {
    NodeSpec {
        devices,
        ..NodeSpec::unity(id, Destination::Node(MASTER))
    }
}

fn routing_of(nodes: Vec<NodeSpec>) -> Routing {
    let mut all = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    all.extend(nodes);
    Routing {
        nodes: all,
        cue: None,
        measure: None,
    }
}

fn hosted(host: u64, latency: u32) -> Vec<Device> {
    vec![Device::Hosted { host, latency }]
}

struct Rig {
    _clips: ClipsHandle,
    transport: EngineHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<ClipPlayer>>,
    /// Frames rendered so far.
    at: usize,
}

fn rig(tracks: u64, rate: u32, routing: &Routing) -> Rig {
    let audio = audio();
    let (mut clips, player) = clip_player();
    clips
        .set((1..=tracks).map(|t| clip(t, t, &audio)).collect())
        .unwrap();
    let (mut handle, source) = mixer(player);
    assert!(handle.set(routing).unwrap());
    let (transport, engine) = engine(Timeline::default(), source, rate);
    Rig {
        _clips: clips,
        transport,
        routing: handle,
        engine,
        at: 0,
    }
}

impl Rig {
    fn attach(&mut self, id: u64, effect: Box<FakeEffect>, latency: u32) -> Arc<Status> {
        let guard = Guard::new(effect, latency);
        let status = guard.status();
        assert!(self.routing.attach_host(id, guard).is_ok());
        status
    }

    fn play(&mut self) {
        self.transport.send(Command::Play).unwrap();
    }

    /// The left channel of the next `frames`, in blocks of `block`.
    fn run(&mut self, frames: usize, block: usize) -> Vec<f32> {
        let mut data = vec![0.0; 2 * block];
        let mut out = Vec::with_capacity(frames);
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine.process(RATE, &mut data[..2 * n], 2, n);
            out.extend_from_slice(&data[..n]);
            done += n;
        }
        self.at += frames;
        out
    }
}

/// Checks `out`, which starts at the frame `from`, against `want(frame)` from
/// frame `check` on.
fn assert_matches(out: &[f32], from: usize, check: usize, what: &str, want: impl Fn(usize) -> f32) {
    for (k, &got) in out.iter().enumerate().skip(check.saturating_sub(from)) {
        let frame = from + k;
        let expected = want(frame);
        assert!(
            (got - expected).abs() < 1e-5,
            "{what}: frame {frame} is {got}, expected {expected}"
        );
    }
}

#[test]
fn a_hosted_effect_processes_the_tracks_audio_sample_for_sample_at_any_block_size() {
    let r = ramp();
    for block in [64, 100, 128, 333, 480] {
        let routing = routing_of(vec![track(1, hosted(7, 0))]);
        let mut rig = rig(1, RATE, &routing);
        let (effect, _) = FakeEffect::new(Mode::Halve, 0);
        let status = rig.attach(7, effect, 0);
        rig.play();
        let out = rig.run(20_000, block);
        // The first 20 ms glide the host in from its fallback.
        assert_matches(&out, 0, 3_000, &format!("blocks of {block}"), |n| {
            r[n] * 0.5
        });
        assert!(!status.failed());
        assert_eq!(status.late_blocks(), 0);
    }
}

#[test]
fn a_hosted_effects_latency_lines_up_with_a_parallel_track() {
    let r = ramp();
    let latency = 64;
    // Track 1 goes through the plugin, which delays by 64 frames; track 2 is
    // plain. What is heard lines up with the timeline: 0.5·r + r.
    let routing = routing_of(vec![track(1, hosted(7, latency)), track(2, vec![])]);
    let mut rig = rig(2, RATE, &routing);
    let (effect, _) = FakeEffect::new(Mode::Halve, latency);
    rig.attach(7, effect, latency);
    assert_eq!(rig.routing.compensation().frames, latency);
    rig.play();
    let out = rig.run(30_000, 128);
    assert_matches(&out, 0, 4_000, "parallel paths", |n| r[n - 64] * 1.5);
}

#[test]
fn bypass_passes_the_input_at_the_reported_latency_without_a_step() {
    let r = ramp();
    let latency = 32;
    let routing = routing_of(vec![track(1, hosted(7, latency))]);
    let mut rig = rig(1, RATE, &routing);
    let (effect, submits) = FakeEffect::new(Mode::Halve, latency);
    let status = rig.attach(7, effect, latency);
    rig.play();
    let before = rig.run(20_000, 128);
    assert_matches(&before, 0, 3_000, "before", |n| r[n - 32] * 0.5);
    status.set_bypass(true);
    let after = rig.run(20_000, 128);
    // The gain from the input to what is heard glides from 0.5 (the host) to
    // 1 (the fallback) over 20 ms: a straight line, no step. (The ramp itself
    // wraps once a period, so the gain is read where the input is not near
    // zero.)
    let mut gains: Vec<(usize, f32)> = Vec::new();
    for (k, &heard) in after.iter().enumerate().take(3_000) {
        let input = r[20_000 + k - 32];
        if input.abs() > 0.05 {
            gains.push((k, heard / input));
        }
    }
    assert!(gains.iter().all(|&(_, g)| (0.499..=1.001).contains(&g)));
    for pair in gains.windows(2) {
        if pair[1].0 == pair[0].0 + 1 {
            assert!(
                (pair[1].1 - pair[0].1).abs() < 0.003,
                "a step in the bypass glide at frame {}: {} to {}",
                pair[1].0,
                pair[0].1,
                pair[1].1
            );
        }
    }
    let end = gains.last().expect("gains were measured").1;
    assert!(end > 0.999, "the glide ends at the fallback: {end}");
    assert_matches(&after, 20_000, 24_000, "bypassed", |n| r[n - 32]);
    let asked = submits.load(Ordering::Relaxed);
    rig.run(5_000, 128);
    assert_eq!(
        submits.load(Ordering::Relaxed),
        asked,
        "a bypassed host is not asked for blocks"
    );
    // And back: the host is heard again.
    status.set_bypass(false);
    let again = rig.run(20_000, 128);
    assert_matches(&again, 45_000, 49_000, "enabled again", |n| r[n - 32] * 0.5);
}

#[test]
fn a_host_that_fails_is_replaced_by_its_fallback_at_the_same_time() {
    let r = ramp();
    let latency = 32;
    let routing = routing_of(vec![track(1, hosted(7, latency))]);
    let mut rig = rig(1, RATE, &routing);
    let (effect, _) = FakeEffect::new(Mode::Late, latency);
    let status = rig.attach(7, effect, latency);
    rig.play();
    let out = rig.run(60_000, 128);
    assert!(status.failed(), "a second of late blocks in a row");
    assert_matches(&out, 0, 2_000, "the fallback", |n| r[n - 32]);
}

#[test]
fn a_block_a_busy_host_refused_is_one_missed_block_however_often_it_is_tried() {
    // The first device of a node is handed its block early and again at the
    // node: the host being busy is one missed block, not two (review H4).
    let routing = routing_of(vec![track(1, hosted(7, 0))]);
    let mut rig = rig(1, RATE, &routing);
    let (effect, submits) = FakeEffect::new(Mode::Busy, 0);
    let status = rig.attach(7, effect, 0);
    rig.play();
    rig.run(128, 128);
    assert!(submits.load(Ordering::Relaxed) >= 1, "the host was asked");
    assert_eq!(status.late_blocks(), 1);
}

#[test]
fn a_hosted_effect_rings_out_after_stop_until_its_output_is_quiet() {
    // The owner's report: an amp's delay and reverb ended a moment after the
    // notes did. A plugin still sounding after Stop is heard until it goes
    // quiet, then left alone.
    let routing = routing_of(vec![track(1, hosted(7, 0))]);
    let mut rig = rig(1, RATE, &routing);
    let level = Arc::new(AtomicU32::new(0.25f32.to_bits()));
    let (effect, submits) = FakeEffect::new(Mode::Hum(Arc::clone(&level)), 0);
    rig.attach(7, effect, 0);
    rig.play();
    rig.run(10_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    let seconds = |n: usize| n * RATE as usize;
    let after = rig.run(seconds(10), 128);
    assert_matches(&after, 0, seconds(1), "ten seconds after Stop", |_| 0.25);

    level.store(0.0f32.to_bits(), Ordering::Relaxed);
    let asked = submits.load(Ordering::Relaxed);
    rig.run(seconds(3), 128);
    assert_eq!(
        submits.load(Ordering::Relaxed) - asked,
        (seconds(2) / 128) as u32,
        "asked for blocks through two seconds of quiet"
    );
    let asked = submits.load(Ordering::Relaxed);
    rig.run(seconds(1), 128);
    assert_eq!(
        submits.load(Ordering::Relaxed),
        asked,
        "and left alone once quiet"
    );
}

/// Track 1 with a hosted effect that plays its sidechain, keyed from track 2
/// at `tap`; track 2 plays to nowhere, so only the key reaches the master.
fn keyed(tap: Tap, source: NodeSpec) -> Routing {
    let mut effect = track(1, hosted(7, 0));
    effect.sidechains.push(SidechainSpec { from: 2, tap });
    routing_of(vec![effect, source])
}

fn source(devices: Vec<Device>, audible: bool) -> NodeSpec {
    NodeSpec {
        devices,
        audible,
        sends_audible: audible,
        ..NodeSpec::unity(2, Destination::None)
    }
}

/// Frames the effect's wet glide (20 ms) takes to be done with the fallback.
const GLIDE: usize = 2_400;

fn key_rig(routing: &Routing) -> (Rig, Vec<f32>) {
    key_rig_in(routing, 128)
}

fn key_rig_in(routing: &Routing, block: usize) -> (Rig, Vec<f32>) {
    let mut rig = rig(2, RATE, routing);
    let (effect, _) = FakeEffect::new(Mode::Key, 0);
    rig.attach(7, effect, 0);
    rig.play();
    let out = rig.run(RATE as usize / 2, block);
    (rig, out)
}

fn latent_device(frames: u32) -> Vec<Device> {
    vec![Device::Delay {
        frames,
        listen: false,
    }]
}

#[test]
fn a_hosted_effect_is_given_the_sidechain_on_the_samples_its_source_plays() {
    // The effect plays its key; track 3 plays the same audio upside down
    // straight into the master. Lined up, the two cancel exactly, whatever
    // the source's own latency and the block size.
    for (name, devices, block) in [
        ("plain source", vec![], 128),
        ("latent source", latent_device(240), 128),
        ("latent source, odd blocks", latent_device(333), 100),
    ] {
        let mut routing = keyed(Tap::PostFx, source(devices, true));
        routing.nodes.push(NodeSpec {
            invert: true,
            ..NodeSpec::unity(3, Destination::Node(MASTER))
        });
        let mut rig = rig(3, RATE, &routing);
        let (effect, _) = FakeEffect::new(Mode::Key, 0);
        rig.attach(7, effect, 0);
        rig.play();
        let out = rig.run(RATE as usize / 2, block);
        assert_matches(&out, 0, GLIDE, name, |_| 0.0);
        assert!(
            peak(&rig_alone(&out)) == 0.0,
            "{name}: something is left over"
        );
    }
}

fn rig_alone(out: &[f32]) -> Vec<f32> {
    out[GLIDE..].to_vec()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| f32::max(m, s.abs()))
}

#[test]
fn the_tap_decides_whether_a_muted_source_still_keys_the_effect() {
    // Mute comes after the devices: Pre and Post FX still carry the sound;
    // what the track plays does not.
    for (tap, heard) in [
        (Tap::PreFx, true),
        (Tap::PostFx, true),
        (Tap::PostMixer, false),
    ] {
        let (_, out) = key_rig(&keyed(tap, source(vec![], false)));
        let level = peak(&out[GLIDE..]);
        assert_eq!(level > 0.1, heard, "{tap:?}: peak {level}");
    }
}

#[test]
fn an_effect_with_no_sidechain_is_given_none_and_the_level_of_one_can_be_read() {
    let plain = routing_of(vec![track(1, hosted(7, 0)), source(vec![], true)]);
    let (rig, out) = key_rig(&plain);
    assert_eq!(peak(&out[GLIDE..]), 0.0, "no key, nothing to play");
    assert!(rig.routing.take_side_level(1).is_none());

    let (rig, _) = key_rig(&keyed(Tap::PostFx, source(vec![], true)));
    let level = rig
        .routing
        .take_side_level(1)
        .expect("a sidechain is metered");
    assert!(level.peak[0] > 0.1, "what arrives is measured: {level:?}");
    assert!(
        rig.routing.take_side_level(2).is_none(),
        "the source has none"
    );
}

#[test]
fn a_locate_makes_effects_forget_what_they_hold_and_a_loop_wrap_or_a_start_does_not() {
    let routing = routing_of(vec![track(1, hosted(7, 0)), track(2, hosted(8, 0))]);
    let mut rig = rig(2, RATE, &routing);
    let (effect, _) = FakeEffect::new(Mode::Halve, 0);
    let flushes = Arc::clone(&effect.flushes);
    rig.attach(7, effect, 0);
    let (other, _) = FakeEffect::new(Mode::Halve, 0);
    let others = Arc::clone(&other.flushes);
    rig.attach(8, other, 0);
    let count = |flushes: &Arc<AtomicU32>| flushes.load(Ordering::Relaxed);

    rig.play();
    rig.run(10_000, 128);
    assert_eq!((count(&flushes), count(&others)), (0, 0), "playing on");
    rig.transport
        .send(Command::Locate(Ticks::quarters(4)))
        .unwrap();
    rig.run(1_000, 128);
    assert_eq!(
        (count(&flushes), count(&others)),
        (1, 1),
        "a move of the playhead reaches every effect once"
    );

    // A loop wraps seamlessly: nothing is forgotten.
    rig.transport
        .send(Command::SetLoop(lumen_engine::Region::new(
            Ticks::ZERO,
            Ticks::quarters(1),
            true,
        )))
        .unwrap();
    rig.run(RATE as usize * 3, 128);
    assert_eq!(count(&flushes), 1, "three seconds of loop wraps");

    // Stop, then start: the tail rings out after Stop (ADR-0074) and playing
    // again is not a jump.
    rig.transport.send(Command::Stop).unwrap();
    rig.run(1_000, 128);
    rig.play();
    rig.run(1_000, 128);
    assert_eq!(count(&flushes), 1, "stop and play");
}

#[test]
fn a_bypassed_or_quiet_effect_does_not_keep_a_stopped_engine_running() {
    let routing = routing_of(vec![track(1, hosted(7, 0)), track(2, hosted(8, 0))]);
    let mut rig = rig(2, RATE, &routing);
    let level = Arc::new(AtomicU32::new(0.25f32.to_bits()));
    let (humming, hums) = FakeEffect::new(Mode::Hum(Arc::clone(&level)), 0);
    let (quiet, quiets) = FakeEffect::new(Mode::Hum(Arc::new(AtomicU32::new(0))), 0);
    let status = rig.attach(7, humming, 0);
    rig.attach(8, quiet, 0);
    rig.play();
    rig.run(10_000, 128);
    status.set_bypass(true);
    rig.run(10_000, 128);
    rig.transport.send(Command::Stop).unwrap();
    // A quiet plugin goes on only while the clip it heard rings in it.
    rig.run(RATE as usize * 3, 128);
    let (hummed, quieted) = (hums.load(Ordering::Relaxed), quiets.load(Ordering::Relaxed));
    rig.run(RATE as usize, 128);
    assert_eq!(hums.load(Ordering::Relaxed), hummed, "bypassed: not asked");
    assert_eq!(quiets.load(Ordering::Relaxed), quieted, "quiet: not asked");
}

#[test]
fn a_hosted_device_with_no_host_attached_plays_its_fallback() {
    let r = ramp();
    let routing = routing_of(vec![track(1, hosted(7, 16))]);
    let mut rig = rig(1, RATE, &routing);
    rig.play();
    let out = rig.run(10_000, 128);
    assert_matches(&out, 0, 2_000, "no host", |n| r[n - 16]);
}

#[test]
fn a_new_latency_from_the_host_is_news_and_a_new_routing_lines_the_paths_up() {
    let r = ramp();
    let latency = 64;
    // The routing was made when the plugin reported nothing; it reports 64.
    let first = routing_of(vec![track(1, hosted(7, 0)), track(2, vec![])]);
    let mut rig = rig(2, RATE, &first);
    let (effect, _) = FakeEffect::new(Mode::Halve, latency);
    let status = rig.attach(7, effect, 0);
    rig.play();
    rig.run(10_000, 128);
    assert_eq!(status.take_news(), Some(latency));
    assert_eq!(status.take_news(), None, "told once");
    // The interface hears it and sends the routing again.
    let second = routing_of(vec![track(1, hosted(7, latency)), track(2, vec![])]);
    assert!(rig.routing.set(&second).unwrap());
    let out = rig.run(40_000, 128);
    assert_matches(&out, 10_000, 40_000, "after the new routing", |n| {
        r[n] * 1.5
    });
}

#[test]
fn sixty_four_hosted_effects_have_their_blocks_in_flight_together() {
    const HOSTS: u64 = 64;
    let overlap = Arc::new(Overlap::default());
    let routing = routing_of(
        (1..=HOSTS)
            .map(|id| track(id, hosted(1_000 + id, 0)))
            .collect(),
    );
    // One part of 256 frames at 8 kHz, and hosts that take 50 ms: long enough
    // that every submit (a thread start each, in this fake) comes before the
    // first answer. Had the mixer waited for each host in turn, only one
    // block would ever be in flight.
    let mut rig = rig(HOSTS, 8_000, &routing);
    for id in 1..=HOSTS {
        let (effect, _) = FakeEffect::new(
            Mode::Slow(Duration::from_millis(50), Arc::clone(&overlap)),
            0,
        );
        rig.attach(1_000 + id, effect, 0);
    }
    rig.play();
    let mut data = vec![0.0; 2 * 256];
    rig.engine.process(8_000, &mut data, 2, 256);
    assert_eq!(
        overlap.before_first_collect.load(Ordering::Acquire),
        HOSTS as u32,
        "every leaf track handed its host the block before the first was waited for"
    );
}

#[test]
fn hosted_effects_never_touch_the_heap() {
    let routing = routing_of(vec![
        track(1, hosted(11, 0)),
        track(2, hosted(12, 48)),
        track(3, hosted(13, 0)),
        track(4, vec![]),
    ]);
    let mut rig = rig(4, RATE, &routing);
    let a = rig.attach(11, FakeEffect::new(Mode::Halve, 0).0, 0);
    rig.attach(12, FakeEffect::new(Mode::Halve, 48).0, 48);
    let late = rig.attach(13, FakeEffect::new(Mode::Late, 0).0, 0);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..4_000_u32 {
        match block {
            0 => rig.play(),
            700 => a.set_bypass(true),
            1_400 => a.set_bypass(false),
            1_500 => rig.transport.send(Command::Stop).unwrap(),
            1_600 => rig.play(),
            2_000 => {
                // A new routing swaps in with a crossfade, then a host goes.
                let again = routing_of(vec![
                    track(1, hosted(11, 0)),
                    track(2, hosted(12, 48)),
                    track(3, hosted(13, 0)),
                    track(4, hosted(14, 0)),
                ]);
                assert!(rig.routing.set(&again).unwrap());
            }
            2_600 => assert!(rig.routing.detach_host(12)),
            3_000 => rig
                .transport
                .send(Command::Locate(Ticks::quarters(8)))
                .unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| rig.engine.process(RATE, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        rig.transport.collect_garbage();
        rig.routing.collect_garbage();
    }
    assert!(late.failed());
    assert!(!a.failed());
}

#[test]
fn keyed_hosted_effects_at_every_tap_and_a_refusing_host_never_touch_the_heap() {
    // Three effects, each keyed from a latent track at another tap; a fourth
    // host that refuses every block; a fifth that has no sidechain.
    let latent = |id| NodeSpec {
        devices: latent_device(240),
        ..NodeSpec::unity(id, Destination::None)
    };
    let mut nodes = Vec::new();
    for (effect, key, tap, host) in [
        (1, 5, Tap::PreFx, 21),
        (2, 6, Tap::PostFx, 22),
        (3, 7, Tap::PostMixer, 23),
    ] {
        let mut keyed = track(effect, hosted(host, 0));
        keyed.sidechains.push(SidechainSpec { from: key, tap });
        nodes.push(keyed);
        nodes.push(latent(key));
    }
    nodes.push(track(4, hosted(24, 0)));
    nodes.push(track(8, hosted(25, 0)));
    let routing = routing_of(nodes);
    let mut rig = rig(8, RATE, &routing);
    for host in [21, 22, 23] {
        rig.attach(host, FakeEffect::new(Mode::Key, 0).0, 0);
    }
    let refusing = rig.attach(24, FakeEffect::new(Mode::Busy, 0).0, 0);
    rig.attach(25, FakeEffect::new(Mode::Halve, 0).0, 0);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..3_000_u32 {
        match block {
            0 => rig.play(),
            800 => rig.transport.send(Command::Stop).unwrap(),
            1_000 => rig
                .transport
                .send(Command::Scrub {
                    at: Ticks::quarters(2),
                    style: ScrubStyle::Loop {
                        length: Ticks::quarters(1),
                    },
                })
                .unwrap(),
            1_300 => rig.transport.send(Command::EndScrub).unwrap(),
            1_500 => rig.play(),
            2_000 => rig
                .transport
                .send(Command::Locate(Ticks::quarters(8)))
                .unwrap(),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| rig.engine.process(RATE, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        rig.transport.collect_garbage();
        rig.routing.collect_garbage();
    }
    assert!(
        refusing.failed(),
        "the host that refused every block failed"
    );
}

// Keeps the shared log type in step with the instrument tests.
#[allow(dead_code)]
type Log = Arc<Mutex<Vec<u8>>>;

/// A lane that holds `before` and steps to `after` on sample `at`.
fn step_lane(param: u32, at: i64, before: f32, after: f32) -> ParamLane {
    ParamLane {
        param,
        lane: AutoLane::new(vec![
            AutoPoint {
                at: 0,
                value: before,
                shape: Shape::Step,
            },
            AutoPoint {
                at,
                value: after,
                shape: Shape::Step,
            },
        ])
        .unwrap(),
    }
}

#[test]
fn a_parameter_lane_changes_the_plugin_on_its_exact_sample_at_any_block_size() {
    let r = ramp();
    for block in [64, 100, 128, 256, 333] {
        let mut node = track(1, hosted(7, 0));
        node.automation = Some(NodeAutomation {
            plugins: vec![(7, step_lane(3, 10_001, 0.25, 1.0))],
            ..Default::default()
        });
        let mut rig = rig(1, RATE, &routing_of(vec![node]));
        let (effect, _) = FakeEffect::new(Mode::Follow(3), 0);
        let status = rig.attach(7, effect, 0);
        rig.play();
        let out = rig.run(20_000, block);
        // Past the 20 ms glide in from the fallback: the lane's level, with
        // the change on sample 10,001 and not the one before.
        assert_matches(&out, 0, 3_000, &format!("blocks of {block}"), |n| {
            r[n] * if n < 10_001 { 0.25 } else { 1.0 }
        });
        assert!(!status.failed());
    }
}

#[test]
fn a_parameter_lane_is_read_where_the_audio_is_after_an_earlier_devices_latency() {
    // A 64-frame effect ahead of the one with the lane: the audio the second
    // is given trails the timeline by 64 frames, and the lane is read there.
    let r = ramp();
    let mut node = track(
        1,
        vec![
            Device::Hosted {
                host: 6,
                latency: 64,
            },
            Device::Hosted {
                host: 7,
                latency: 0,
            },
        ],
    );
    node.automation = Some(NodeAutomation {
        plugins: vec![(7, step_lane(3, 10_000, 0.25, 1.0))],
        ..Default::default()
    });
    let mut rig = rig(1, RATE, &routing_of(vec![node]));
    let (first, _) = FakeEffect::new(Mode::Halve, 64);
    rig.attach(6, first, 64);
    let (second, _) = FakeEffect::new(Mode::Follow(3), 0);
    rig.attach(7, second, 0);
    rig.play();
    let out = rig.run(20_000, 128);
    // The audio reaches the second plugin 64 frames late, so the level
    // changes with it, at 10,064 in what is heard (the first halves it).
    assert_matches(&out, 0, 3_000, "the second plugin", |n| {
        r[n - 64] * 0.5 * if n < 10_064 { 0.25 } else { 1.0 }
    });
}

#[test]
fn parameter_lanes_on_plugins_never_touch_the_heap_through_every_transport_move() {
    // Curved, stepped and straight lanes on two plugins of a node, the first
    // handed its block early, played through plays, stops, locates, a loop
    // wrap's worth of blocks and a new routing swapping in.
    let curved = ParamLane {
        param: 3,
        lane: AutoLane::new(vec![
            AutoPoint {
                at: 0,
                value: 0.0,
                shape: Shape::Curve(lumen_core::curve::Curve::S_CURVE),
            },
            AutoPoint {
                at: 40_000,
                value: 1.0,
                shape: Shape::Step,
            },
            AutoPoint {
                at: 41_000,
                value: 0.2,
                shape: Shape::Linear,
            },
            AutoPoint {
                at: 90_000,
                value: 0.9,
                shape: Shape::Linear,
            },
        ])
        .unwrap(),
    };
    let node = |extra: u64| {
        let mut node = track(
            1,
            vec![
                Device::Hosted {
                    host: 21,
                    latency: 0,
                },
                Device::Hosted {
                    host: 22 + extra,
                    latency: 0,
                },
            ],
        );
        node.automation = Some(NodeAutomation {
            plugins: vec![
                (21, curved.clone()),
                (22 + extra, step_lane(3, 20_000, 0.1, 0.8)),
            ],
            ..Default::default()
        });
        node
    };
    let mut rig = rig(1, RATE, &routing_of(vec![node(0)]));
    rig.attach(21, FakeEffect::new(Mode::Follow(3), 0).0, 0);
    rig.attach(22, FakeEffect::new(Mode::Follow(3), 0).0, 0);
    let mut data = vec![0.0; 2 * 128];
    for block in 0..2_000_u32 {
        match block {
            0 => rig.play(),
            300 => rig.transport.send(Command::Stop).unwrap(),
            400 => rig.play(),
            700 => rig
                .transport
                .send(Command::Locate(Ticks::quarters(2)))
                .unwrap(),
            1_000 => assert!(rig.routing.set(&routing_of(vec![node(0)])).unwrap()),
            _ => {}
        }
        let ((), heap) = count_rt_violations(|| rig.engine.process(RATE, &mut data, 2, 128));
        assert_eq!(heap, 0, "heap operation in process() at block {block}");
        rig.transport.collect_garbage();
        rig.routing.collect_garbage();
    }
}

#[test]
fn a_node_without_a_lane_for_a_plugin_gives_it_no_parameters() {
    let r = ramp();
    let mut rig = rig(1, RATE, &routing_of(vec![track(1, hosted(7, 0))]));
    let (effect, _) = FakeEffect::new(Mode::Follow(3), 0);
    rig.attach(7, effect, 0);
    rig.play();
    let out = rig.run(8_000, 128);
    assert_matches(&out, 0, 3_000, "untouched", |n| r[n]);
}
