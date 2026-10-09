//! Golden renders of the mixer graph (M4 slice 1): groups, returns, sends,
//! mute, polarity, pan laws and device output pairs, and changes while
//! playing that glide or crossfade instead of clicking.

use std::sync::Arc;

use lumen_core::pan::PanLaw;
use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, ClipsHandle, Command, Destination, Engine, EngineHandle, Mixer,
    MixerHandle, NodeSpec, Routing, RoutingError, SendSpec, Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
/// Past the 2 ms play fade-in and any 20 ms glide.
const SETTLED: usize = 2_000;

type Player = Engine<Mixer<ClipPlayer>>;

struct Rig {
    transport: EngineHandle,
    clips: ClipsHandle,
    routing: MixerHandle,
    engine: Player,
    channels: usize,
}

/// Steady audio at `level` on both sides (or distinct sides).
fn steady(left: f32, right: f32) -> Arc<ClipAudio> {
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![vec![left; 480_000], vec![right; 480_000]],
    })
}

/// Audio whose every sample differs, to catch any misplaced frame.
fn ramp() -> Arc<ClipAudio> {
    let left: Vec<f32> = (0..480_000)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect();
    let right = left.iter().map(|v| -v * 0.5).collect();
    Arc::new(ClipAudio {
        sample_rate: RATE,
        channels: vec![left, right],
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

fn master() -> NodeSpec {
    NodeSpec::unity(MASTER, Destination::Hardware(0))
}

fn to_master(id: u64) -> NodeSpec {
    NodeSpec::unity(id, Destination::Node(MASTER))
}

fn rig(clips: Vec<Clip>, routing: Routing, channels: usize) -> Rig {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing_handle, source) = mixer(player);
    assert!(routing_handle.set(&routing).unwrap());
    let (mut transport, engine) = engine(Timeline::default(), source, RATE);
    transport.send(Command::Play).unwrap();
    Rig {
        transport,
        clips: clips_handle,
        routing: routing_handle,
        engine,
        channels,
    }
}

impl Rig {
    /// Renders `frames` in blocks of `block`; returns each frame's
    /// channels.
    fn run(&mut self, frames: usize, block: usize) -> Vec<Vec<f32>> {
        let channels = self.channels;
        let mut data = vec![0.0; channels * block];
        let mut out = Vec::with_capacity(frames);
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            self.engine
                .process(RATE, &mut data[..channels * n], channels, n);
            out.extend((0..n).map(|i| (0..channels).map(|c| data[c * n + i]).collect()));
            done += n;
        }
        out
    }

    /// The last frame after settling.
    fn settled(&mut self) -> Vec<f32> {
        self.run(SETTLED, 128).pop().unwrap()
    }

    fn reroute(&mut self, routing: &Routing) {
        self.routing.collect_garbage();
        assert!(self.routing.set(routing).unwrap());
    }
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}

/// The largest change from one frame to the next, on any channel.
fn steepest(out: &[Vec<f32>]) -> f32 {
    out.windows(2)
        .flat_map(|w| w[0].iter().zip(&w[1]).map(|(a, b)| (b - a).abs()))
        .fold(0.0, f32::max)
}

#[test]
fn a_group_equals_the_sum_of_its_children() {
    let (a, b) = (ramp(), steady(0.1, 0.2));
    let clips = vec![clip(1, 1, &a), clip(2, 2, &b)];
    let flat = Routing {
        nodes: vec![master(), to_master(1), to_master(2)],
    };
    let grouped = Routing {
        nodes: vec![
            master(),
            to_master(50),
            NodeSpec::unity(1, Destination::Node(50)),
            NodeSpec::unity(2, Destination::Node(50)),
        ],
    };
    // Nested: a group inside a group.
    let nested = Routing {
        nodes: vec![
            master(),
            to_master(50),
            NodeSpec::unity(51, Destination::Node(50)),
            NodeSpec::unity(1, Destination::Node(51)),
            NodeSpec::unity(2, Destination::Node(50)),
        ],
    };
    let want = rig(clips.clone(), flat, 2).run(5_000, 128);
    for routing in [grouped, nested] {
        let got = rig(clips.clone(), routing, 2).run(5_000, 128);
        assert_eq!(got, want);
    }
    // And the sum is the two clips added together.
    let one = rig(
        vec![clip(1, 1, &a)],
        Routing {
            nodes: vec![master(), to_master(1)],
        },
        2,
    )
    .run(5_000, 128);
    assert!(close(
        &want[3_000],
        &[one[3_000][0] + 0.1, one[3_000][1] + 0.2]
    ));
}

#[test]
fn a_group_level_applies_to_all_its_children() {
    let audio = steady(0.5, 0.5);
    let mut group = to_master(50);
    group.volume = 0.5;
    let mut r = rig(
        vec![clip(1, 1, &audio), clip(2, 2, &audio)],
        Routing {
            nodes: vec![
                master(),
                group,
                NodeSpec::unity(1, Destination::Node(50)),
                NodeSpec::unity(2, Destination::Node(50)),
            ],
        },
        2,
    );
    assert!(close(&r.settled(), &[0.5, 0.5]));
}

#[test]
fn a_muted_track_is_silent_and_so_are_its_sends() {
    let audio = steady(0.5, 0.5);
    let mut track = to_master(1);
    track.audible = false;
    track.sends_audible = false;
    track.sends.push(SendSpec {
        to: 60,
        gain: 1.0,
        pre_fader: true,
    });
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master(), to_master(60), track],
        },
        2,
    );
    assert!(r.run(5_000, 128).iter().all(|f| f == &[0.0, 0.0]));
}

