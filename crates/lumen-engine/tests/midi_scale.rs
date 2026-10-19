//! How long the audio thread spends on the work that grows with a MIDI
//! project (M6 review, M7): the chase after a locate, a loop wrap or a start,
//! and the comparison of a new track list with the old one. A measurement,
//! not a gate: run it in release and read the table.
//!
//! ```text
//! cargo test --release -p lumen-engine --test midi_scale -- --ignored --nocapture
//! ```
//!
//! Every shape holds 100,000 notes. The block is 128 frames at 48 kHz, which
//! has 2,667 µs to its name.

use std::time::{Duration, Instant};

use lumen_core::midi::{Event, Msg};
use lumen_core::time::Ticks;
use lumen_devices::Instrument;
use lumen_engine::{
    Command, Destination, Engine, EngineHandle, InstrumentSpec, Layered, MidiClip, MidiControl,
    MidiHandle, MidiNote, MidiPlayer, MidiTrack, Mixer, NodeSpec, Region, Routing, Target,
    Timeline, clip_player, control_list, engine, midi_player, mixer, note_list,
};

const MASTER: u64 = 1000;
const RATE: u32 = 48_000;
const BLOCK: usize = 128;
const BUDGET_US: f64 = BLOCK as f64 / RATE as f64 * 1e6;
const Q: fn(i64) -> Ticks = Ticks::quarters;

struct Count(u32);

impl Instrument for Count {
    fn render(&mut self, _: u32, events: &[Event], _: &mut [f32], _: &mut [f32]) {
        self.0 += events.len() as u32;
    }
    fn active(&self) -> bool {
        false
    }
    fn silence(&mut self) {}
}

#[derive(Clone)]
struct Shape {
    name: &'static str,
    tracks: u64,
    notes: usize,
    controls: usize,
}

/// A sixteenth apart, one sixteenth long; the song runs `notes / 4` quarters
/// on each track.
fn notes_of(count: usize) -> Vec<MidiNote> {
    let step = Q(1).0 / 4;
    (0..count)
        .map(|i| MidiNote {
            start: Ticks(i as i64 * step),
            length: Ticks(step),
            pitch: 36 + (i % 48) as u8,
            velocity: 100,
        })
        .collect()
}

fn controls_of(count: usize, quarters: i64) -> Vec<MidiControl> {
    (0..count)
        .map(|i| MidiControl {
            at: Ticks(i as i64 * Q(quarters).0 / count as i64),
            msg: Msg::controller(0, 1 + (i % 100) as u8, (i % 128) as u8),
        })
        .collect()
}

fn clip(shape: &Shape, notes: Vec<MidiNote>) -> MidiClip {
    let quarters = shape.notes as i64 / 4 + 4;
    MidiClip {
        id: 1,
        start: Ticks::ZERO,
        length: Q(quarters),
        period: None,
        seams: Default::default(),
        notes: note_list(notes),
        controls: control_list(controls_of(shape.controls, quarters)),
    }
}

/// One clip a track, built from `lists`: each clip has an allocation of its own.
fn clips_of(shape: &Shape, lists: &[Vec<MidiNote>]) -> Vec<MidiClip> {
    (1..=shape.tracks)
        .map(|id| {
            let mut c = clip(shape, lists[(id - 1) as usize].clone());
            c.id = id;
            c
        })
        .collect()
}

/// Tracks over `clips`: clones of a clip share its notes.
fn tracks(clips: &[MidiClip]) -> Vec<MidiTrack> {
    clips
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let id = k as u64 + 1;
            let c = c.clone();
            MidiTrack::new(
                id,
                Target::Instrument,
                Some(InstrumentSpec {
                    kind: 1,
                    preset: 0,
                    instrument: Box::new(Count(0)),
                }),
                vec![c],
            )
        })
        .collect()
}

struct Rig {
    transport: EngineHandle,
    midi: MidiHandle,
    engine: Engine<Mixer<Layered<lumen_engine::ClipPlayer, MidiPlayer>>>,
    data: Vec<f32>,
}

fn rig(shape: &Shape, clips: &[MidiClip]) -> Rig {
    let (_clips, players) = clip_player();
    let (mut midi, player) = midi_player();
    midi.set(tracks(clips)).ok().unwrap();
    let (mut routing, source) = mixer(Layered(players, player));
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    nodes.extend((1..=shape.tracks).map(|id| NodeSpec::unity(id, Destination::Node(MASTER))));
    routing
        .set(&Routing {
            nodes,
            cue: None,
            measure: None,
        })
        .unwrap();
    let (transport, engine) = engine(Timeline::default(), source, RATE);
    Rig {
        transport,
        midi,
        engine,
        data: vec![0.0; 2 * BLOCK],
    }
}

impl Rig {
    fn block(&mut self) -> Duration {
        let started = Instant::now();
        self.engine.process(RATE, &mut self.data, 2, BLOCK);
        let took = started.elapsed();
        self.midi.collect_garbage();
        took
    }

    fn send(&mut self, command: Command) {
        self.transport.send(command).unwrap();
    }

    /// Plays blocks until `n` are done, so the engine has taken everything
    /// sent so far.
    fn settle(&mut self, n: usize) {
        for _ in 0..n {
            self.block();
        }
    }
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn stats(mut times: Vec<f64>) -> (f64, f64) {
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], *times.last().unwrap())
}

