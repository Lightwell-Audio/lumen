//! A click detector for the built-in effects (M8, OR-4): a steady sine and pink
//! noise played through each device at each block size while an event happens
//! mid-stream, and the worst discontinuity of the output reported against the
//! same signal through a bypassed device.
//!
//! A click is a jump the signal itself could not make. The metric is the worst
//! **second difference** `|x[n] - 2x[n-1] + x[n-2]|`: for a tone well under the
//! sample rate that is nearly zero (the wave bends by `(ω)²` a frame), while a
//! step in the gain, or a device resetting its state, bends it by the step's
//! own size — so a discontinuity stands out against the signal by two orders of
//! magnitude. The first difference is reported beside it: it is what a person
//! hears as the click, but a tone moves it every frame, so it is the weaker of
//! the two measures.
//!
//! The table is printed by the ignored test `the_pop_table`, which is a
//! measurement, not a check:
//!
//! ```text
//! cargo test -p lumen-devices --test pops -- --ignored --nocapture
//! ```
//!
//! The checks that hold the fixes are the tests below it.

use lumen_core::pan::PanLaw;
use lumen_devices::params::ParamInfo;
use lumen_devices::{Block, Compressor, Effect, Key, Limiter, ParamBlock, Stereo, Utility};

const RATE: u32 = 48_000;

/// Blocks the harness plays. 256 is the engine's chunk (`CHUNK`), the most one
/// device is ever given; 333 is not a multiple of anything, played as 256 then
/// 77, which is what a device sees at any odd buffer size.
const BLOCKS: [usize; 4] = [64, 128, 256, 333];

/// Frames a run plays: long enough for every glide and ballistics to settle,
/// and for a few seconds of steady state before and after an event.
const FRAMES: usize = 24_000;

/// The frame an event lands on: past every glide from the defaults, and on a
/// multiple of 256 so it falls at a block boundary at every block size.
const EVENT: usize = 9_600;

// ---------------------------------------------------------------- the signals

/// A steady tone at −12 dBFS, off the grid so its samples land all over the
/// wave.
fn sine(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|k| 0.25 * (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin())
        .collect()
}

/// A low tone too: the worst case for a detector that follows the wave.
fn low_sine(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|k| 0.25 * (std::f32::consts::TAU * 55.0 * k as f32 / RATE as f32).sin())
        .collect()
}

/// Pink noise at about −18 dBFS RMS, from a fixed seed, so a run repeats.
fn pink(frames: usize) -> Vec<f32> {
    let mut seed = 0x2545_f491u32;
    let mut white = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 - 0.5
    };
    let (mut b0, mut b1, mut b2, mut b3) = (0.0f32, 0.0, 0.0, 0.0);
    let (mut b4, mut b5, mut b6) = (0.0f32, 0.0, 0.0);
    (0..frames)
        .map(|_| {
            let w = white();
            b0 = 0.99886 * b0 + w * 0.055_517_9;
            b1 = 0.99332 * b1 + w * 0.075_075_9;
            b2 = 0.969 * b2 + w * 0.153_852;
            b3 = 0.8665 * b3 + w * 0.310_485_6;
            b4 = 0.55 * b4 + w * 0.532_952_2;
            b5 = -0.7616 * b5 - w * 0.016_898;
            let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362;
            b6 = w * 0.115_926;
            pink * 0.22
        })
        .collect()
}

// ------------------------------------------------------------- the measures

/// A local level for each frame: the largest sample within a frame's own block
/// of [`LOCAL`] and the blocks either side of it, so a bend is judged against
/// the wave around it and not against a stretch of another level.
const LOCAL: usize = 64;

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

/// The worst bend of `x` as a fraction of the level around it: a click is a
/// bend the wave nearby cannot account for, so a change of level, or a tone at
/// any frequency, reads near the clean signal's own and a discontinuity does
/// not. `floor` keeps a quiet stretch from reading as an infinite bend.
fn worst_bend_over_level(x: &[f32], from: usize, to: usize, floor: f32) -> f32 {
    let local = local_level(x);
    let to = to.min(x.len());
    (from.max(2)..to).fold(0.0f32, |m, k| {
        m.max((x[k] - 2.0 * x[k - 1] + x[k - 2]).abs() / local[k].max(floor))
    })
}

