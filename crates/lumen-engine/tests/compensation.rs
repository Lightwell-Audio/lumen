//! Delay compensation (M4 slice 6, ADR-0041), with the synthetic latent
//! device: parallel paths line up sample for sample, sends and sidechains
//! included, and what is heard lines up with the playhead (read ahead),
//! across loops, jumps and latency changes.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Compensation, Destination, Device, Engine,
    EngineHandle, Mixer, MixerHandle, NodeSpec, Region, Routing, SendSpec, SidechainSpec, Tap,
    Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
/// Frames of the 2 ms blend that starts playback and follows each jump.
const BLEND: usize = 96;

struct Rig {
    _clips: ClipsHandle,
    transport: EngineHandle,
    routing: MixerHandle,
    engine: Engine<Mixer<ClipPlayer>>,
}

/// Audio whose every sample differs, to catch any misplaced frame.
fn ramp() -> Vec<f32> {
    (0..480_000)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect()
}

fn audio(samples: Vec<f32>) -> Arc<ClipAudio> {
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

fn latent(frames: u32) -> Vec<Device> {
    vec![Device::Delay {
        frames,
        listen: false,
    }]
}

/// A device that hears the node's sidechain.
fn listener() -> Vec<Device> {
    vec![Device::Delay {
        frames: 0,
        listen: true,
    }]
}

fn node(id: u64, output: Destination) -> NodeSpec {
    NodeSpec::unity(id, output)
}

fn to_master(id: u64) -> NodeSpec {
    node(id, Destination::Node(MASTER))
}

fn routing_of(nodes: Vec<NodeSpec>) -> Routing {
    let mut all = vec![node(MASTER, Destination::Hardware(0))];
    all.extend(nodes);
    Routing {
        nodes: all,
        cue: None,
        measure: None,
    }
}

fn rig(clips: Vec<Clip>, routing: &Routing) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(routing).unwrap());
    let (transport, engine) = engine(Timeline::default(), source, RATE);
    Rig {
        _clips: clips_handle,
        transport,
        routing: routing_handle,
        engine,
    }
}

impl Rig {
    fn play(&mut self) {
        self.transport.send(Command::Play).unwrap();
    }

    /// Renders `frames` in blocks of `block`; returns the left channel.
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
        out
    }

    fn reroute(&mut self, routing: &Routing) {
        self.routing.collect_garbage();
        assert!(self.routing.set(routing).unwrap());
    }

    fn routing_compensation(&self) -> u32 {
        self.routing.compensation().frames
    }

    fn playhead(&self) -> i64 {
        self.transport.transport().sample
    }
}

/// The loudest sample.
fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| f32::max(m, s.abs()))
}

/// The largest change from one sample to the next.
fn steepest(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn what_is_heard_lines_up_with_the_playhead() {
    // A 500-frame device on the only track: the engine renders 500 frames
    // ahead, so the playhead waits at the start for them, then what is
    // heard at each playhead position is the audio at that position.
    let samples = ramp();
    let mut track = to_master(1);
    track.devices = latent(500);
    let routing = routing_of(vec![track]);
    let mut r = rig(vec![clip(1, 1, &audio(samples.clone()))], &routing);
    assert_eq!(
        r.routing.compensation(),
        &Compensation {
            frames: 500,
            path: vec![(1, 500)],
        }
    );
    r.play();
    let out = r.run(128, 128);
    assert_eq!(r.playhead(), 0, "the playhead waits through the pre-roll");
    assert_eq!(peak(&out), 0.0);
    let out = [out, r.run(9_872, 128)].concat();
    assert_eq!(r.playhead(), 10_000 - 500);
    for k in 500 + BLEND..10_000 {
        assert_eq!(out[k], samples[k - 500], "frame {k}");
    }
}

#[test]
fn parallel_paths_line_up_sample_for_sample() {
    // Track 1 goes through a 37-frame device and a group; track 2 plays
    // the same audio upside down straight into the master. Lined up, they
    // cancel exactly.
    let samples = ramp();
    let a = audio(samples);
    let mut slow = node(1, Destination::Node(50));
    slow.devices = latent(37);
    let mut upside_down = to_master(2);
    upside_down.invert = true;
    let mut r = rig(
        vec![clip(1, 1, &a), clip(2, 2, &a)],
        &routing_of(vec![to_master(50), slow, upside_down]),
    );
    assert_eq!(r.routing.compensation().frames, 37);
    r.play();
    let out = r.run(20_000, 100);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));

    // The same with the device on the group instead.
    let mut upside_down = to_master(2);
    upside_down.invert = true;
    let mut group = to_master(50);
    group.devices = latent(37);
    r.reroute(&routing_of(vec![
        group,
        node(1, Destination::Node(50)),
        upside_down,
    ]));
    let out = r.run(20_000, 100);
    assert!(peak(&out[10_000..]) < 1e-6, "{}", peak(&out[10_000..]));
}