fn row(shape: &Shape, what: &str, times: Vec<f64>) {
    let (median, max) = stats(times);
    println!(
        "{:<28} {:<46} median {:>9.1} µs  worst {:>9.1} µs  ({:>6.1}% of a block)",
        shape.name,
        what,
        median,
        max,
        max / BUDGET_US * 100.0
    );
}

#[test]
#[ignore = "a measurement: run in release with --nocapture"]
fn what_the_chase_and_the_list_swap_cost_at_a_hundred_thousand_notes() {
    let shapes = [
        Shape {
            name: "1 track, 100,000 notes",
            tracks: 1,
            notes: 100_000,
            controls: 0,
        },
        Shape {
            name: "100 tracks, 1,000 notes",
            tracks: 100,
            notes: 1_000,
            controls: 0,
        },
        Shape {
            name: "1 track, 100,000 + 20,000 cc",
            tracks: 1,
            notes: 100_000,
            controls: 20_000,
        },
    ];
    println!("\nblock: {BLOCK} frames at {RATE} Hz = {BUDGET_US:.0} µs; run in release\n");
    for shape in shapes {
        let per_track = shape.notes;
        let lists: Vec<Vec<MidiNote>> = (0..shape.tracks).map(|_| notes_of(per_track)).collect();
        let clips = clips_of(&shape, &lists);
        let late = Q(per_track as i64 / 4 - 10);

        // Making the track list, on the UI thread (the index included).
        let mut times = Vec::new();
        for _ in 0..15 {
            let started = Instant::now();
            let made = tracks(&clips);
            times.push(us(started.elapsed()));
            drop(made);
        }
        row(&shape, "making the track list (UI thread)", times);

        // Steady playing, early in the song.
        let mut r = rig(&shape, &clips);
        r.send(Command::Play);
        r.settle(4);
        row(
            &shape,
            "steady play (blocks 5 to 2004)",
            (0..2000).map(|_| us(r.block())).collect(),
        );

        // A locate while playing: near the start, then near the end.
        for (what, to) in [
            ("locate to bar 3 while playing", Q(8)),
            (
                "locate to the middle while playing",
                Q(per_track as i64 / 8),
            ),
            ("locate to the end while playing", late),
        ] {
            let mut r = rig(&shape, &clips);
            r.send(Command::Play);
            r.settle(4);
            let mut times = Vec::new();
            for trial in 0..30 {
                r.send(Command::Locate(if trial % 2 == 0 { to } else { Q(1) }));
                let t = us(r.block());
                if trial % 2 == 0 {
                    times.push(t);
                }
                r.settle(2);
            }
            row(&shape, what, times);
        }

        // Start from a stopped song, at the end.
        let mut times = Vec::new();
        let mut r = rig(&shape, &clips);
        for _ in 0..15 {
            r.send(Command::Stop);
            r.settle(3);
            r.send(Command::Locate(late));
            r.settle(3);
            r.send(Command::Play);
            times.push(us(r.block()));
            r.settle(2);
        }
        row(&shape, "play from the end", times);

        // A loop at the end: every wrap chases from the loop start.
        let mut r = rig(&shape, &clips);
        r.send(Command::SetLoop(Region::new(late, late + Q(2), true)));
        r.send(Command::Locate(late));
        r.send(Command::Play);
        r.settle(4);
        let times: Vec<f64> = (0..1_000).map(|_| us(r.block())).collect(); // 2.7 s: two wraps
        row(
            &shape,
            "loop of 2 quarters at the end (1,000 blocks)",
            times,
        );

        // The track list swapped while the song plays at the end.
        let mut r = rig(&shape, &clips);
        r.send(Command::Locate(late));
        r.send(Command::Play);
        r.settle(8);
        let mut same = Vec::new();
        let mut equal = Vec::new();
        let mut edited = Vec::new();
        for trial in 0..15 {
            // Back to the song as it was (the last trial edited it): not timed.
            r.midi.set(tracks(&clips_of(&shape, &lists))).ok().unwrap();
            r.settle(3);
            // New allocations with the same notes (the document rebuilt them).
            let rebuilt = clips_of(&shape, &lists);
            r.midi.set(tracks(&rebuilt)).ok().unwrap();
            equal.push(us(r.block()));
            r.settle(2);
            // The same allocations again (an edit elsewhere): the pointers
            // match and nothing is compared.
            r.midi.set(tracks(&rebuilt)).ok().unwrap();
            same.push(us(r.block()));
            r.settle(2);
            // The last note of every track changed: the comparison runs to it
            // and the notes are chased again.
            let mut changed = lists.clone();
            for list in &mut changed {
                let last = list.len() - 1;
                list[last].velocity = 90 - (trial % 2) as u8;
            }
            r.midi
                .set(tracks(&clips_of(&shape, &changed)))
                .ok()
                .unwrap();
            edited.push(us(r.block()));
            r.settle(2);
        }
        row(&shape, "list swap, notes rebuilt (equal)", equal);
        row(&shape, "list swap, last note changed", edited);
        row(&shape, "list swap, same note allocations", same);
    }
}
