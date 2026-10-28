//! Rendering a whole source without a device (M7b slice E4, ADR-0096): the
//! mixer is played from a place on the timeline as fast as it goes, and what
//! reaches the sink is what playback would have played there: no fade-in at
//! the start, delay compensation followed, a tail that is left off where it
//! is quiet.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use lumen_core::midi::Event;
use lumen_core::time::Ticks;
use lumen_devices::{Guard, Hosted, ParamBlock, Refused, Reply, Status};
use lumen_engine::{
    Clip, ClipAudio, Command, Destination, Device, Ended, InstrumentSpec, Layered, MidiClip,
    MidiNote, MidiTrack, NodeSpec, Routing, SourceRender, Target, Timeline, clip_player,
    control_list, engine, midi_player, mixer, note_list, render_source,
};

const RATE: u32 = 48_000;
const MASTER: u64 = 100;
const HOST: u64 = 7;

#[derive(Clone, Copy)]
enum Mode {
    /// Half the input, delayed by the latency.
    Halve,
    /// The input plus a feedback of its own output: it rings on.
    Feedback,
    /// Never answers in time.
    Late,
}

struct FakeEffect {
    mode: Mode,
    latency: u32,
    ring: Vec<f32>,
    at: usize,
    last: [f32; 2],
    out: [Vec<f32>; 2],
}

impl FakeEffect {
    fn new(mode: Mode, latency: u32) -> Box<Self> {
        Box::new(Self {
            mode,
            latency,
            ring: vec![0.0; 2 * latency.max(1) as usize],
            at: 0,
            last: [0.0; 2],
            out: [Vec::new(), Vec::new()],
        })
    }
}

impl Hosted for FakeEffect {
    fn submit(
        &mut self,
        frames: usize,
        input: &[&[f32]],
        _: &[Event],
        _: &ParamBlock,
    ) -> Result<(), Refused> {
        let depth = self.latency as usize;
        for (channel, out) in self.out.iter_mut().enumerate() {
            out.clear();
            for &sample in &input[channel][..frames] {
                match self.mode {
                    Mode::Feedback => {
                        self.last[channel] = sample + 0.9998 * self.last[channel];
                        out.push(self.last[channel]);
                    }
                    _ => out.push(sample * 0.5),
                }
            }
            if depth > 0 {
                // Delay the output by the latency.
                for (k, sample) in out.iter_mut().enumerate() {
                    let slot = channel * depth + (self.at + k) % depth;
                    *sample = std::mem::replace(&mut self.ring[slot], *sample);
                }
            }
        }
        self.at = (self.at + frames) % depth.max(1);
        Ok(())
    }

    fn collect(&mut self, _: Instant, left: &mut [f32], right: &mut [f32]) -> Reply {
        if matches!(self.mode, Mode::Late) {
            return Reply::Late;
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
        self.ring.fill(0.0);
        self.last = [0.0; 2];
    }
}

fn ramp() -> Vec<f32> {
    (0..96_000)
        .map(|i| ((i % 997) as f32 - 498.0) / 1e3)
        .collect()
}

type Rig = (
    lumen_engine::Mixer<lumen_engine::ClipPlayer>,
    lumen_engine::ClipsHandle,
    lumen_engine::MixerHandle,
);

/// A track playing the ramp from the start, behind these devices.
fn rig(devices: Vec<Device>) -> Rig {
    let samples = ramp();
    let audio = Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![samples.clone(), samples],
    });
    let (mut clips, player) = clip_player();
    clips
        .set(vec![Clip {
            envelope: Default::default(),
            id: 1,
            track: 1,
            start: Ticks::ZERO,
            offset: 0,
            length: audio.frames() as i64,
            gain: 1.0,
            audio,
            shape: Default::default(),
        }])
        .unwrap();
    let (mut routing, source) = mixer(player);
    let track = NodeSpec {
        devices,
        ..NodeSpec::unity(1, Destination::Node(MASTER))
    };
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(MASTER, Destination::Hardware(0)), track],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    (source, clips, routing)
}