#[test]
fn a_silent_output_can_keep_its_sends_wet_only() {
    // M4 slice 4: a soloed return plays the sends into it, not the
    // tracks' own outputs.
    let audio = steady(0.25, 0.125);
    let wet = |audible| {
        let mut track = to_master(1);
        track.audible = audible;
        track.sends.push(SendSpec {
            to: 60,
            gain: 1.0,
            pre_fader: false,
        });
        Routing {
            nodes: vec![master(), to_master(60), track],
        }
    };
    let mut r = rig(vec![clip(1, 1, &audio)], wet(false), 2);
    assert!(close(&r.settled(), &[0.25, 0.125]), "the send alone");
    // Opening the output glides: no click.
    r.reroute(&wet(true));
    let out = r.run(SETTLED, 128);
    assert!(close(out.last().unwrap(), &[0.5, 0.25]));
    assert!(steepest(&out) < 0.01, "steepest step {}", steepest(&out));
}

#[test]
fn a_send_at_0_db_doubles_into_a_return() {
    let audio = steady(0.25, 0.125);
    let mut track = to_master(1);
    track.sends.push(SendSpec {
        to: 60,
        gain: 1.0,
        pre_fader: false,
    });
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master(), to_master(60), track],
        },
        2,
    );
    assert!(close(&r.settled(), &[0.5, 0.25]));
}

#[test]
fn pre_fader_sends_ignore_the_fader_and_post_fader_sends_follow_it() {
    let audio = steady(0.5, 0.5);
    let send = |pre_fader| {
        let mut track = NodeSpec::unity(1, Destination::None);
        track.volume = 0.25;
        track.pan = -1.0;
        track.sends.push(SendSpec {
            to: 60,
            gain: 1.0,
            pre_fader,
        });
        let mut r = rig(
            vec![clip(1, 1, &audio)],
            Routing {
                nodes: vec![master(), to_master(60), track],
            },
            2,
        );
        r.settled()
    };
    assert!(close(&send(true), &[0.5, 0.5]));
    // After volume and pan: hard left with the balance law.
    assert!(close(&send(false), &[0.125, 0.0]));
}

#[test]
fn the_default_pan_law_drops_the_centre_by_3_db() {
    let audio = steady(0.5, 0.5);
    let mut track = to_master(1);
    track.law = PanLaw::default();
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master(), track],
        },
        2,
    );
    let half_power = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(close(&r.settled(), &[half_power, half_power]));
}

#[test]
fn polarity_inverts_and_trim_scales() {
    let audio = steady(0.5, -0.25);
    let mut track = to_master(1);
    track.invert = true;
    track.trim = 0.5;
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master(), track],
        },
        2,
    );
    assert!(close(&r.settled(), &[-0.25, 0.125]));
}

#[test]
fn tracks_reach_the_device_output_pair_they_are_routed_to() {
    let audio = steady(0.5, 0.25);
    let routing = Routing {
        nodes: vec![
            master(),
            to_master(1),
            NodeSpec::unity(2, Destination::Hardware(1)),
            NodeSpec::unity(3, Destination::Hardware(7)),
        ],
    };
    let clips = vec![clip(1, 1, &audio), clip(2, 2, &audio), clip(3, 3, &audio)];
    let mut four = rig(clips.clone(), routing.clone(), 4);
    assert!(close(&four.settled(), &[0.5, 0.25, 0.5, 0.25]));
    // A pair the device does not have is silent; a mono device gets the
    // left side.
    let mut mono = rig(clips, routing, 1);
    assert!(close(&mono.settled(), &[0.5]));
}

#[test]
fn a_track_missing_from_the_routing_is_not_heard() {
    let audio = steady(0.5, 0.5);
    let mut r = rig(
        vec![clip(1, 1, &audio), clip(2, 9, &audio)],
        Routing {
            nodes: vec![master(), to_master(1)],
        },
        2,
    );
    assert!(close(&r.settled(), &[0.5, 0.5]));
}