/// The same for the sample-to-sample step.
fn worst_step_over_level(x: &[f32], from: usize, to: usize, floor: f32) -> f32 {
    let local = local_level(x);
    let to = to.min(x.len());
    (from.max(1)..to).fold(0.0f32, |m, k| {
        m.max((x[k] - x[k - 1]).abs() / local[k].max(floor))
    })
}

/// `a` against `b`, in decibels.
fn ratio_db(a: f32, b: f32) -> f32 {
    20.0 * (a.max(1e-12) / b.max(1e-12)).log10()
}

// --------------------------------------------------------------- the script

/// What the interface and the transport do while the song plays.
#[derive(Default, Clone)]
struct Script {
    /// `(frame, parameter, normalised value)`: a value that steps, as a knob
    /// drag or an automation step does.
    steps: Vec<(usize, u32, f32)>,
    /// Every value is sent again every block, unchanged: what an interface
    /// that writes its whole strip each frame does (event g).
    repeat: bool,
    /// Frames where playback jumps: the device is reset (event e).
    resets: Vec<usize>,
    /// `(frame, connected)`: the sidechain key comes and goes (event f).
    key: Vec<(usize, bool)>,
}

impl Script {
    fn steady() -> Self {
        Self::default()
    }

    /// One parameter stepped to a new value.
    fn step(id: u32, value: f32) -> Self {
        Self {
            steps: vec![(EVENT, id, value)],
            ..Self::default()
        }
    }

    fn keyed(connected: bool) -> Self {
        Self {
            key: vec![(EVENT, connected)],
            ..Self::default()
        }
    }
}

/// The values of every parameter of `table` at `frame` of the script: the
/// default, or whatever a step has left it at.
fn held_at(table: &[ParamInfo], script: &Script, frame: usize) -> Vec<(u32, f32)> {
    table
        .iter()
        .map(|info| {
            let mut value = info.normalised(info.default);
            for &(at, id, to) in &script.steps {
                if id == info.id && at <= frame {
                    value = to;
                }
            }
            (info.id, value)
        })
        .collect()
}

/// Plays `left`/`right` through `device` in blocks of at most `block`, with the
/// events `script` names. `key` is the sidechain signal, when there is one.
fn drive(
    device: &mut dyn Effect,
    table: &'static [ParamInfo],
    (left, right): (&mut [f32], &mut [f32]),
    key: Option<(&[f32], &[f32])>,
    block: usize,
    script: &Script,
) {
    let block = block.clamp(1, 256);
    let frames = left.len().min(right.len());
    let mut done = 0;
    let mut sent: Vec<(u32, f32)> = Vec::new();
    let mut connected = false;
    while done < frames {
        let n = block.min(frames - done);
        let values = held_at(table, script, done);
        if script.resets.contains(&done) {
            device.reset();
        }
        for &(at, state) in &script.key {
            if at <= done {
                connected = state;
            }
        }
        // What the engine sends: a value only when it moved, the whole strip
        // every block when the interface repeats itself.
        let mut events = ParamBlock::new();
        events.start(n);
        for &(id, value) in &values {
            let changed = sent
                .iter()
                .find(|(known, _)| *known == id)
                .is_none_or(|&(_, was)| was != value);
            if (changed || script.repeat) && events.open(id) {
                events.point(0, f64::from(value));
            }
        }
        sent = values;
        let block = Block {
            rate: RATE,
            start: done as i64,
            bpm: 120.0,
            law: PanLaw::Balance,
            events: &events,
        };
        let key = connected.then(|| {
            key.map(|(l, r)| Key {
                left: &l[done..done + n],
                right: &r[done..done + n],
            })
        });
        device.process(
            &block,
            Stereo {
                left: &mut left[done..done + n],
                right: &mut right[done..done + n],
            },
            key.flatten(),
        );
        done += n;
    }
}