#[test]
fn sends_line_up_at_the_return() {
    // Track 1 (50 frames) and track 2, upside down, both send to a return
    // and nowhere else: the return hears them line up and cancel.
    let a = audio(ramp());
    let send = SendSpec {
        to: 60,
        gain: 1.0,
        pre_fader: false,
    };
    let mut slow = node(1, Destination::None);
    slow.devices = latent(50);
    slow.sends.push(send);
    let mut upside_down = node(2, Destination::None);
    upside_down.invert = true;
    upside_down.sends.push(SendSpec {
        pre_fader: true,
        ..send
    });
    let routing = routing_of(vec![to_master(60), slow, upside_down]);
    let mut r = rig(vec![clip(1, 1, &a), clip(2, 2, &a)], &routing);
    r.play();
    let out = r.run(20_000, 128);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));
}

/// Track 2 plays the ramp through `source_latency` frames into the
/// sidechain of track 3 (taken at `tap`), which plays the ramp upside down
/// and listens to its sidechain. Returns what the master hears.
fn keyed(tap: Tap, source_latency: u32, source_heard: bool) -> Vec<f32> {
    let a = audio(ramp());
    let mut source = node(2, Destination::None);
    source.devices = latent(source_latency);
    source.audible = source_heard;
    source.sends_audible = source_heard;
    let mut keyed = to_master(3);
    keyed.invert = true;
    keyed.devices = listener();
    keyed.sidechains.push(SidechainSpec { from: 2, tap });
    let routing = routing_of(vec![keyed, source]);
    let mut r = rig(vec![clip(2, 2, &a), clip(3, 3, &a)], &routing);
    r.play();
    r.run(20_000, 128)
}

#[test]
fn a_sidechain_lines_up_with_the_input_it_keys() {
    // Post-FX, the sidechain arrives 30 frames late, so the listening
    // track's own audio waits for it: they cancel.
    let out = keyed(Tap::PostFx, 30, true);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));
    // Pre-FX, it leaves before the source's devices: no wait at all.
    let out = keyed(Tap::PreFx, 30, true);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));
}

#[test]
fn fx_taps_ignore_mute_and_the_post_mixer_tap_follows_it() {
    // A muted source still keys the listener before its fader (the ghost
    // kick)...
    for tap in [Tap::PreFx, Tap::PostFx] {
        let out = keyed(tap, 30, false);
        assert!(peak(&out) < 1e-6, "{tap:?}: {}", peak(&out));
    }
    // ...but after it, mute silences the sidechain, and only the
    // listener's own audio is heard.
    let out = keyed(Tap::PostMixer, 30, false);
    assert!(peak(&out[1_000..]) > 0.4);
    let out = keyed(Tap::PostMixer, 30, true);
    assert!(peak(&out) < 1e-6, "{}", peak(&out));
}

#[test]
fn loops_stay_in_line_with_the_playhead() {
    // A loop one beat long (24_000 frames) and 500 frames of compensation:
    // each wrap is rendered 500 frames early and heard when the playhead
    // wraps.
    let samples = ramp();
    let mut track = to_master(1);
    track.devices = latent(500);
    let routing = routing_of(vec![track]);
    let mut r = rig(vec![clip(1, 1, &audio(samples.clone()))], &routing);
    let beat = Ticks::quarters(1);
    r.transport
        .send(Command::SetLoop(Region::new(Ticks::ZERO, beat, true)))
        .unwrap();
    r.play();
    let out = r.run(80_000, 128);
    assert_eq!(r.playhead(), (80_000 - 500) % 24_000);
    for (k, &heard) in out.iter().enumerate().skip(500) {
        let at = (k - 500) % 24_000;
        // Each wrap blends over its first 2 ms.
        if at >= BLEND {
            assert_eq!(heard, samples[at], "frame {k} (beat position {at})");
        }
    }
}