struct Rendered {
    left: Vec<f32>,
    right: Vec<f32>,
    ended: Ended,
}

fn render(
    devices: Vec<Device>,
    effect: Option<Box<FakeEffect>>,
    from: i64,
    frames: u64,
    most_tail: u64,
    cancel: &AtomicBool,
) -> Rendered {
    let (mut source, _clips, mut routing) = rig(devices);
    let status: Option<Arc<Status>> = effect.map(|effect| {
        let latency = effect.latency;
        let guard = Guard::new(effect, latency);
        let status = guard.status();
        assert!(routing.attach_host(HOST, guard).is_ok());
        status
    });
    let timeline = Timeline::default();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let ended = render_source(
        &mut source,
        SourceRender {
            timeline: &timeline,
            sample_rate: RATE,
            from,
            frames,
            most_tail,
            block: 256,
            wait: Duration::from_secs(1),
        },
        cancel,
        &mut || status.as_ref().is_some_and(|s| s.late_blocks() > 0),
        &mut |_| {},
        &mut |l, r| {
            left.extend_from_slice(l);
            right.extend_from_slice(r);
        },
    );
    Rendered { left, right, ended }
}

fn go(devices: Vec<Device>, effect: Option<Box<FakeEffect>>, from: i64, frames: u64) -> Rendered {
    render(devices, effect, from, frames, 0, &AtomicBool::new(false))
}

fn assert_ramp(r: &Rendered, from: usize, scale: f32) {
    let ramp = ramp();
    assert!(!r.left.is_empty());
    for (k, (&l, &rt)) in r.left.iter().zip(&r.right).enumerate() {
        let want = ramp[from + k] * scale;
        assert!(
            (l - want).abs() < 1e-5 && (rt - want).abs() < 1e-5,
            "frame {k} is {l} / {rt}, expected {want}"
        );
    }
}

#[test]
fn a_render_from_the_middle_of_a_clip_has_no_fade_in() {
    let r = go(Vec::new(), None, 30_000, 5_000);
    assert_eq!(r.ended, Ended::Done);
    assert_eq!(r.left.len(), 5_000);
    assert_ramp(&r, 30_000, 1.0);
}

#[test]
fn a_render_from_the_start_is_the_clip() {
    let r = go(Vec::new(), None, 0, 5_000);
    assert_eq!(r.left.len(), 5_000);
    // The clip's own 2 ms edge fade is its start; after it, the audio.
    assert_ramp(
        &Rendered {
            left: r.left[200..].to_vec(),
            right: r.right[200..].to_vec(),
            ended: r.ended,
        },
        200,
        1.0,
    );
}

#[test]
fn delay_compensation_is_followed_and_nothing_fades_in() {
    let effect = FakeEffect::new(Mode::Halve, 96);
    let devices = vec![Device::Hosted {
        host: HOST,
        latency: 96,
    }];
    let r = go(devices, Some(effect), 30_000, 5_000);
    assert_eq!(r.ended, Ended::Done);
    assert_eq!(r.left.len(), 5_000);
    assert_ramp(&r, 30_000, 0.5);
}

#[test]
fn cancelling_ends_the_render() {
    let cancel = AtomicBool::new(true);
    let r = render(Vec::new(), None, 0, 5_000, 0, &cancel);
    assert_eq!(r.ended, Ended::Cancelled);
    assert!(r.left.is_empty());
}

#[test]
fn a_host_that_misses_a_block_ends_the_render() {
    let effect = FakeEffect::new(Mode::Late, 0);
    let devices = vec![Device::Hosted {
        host: HOST,
        latency: 0,
    }];
    let r = go(devices, Some(effect), 30_000, 5_000);
    assert_eq!(r.ended, Ended::Failed);
    assert!(r.left.len() < 5_000);
}