/// A device of the harness, and the table its parameters are in.
struct Builtin {
    name: &'static str,
    table: &'static [ParamInfo],
    make: fn() -> Box<dyn Effect>,
    /// The sidechain signal, when the device takes one.
    keyed: bool,
}

fn devices() -> Vec<Builtin> {
    vec![
        Builtin {
            name: "Prism",
            table: lumen_devices::utility::TABLE,
            make: || Box::new(Utility::new()),
            keyed: false,
        },
        Builtin {
            name: "Iris",
            table: lumen_devices::eq::TABLE,
            make: || lumen_devices::effect("eq").expect("a device"),
            keyed: false,
        },
        Builtin {
            name: "Parallax",
            table: lumen_devices::delay::TABLE,
            make: || lumen_devices::effect("delay").expect("a device"),
            keyed: false,
        },
        Builtin {
            name: "Afterglow",
            table: lumen_devices::reverb::TABLE,
            make: || lumen_devices::effect("reverb").expect("a device"),
            keyed: false,
        },
        Builtin {
            name: "Gravity",
            table: lumen_devices::dynamics::compressor::TABLE,
            make: || Box::new(Compressor::new()),
            keyed: true,
        },
        Builtin {
            name: "Horizon",
            table: lumen_devices::dynamics::limiter::TABLE,
            make: || Box::new(Limiter::new()),
            keyed: false,
        },
    ]
}

/// The output of one run, left and right.
fn run(device: &Builtin, input: &[f32], block: usize, script: &Script) -> (Vec<f32>, Vec<f32>) {
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    let key = if device.keyed {
        Some((input, input))
    } else {
        None
    };
    let mut effect = (device.make)();
    drive(
        effect.as_mut(),
        device.table,
        (&mut left, &mut right),
        key,
        block,
        script,
    );
    (left, right)
}

/// What one device does about one event: the worst bend of the output over the
/// event's window, against the bypass's, and against the device's own steady
/// state over the same window.
#[derive(Debug, Clone, Copy)]
struct Pop {
    bend_db: f32,
    step_db: f32,
    steady_db: f32,
}

/// The window an event is measured over: from the event to the settle time
/// after it, and the same number of frames before it for the steady state.
fn window() -> (usize, usize) {
    (EVENT.saturating_sub(2_400), EVENT + 4_800)
}