#[test]
fn volume_pan_mute_and_polarity_changes_glide_instead_of_stepping() {
    // Owner (M3): dragging volume or pan crackled during playback, and
    // muting clicked. The mixer's levels glide as clip levels did.
    let audio = steady(0.5, 0.5);
    let routing = |volume: f32, pan: f32, audible: bool, invert: bool| Routing {
        nodes: vec![
            master(),
            NodeSpec {
                volume,
                pan,
                audible,
                sends_audible: audible,
                invert,
                law: PanLaw::default(),
                ..to_master(1)
            },
        ],
    };
    let mut r = rig(vec![clip(1, 1, &audio)], routing(1.0, 0.0, true, false), 2);
    let mut out = r.run(1_000, 128);
    // A drag: a new routing every 16 ms (one screen frame).
    for (volume, pan) in [(0.8, 0.0), (0.6, -0.3), (0.6, -0.9), (1.0, 0.5)] {
        r.reroute(&routing(volume, pan, true, false));
        out.extend(r.run(768, 128));
    }
    r.reroute(&routing(1.0, 0.0, false, false));
    out.extend(r.run(2_000, 128));
    assert_eq!(out.last().unwrap(), &[0.0, 0.0], "muted reaches silence");
    r.reroute(&routing(1.0, 0.0, true, true));
    out.extend(r.run(2_000, 128));
    let centre = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(close(out.last().unwrap(), &[-centre, -centre]), "inverted");
    // Full scale (−1 to +1 of the gain) in 20 ms, on audio at 0.5.
    let max_step = 2.0 * 0.5 / (RATE as f32 * 0.02) + 1e-6;
    assert!(
        steepest(&out[200..]) <= max_step,
        "{}",
        steepest(&out[200..])
    );
}

#[test]
fn rerouting_while_playing_crossfades() {
    // A track moves from one device pair to another: the old path fades
    // out over 5 ms while the new one fades in.
    let audio = steady(0.5, 0.5);
    let pair = |pair| Routing {
        nodes: vec![NodeSpec::unity(1, Destination::Hardware(pair))],
    };
    let mut r = rig(vec![clip(1, 1, &audio)], pair(0), 4);
    let mut out = r.run(1_000, 128);
    r.reroute(&pair(1));
    out.extend(r.run(1_000, 128));
    assert!(close(out.last().unwrap(), &[0.0, 0.0, 0.5, 0.5]));
    // 0.5 over 240 frames.
    let max_step = 0.5 / 240.0 + 1e-6;
    assert!(
        steepest(&out[200..]) <= max_step,
        "{}",
        steepest(&out[200..])
    );
    // The total never dips: in and out add up to one.
    for frame in &out[200..] {
        assert!((frame.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{frame:?}");
    }
}

#[test]
fn a_track_that_joins_the_routing_while_playing_fades_in() {
    let audio = steady(0.5, 0.5);
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master()],
        },
        2,
    );
    let mut out = r.run(1_000, 128);
    r.reroute(&Routing {
        nodes: vec![master(), to_master(1)],
    });
    out.extend(r.run(3_000, 128));
    assert!(close(out.last().unwrap(), &[0.5, 0.5]));
    // A new shape: the plan crossfades in over 5 ms, under the clip's own
    // 20 ms fade-in.
    let max_step = 0.5 / 240.0 + 1e-6;
    assert!(steepest(&out) <= max_step, "{}", steepest(&out));
}

#[test]
fn a_routed_mix_is_the_same_at_every_block_size() {
    let audio = ramp();
    let clips: Vec<Clip> = (0..16).map(|i| clip(i, i, &audio)).collect();
    let routing = lumen_engine::testing::routed(16, 0);
    let render = |block: usize| {
        let mut r = rig(clips.clone(), routing.clone(), 2);
        let mut out = r.run(3_000, block);
        // Levels change and a track moves partway (a glide and a plan
        // crossfade), at the same frame whatever the block size.
        r.reroute(&lumen_engine::testing::routed(16, 1));
        out.extend(r.run(3_000, block));
        out
    };
    let want = render(128);
    for block in [1, 37, 256, 300, 1_024] {
        let got = render(block);
        assert!(
            want.iter().zip(&got).all(|(a, b)| close(a, b)),
            "block {block}"
        );
    }
}

#[test]
fn replaced_plans_come_back_to_be_freed() {
    let audio = steady(0.5, 0.5);
    let routing = |volume| Routing {
        nodes: vec![NodeSpec {
            volume,
            ..NodeSpec::unity(1, Destination::Hardware(0))
        }],
    };
    let mut r = rig(vec![clip(1, 1, &audio)], routing(1.0), 2);
    r.run(256, 128);
    for volume in [0.9, 0.8, 0.7] {
        assert!(r.routing.set(&routing(volume)).unwrap());
    }
    r.run(256, 128);
    // The empty plan the mixer starts with, the first routing, and the
    // two that queued up and never played.
    assert_eq!(r.routing.collect_garbage(), 4);
    assert!(close(&r.settled(), &[0.35, 0.35]));
}

#[test]
fn a_routing_that_cannot_play_is_refused_and_the_old_one_plays_on() {
    let audio = steady(0.5, 0.5);
    let mut r = rig(
        vec![clip(1, 1, &audio)],
        Routing {
            nodes: vec![master(), to_master(1)],
        },
        2,
    );
    let looped = Routing {
        nodes: vec![NodeSpec::unity(MASTER, Destination::Node(1)), to_master(1)],
    };
    assert_eq!(r.routing.set(&looped), Err(RoutingError::Cycle));
    assert!(close(&r.settled(), &[0.5, 0.5]));
    let _ = &r.clips;
    let _ = &r.transport;
}