#[test]
fn a_tail_rings_until_it_is_quiet_and_no_longer() {
    let effect = FakeEffect::new(Mode::Feedback, 0);
    let devices = vec![Device::Hosted {
        host: HOST,
        latency: 0,
    }];
    let most = 10 * u64::from(RATE);
    // Past the end of the clip (96,000 frames), so the input has stopped.
    let r = render(
        devices.clone(),
        Some(effect),
        90_000,
        6_000,
        most,
        &AtomicBool::new(false),
    );
    assert_eq!(r.ended, Ended::Done);
    let tail = r.left.len() - 6_000;
    assert!(tail > 20_000, "it rang on: {tail}");
    assert!(tail < 2 * RATE as usize, "and was cut where quiet: {tail}");
    let last = r.left.last().copied().unwrap();
    let right = r.right.last().copied().unwrap();
    assert!(
        last.abs() > 3e-5 || right.abs() > 3e-5,
        "the end is not silence"
    );

    // Without a tail, only the span.
    let again = render(
        devices,
        Some(FakeEffect::new(Mode::Feedback, 0)),
        90_000,
        6_000,
        0,
        &AtomicBool::new(false),
    );
    assert_eq!(again.left.len(), 6_000);
    assert_eq!(&again.left[..], &r.left[..6_000]);
}

/// A synth track with one note, half a second in.
fn synth_mixer() -> (
    lumen_engine::Mixer<Layered<lumen_engine::ClipPlayer, lumen_engine::MidiPlayer>>,
    lumen_engine::MidiHandle,
    lumen_engine::ClipsHandle,
) {
    let track = MidiTrack::new(
        1,
        Target::Instrument,
        Some(InstrumentSpec {
            kind: lumen_devices::synth::KIND,
            preset: 0,
            instrument: Box::new(lumen_devices::Synth::new(lumen_devices::Preset::Poly)),
        }),
        vec![MidiClip {
            id: 1,
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
            period: None,
            seams: Default::default(),
            notes: note_list(vec![MidiNote {
                start: Ticks::quarters(1) + Ticks(77),
                length: Ticks::quarters(1),
                pitch: 60,
                velocity: 100,
            }]),
            controls: control_list(Vec::new()),
        }],
    );
    let (clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(vec![track]).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    routing
        .set(&Routing {
            nodes: vec![
                NodeSpec::unity(MASTER, Destination::Hardware(0)),
                NodeSpec::unity(1, Destination::Node(MASTER)),
            ],
            cue: None,
            measure: None,
        })
        .unwrap();
    // The mixer takes the routing up at the first locate; keep the handles.
    let _ = routing;
    (source, midi, clips)
}

#[test]
fn an_instrument_renders_as_playback_plays_it() {
    let timeline = Timeline::default();
    let frames = 40_000;

    let (mut source, _midi, _clips) = synth_mixer();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let ended = render_source(
        &mut source,
        SourceRender {
            timeline: &timeline,
            sample_rate: RATE,
            from: 0,
            frames,
            most_tail: 0,
            block: 256,
            wait: Duration::from_secs(1),
        },
        &AtomicBool::new(false),
        &mut || false,
        &mut |_| {},
        &mut |l, r| {
            left.extend_from_slice(l);
            right.extend_from_slice(r);
        },
    );
    assert_eq!(ended, Ended::Done);
    assert_eq!(left.len(), frames as usize);
    let note = timeline.sample_at(Ticks::quarters(1) + Ticks(77), RATE) as usize;
    let first = left.iter().position(|v| v.abs() > 0.0).expect("it sounds");
    assert_eq!(first, note, "the first sample of sound is the note's");

    // The engine playing the same project from the start: the same audio,
    // past its 2 ms play fade-in (the note comes much later).
    let (source, _midi, _clips) = synth_mixer();
    let (mut transport, mut engine) = engine(timeline, source, RATE);
    transport.send(Command::Play).unwrap();
    let (mut live, mut live_right) = (Vec::new(), Vec::new());
    let mut data = vec![0.0; 2 * 128];
    while live.len() < frames as usize {
        engine.process(RATE, &mut data, 2, 128);
        live.extend_from_slice(&data[..128]);
        live_right.extend_from_slice(&data[128..]);
    }
    assert_eq!(&live[..frames as usize], &left[..]);
    assert_eq!(&live_right[..frames as usize], &right[..]);
}
