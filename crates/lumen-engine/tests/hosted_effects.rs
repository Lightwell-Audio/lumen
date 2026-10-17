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

use lumen_core::midi::Event;
use lumen_core::time::Ticks;
use lumen_devices::{Guard, Hosted, Refused, Reply, Status};
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Destination, Device, Engine, EngineHandle,
    Mixer, MixerHandle, NodeSpec, Routing, Timeline, clip_player, engine, mixer,
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
    /// Halves, but a worker takes this long first (each block is counted in
    /// `overlap`).
    Slow(Duration, Arc<Overlap>),
    /// Halves and adds a level of its own (the bits of an `f32`), as an amp
    /// with a long reverb sounds on after its input has stopped.
    Hum(Arc<AtomicU32>),
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
        });
        (effect, submits)
    }
}

impl Hosted for FakeEffect {
    fn submit(&mut self, frames: usize, input: &[&[f32]], _: &[Event]) -> Result<(), Refused> {
        self.submits.fetch_add(1, Ordering::Relaxed);
        let depth = self.latency as usize;
        for (channel, out) in self.out.iter_mut().enumerate() {
            out.clear();
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
        }
    }

    fn ended(&self) -> bool {
        false
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

// Keeps the shared log type in step with the instrument tests.
#[allow(dead_code)]
type Log = Arc<Mutex<Vec<u8>>>;