#[test]
fn a_jump_while_latent_is_heard_when_the_playhead_jumps() {
    let samples = ramp();
    let mut track = to_master(1);
    track.devices = latent(300);
    let routing = routing_of(vec![track]);
    let mut r = rig(vec![clip(1, 1, &audio(samples.clone()))], &routing);
    r.play();
    let before = r.run(2_048, 128);
    r.transport
        .send(Command::Locate(Ticks::quarters(2)))
        .unwrap();
    let after = r.run(4_096, 128);
    let target = 48_000;
    // Until the jump is heard, the old place plays on in line.
    for (k, &heard) in before.iter().enumerate().skip(300 + BLEND) {
        assert_eq!(heard, samples[k - 300]);
    }
    for (k, &heard) in after.iter().enumerate().take(300) {
        assert_eq!(
            heard,
            samples[2_048 - 300 + k],
            "frame {k} after the locate"
        );
    }
    // Then the new place, from the playhead's jump.
    for (k, &heard) in after.iter().enumerate().skip(300 + BLEND) {
        assert_eq!(
            heard,
            samples[target + k - 300],
            "frame {k} after the locate"
        );
    }
    assert_eq!(r.playhead(), target as i64 + 4_096 - 300);
}

#[test]
fn a_latency_change_while_playing_keeps_the_playhead_going_without_a_click() {
    // Steady audio; a 200-frame device arrives while playing, then leaves.
    let level = 0.5;
    let a = audio(vec![level; 480_000]);
    let plain = routing_of(vec![to_master(1)]);
    let mut slow = to_master(1);
    slow.devices = latent(200);
    let latent_routing = routing_of(vec![slow]);
    let mut r = rig(vec![clip(1, 1, &a)], &plain);
    r.play();
    let mut out = r.run(4_096, 128);
    r.reroute(&latent_routing);
    out.extend(r.run(4_096, 128));
    assert_eq!(r.playhead(), 8_192, "the playhead carries on");
    r.reroute(&plain);
    out.extend(r.run(4_096, 128));
    assert_eq!(r.playhead(), 12_288);
    // No step: the fastest change is a fade (0.5 over at least 96 frames).
    let slope = steepest(&out[1_000..]);
    assert!(slope <= level / 90.0, "{slope}");
    assert!((out.last().unwrap() - level).abs() < 1e-6);
}

#[test]
fn after_a_latency_change_what_is_heard_lines_up_again() {
    let samples = ramp();
    let a = audio(samples.clone());
    let mut slow = to_master(1);
    slow.devices = latent(200);
    let mut r = rig(vec![clip(1, 1, &a)], &routing_of(vec![to_master(1)]));
    r.play();
    let mut out = r.run(4_096, 128);
    r.reroute(&routing_of(vec![slow]));
    out.extend(r.run(8_192, 128));
    // The new plan arrives in the next block and the engine follows it a
    // block later; then the device's delay and a blend pass.
    let settled = 4_096 + 2 * 128 + 200 + BLEND;
    for (k, &heard) in out.iter().enumerate().skip(settled) {
        assert_eq!(heard, samples[k], "frame {k}");
    }
}

#[test]
fn scrubbing_is_not_heard_late_by_the_compensation() {
    // A track 50 ms late makes every path wait 50 ms while playing (ADR-0041).
    // A scrub follows the hand: what it plays comes out at once, whatever the
    // routing's delays (M6 slice 6a, the owner's C1).
    let latent_track = NodeSpec {
        devices: latent(2_400),
        ..to_master(1)
    };
    let routing = routing_of(vec![latent_track, to_master(2)]);
    let audio = audio(ramp());
    let mut rig = rig(vec![clip(1, 1, &audio), clip(2, 2, &audio)], &routing);
    assert_eq!(rig.routing_compensation(), 2_400);
    rig.transport
        .send(Command::Scrub {
            at: Ticks::quarters(1),
            style: lumen_engine::ScrubStyle::Loop {
                length: Ticks::quarters(1),
            },
        })
        .unwrap();
    let heard = rig.run(1_200, 128);
    assert!(
        peak(&heard) > 0.05,
        "nothing within 25 ms of the scrub's start: {}",
        peak(&heard)
    );
    // Playing is compensated again afterwards, exactly as it is without a
    // scrub before it.
    rig.transport.send(Command::EndScrub).unwrap();
    rig.run(4_800, 128);
    rig.play();
    let after = rig.run(9_600, 128);

    let mut fresh = self::rig(vec![clip(1, 1, &audio), clip(2, 2, &audio)], &routing);
    fresh
        .transport
        .send(Command::Locate(Ticks::quarters(1)))
        .unwrap();
    fresh.run(4_800, 128);
    fresh.play();
    let expected = fresh.run(9_600, 128);
    assert_eq!(
        after[BLEND * 2..],
        expected[BLEND * 2..],
        "the delay lines work again once playback starts"
    );
}