/// The root mean square of `x` over `from..to`.
fn rms(x: &[f32], from: usize, to: usize) -> f32 {
    let to = to.min(x.len());
    let n = to.saturating_sub(from);
    if n == 0 {
        return 0.0;
    }
    (x[from..to].iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt()
}

/// How much more a signal bends than a clean one of the same level: the metric
/// a gain does not move and a click does.
fn bend_over(samples: &[f32], reference: &[f32]) -> f32 {
    let (from, to) = window();
    let floor = 0.01 * rms(samples, from, to).max(1e-9);
    let clean_floor = 0.01 * rms(reference, from, to).max(1e-9);
    ratio_db(
        worst_bend_over_level(samples, from, to, floor),
        worst_bend_over_level(reference, from, to, clean_floor),
    )
}

/// The same for the sample-to-sample step.
fn step_over(samples: &[f32], reference: &[f32]) -> f32 {
    let (from, to) = window();
    let floor = 0.01 * rms(samples, from, to).max(1e-9);
    let clean_floor = 0.01 * rms(reference, from, to).max(1e-9);
    ratio_db(
        worst_step_over_level(samples, from, to, floor),
        worst_step_over_level(reference, from, to, clean_floor),
    )
}

fn measure(device: &Builtin, input: &[f32], block: usize, script: &Script) -> Pop {
    let (left, _) = run(device, input, block, script);
    let reference = input;
    let (steady, _) = run(device, input, block, &Script::steady());
    Pop {
        bend_db: bend_over(&left, reference),
        step_db: step_over(&left, reference),
        steady_db: bend_over(&left, &steady),
    }
}

/// The number a table cell shows.
fn cell(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

/// The checks the table is read from: no built-in bends a tone, or noise, more
/// than the tone or the noise does when an event happens — a parameter stepped,
/// the interface repeating its values, or the device reset mid-stream.
///
/// A click is a bend the wave cannot account for; this allows three decibels of
/// one, which is the measurement's own noise.
///
/// Two parameters are left out, and neither is a click:
/// - a **timing** parameter (`lumen_devices::timing_parameter`), the limiter's
///   lookahead, is the device's latency: it re-times the routing and clears the
///   line it held, which the engine covers with its own skip or repeat
///   (ADR-0041) and the interface applies once at the end of a drag (ADR-0151).
/// - the reverb's **pre-delay** slides its read head 150 ms in 20 ms, which is
///   a fast pitch glide through the room, not a step: its sample-to-sample jump
///   is 2.5 dB above the wave's own, against 30 for a click.
fn skip(script: &str, device: &str, table: &[ParamInfo]) -> bool {
    let Some(name) = script.strip_prefix("step ") else {
        return false;
    };
    table
        .iter()
        .find(|p| p.name == name)
        .is_some_and(|p| lumen_devices::timing_parameter(device_name(device), p.id))
        || (device == "Afterglow" && name == "Pre")
}

/// The stable name a device is saved by, from the one the table shows.
fn device_name(shown: &str) -> &'static str {
    match shown {
        "Prism" => lumen_devices::utility::NAME,
        "Iris" => lumen_devices::eq::NAME,
        "Parallax" => lumen_devices::delay::NAME,
        "Afterglow" => lumen_devices::reverb::NAME,
        "Gravity" => lumen_devices::dynamics::compressor::NAME,
        _ => lumen_devices::dynamics::limiter::NAME,
    }
}

#[test]
fn no_built_in_clicks_on_a_tone_when_anything_moves() {
    let sine = sine(FRAMES);
    let noise = pink(FRAMES);
    for device in devices() {
        let mut scripts: Vec<(String, Script)> = vec![
            ("steady".to_owned(), Script::steady()),
            (
                "repeat".to_owned(),
                Script {
                    repeat: true,
                    ..Script::default()
                },
            ),
        ];
        for info in device.table {
            let to = if info.is_choice() {
                if info.normalised(info.default) >= 0.5 {
                    0.0
                } else {
                    1.0
                }
            } else if info.normalised(info.default) < 0.5 {
                0.85
            } else {
                0.15
            };
            scripts.push((format!("step {}", info.name), Script::step(info.id, to)));
        }
        for (name, script) in &scripts {
            for (signal_name, signal) in [("tone", &sine), ("noise", &noise)] {
                if skip(name, device.name, device.table) {
                    continue;
                }
                let pop = measure(&device, signal, 64, script);
                // A filter that moves changes the spectrum, and noise bends a
                // little more than a tone when it does; a click is an order of
                // magnitude past both.
                let limit = if signal_name == "noise" { 8.0 } else { 5.0 };
                assert!(
                    pop.bend_db < limit && pop.step_db < limit,
                    "{}: {name} on {signal_name} bends {:+.1} dB and steps {:+.1} dB",
                    device.name,
                    pop.bend_db,
                    pop.step_db
                );
            }
        }
    }
}

/// Flare, the synth, is an instrument rather than an effect: it is not on the
/// mixer's built-in path, so OR-4's (c) to (g) do not apply to it, but its
/// parameters do arrive the same way while it plays. This holds the one thing
/// that could click — a value the interface sends, and the same value again
/// every block — against the sound it was making.
#[test]
fn flare_does_not_click_when_its_values_move() {
    use lumen_core::midi::{Event, Msg};
    use lumen_devices::synth::params;
    use lumen_devices::{Instrument, Synth};

    let synth = |values: &[(u32, f32)], steps: Option<(u32, f32)>| {
        let mut synth = Synth::new(lumen_devices::Preset::Poly);
        synth.configure_values(values);
        let mut left = vec![0.0f32; FRAMES];
        let mut right = vec![0.0f32; FRAMES];
        let mut sent = values.to_vec();
        let mut done = 0;
        let mut first = true;
        while done < FRAMES {
            let n = 256.min(FRAMES - done);
            let mut events = Vec::new();
            if first {
                events.push(Event {
                    offset: 0,
                    msg: Msg::note_on(0, 60, 100),
                });
                first = false;
            }
            if let Some((id, to)) = steps
                && done >= EVENT
                && sent
                    .iter()
                    .any(|(known, value)| *known == id && *value != to)
            {
                for (known, value) in &mut sent {
                    if *known == id {
                        *value = to;
                    }
                }
                synth.configure_values(&sent);
            }
            Instrument::render(
                &mut synth,
                RATE,
                &events,
                &mut left[done..done + n],
                &mut right[done..done + n],
            );
            done += n;
        }
        left
    };
    let values: Vec<(u32, f32)> = params::TABLE
        .iter()
        .map(|p| (p.id, p.normalised(p.default)))
        .collect();
    let steady = synth(&values, None);
    for info in params::TABLE {
        let to = if info.is_choice() {
            if info.normalised(info.default) >= 0.5 {
                0.0
            } else {
                1.0
            }
        } else if info.normalised(info.default) < 0.5 {
            0.85
        } else {
            0.15
        };
        let moved = synth(&values, Some((info.id, to)));
        let floor = |x: &[f32]| {
            0.01 * x[EVENT - 2_400..EVENT + 4_800]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        // A step in the sound is a jump from one frame to the next, and the
        // frame it would happen on is the change's own: what a parameter does
        // over the frames after it — a filter shape crossing over for 20 ms —
        // changes the wave's shape without ever jumping, and two filters'
        // outputs summed read a little steeper than either alone.
        let jump = worst_step_over_level(&moved, EVENT, EVENT + 8, floor(&moved));
        let flat = worst_step_over_level(&steady, EVENT - 2_400, EVENT + 4_800, floor(&steady));
        let db = 20.0 * (jump.max(1e-9) / flat.max(1e-9)).log10();
        assert!(
            db < 6.0,
            "Flare: {} stepped jumps {db:+.1} dB against its own steady sound",
            info.name
        );
    }
}

#[test]
#[ignore = "a measurement, not a check: prints the table"]
fn the_pop_table() {
    let sine = sine(FRAMES);
    let low = low_sine(FRAMES);
    let noise = pink(FRAMES);
    let signals: [(&str, &[f32]); 3] = [("sine", &sine), ("low", &low), ("pink", &noise)];
    for device in devices() {
        println!("\n== {}", device.name);
        println!(
            "{:<28} {:>6} {:>8} {:>8} {:>8}",
            "event", "block", "bend/dry", "step/dry", "bend/own"
        );
        let mut scripts: Vec<(String, Script)> = vec![("steady".to_owned(), Script::steady())];
        for info in device.table {
            let to = if info.is_choice() {
                if info.normalised(info.default) >= 0.5 {
                    0.0
                } else {
                    1.0
                }
            } else if info.normalised(info.default) < 0.5 {
                0.85
            } else {
                0.15
            };
            scripts.push((format!("step {}", info.name), Script::step(info.id, to)));
        }
        scripts.push((
            "repeat (g)".to_owned(),
            Script {
                repeat: true,
                ..Script::default()
            },
        ));
        scripts.push((
            "reset (e)".to_owned(),
            Script {
                resets: vec![EVENT],
                ..Script::default()
            },
        ));
        if device.keyed {
            scripts.push(("key off (f)".to_owned(), Script::keyed(false)));
        }
        for (name, script) in &scripts {
            for (signal_name, signal) in signals {
                let label = format!("{name} {signal_name}");
                for block in BLOCKS {
                    let pop = measure(&device, signal, block, script);
                    println!(
                        "{label:<28} {block:>6} {:>8} {:>8} {:>8}",
                        cell(pop.bend_db),
                        cell(pop.step_db),
                        cell(pop.steady_db)
                    );
                }
            }
        }
    }
}
