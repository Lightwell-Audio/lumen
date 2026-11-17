use lumen_core::pan::PanLaw;
use lumen_devices::params::ParamInfo;
use lumen_devices::{Block, Compressor, Effect, Key, Limiter, ParamBlock, Stereo, Utility};

const RATE: u32 = 48_000;

const BLOCKS: [usize; 4] = [64, 128, 256, 333];

const FRAMES: usize = 24_000;

const EVENT: usize = 9_600;

fn sine(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|k| 0.25 * (std::f32::consts::TAU * 997.0 * k as f32 / RATE as f32).sin())
        .collect()
}

fn low_sine(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|k| 0.25 * (std::f32::consts::TAU * 55.0 * k as f32 / RATE as f32).sin())
        .collect()
}

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

fn worst_bend_over_level(x: &[f32], from: usize, to: usize, floor: f32) -> f32 {
    let local = local_level(x);
    let to = to.min(x.len());
    (from.max(2)..to).fold(0.0f32, |m, k| {
        m.max((x[k] - 2.0 * x[k - 1] + x[k - 2]).abs() / local[k].max(floor))
    })
}

fn worst_step_over_level(x: &[f32], from: usize, to: usize, floor: f32) -> f32 {
    let local = local_level(x);
    let to = to.min(x.len());
    (from.max(1)..to).fold(0.0f32, |m, k| {
        m.max((x[k] - x[k - 1]).abs() / local[k].max(floor))
    })
}

fn ratio_db(a: f32, b: f32) -> f32 {
    20.0 * (a.max(1e-12) / b.max(1e-12)).log10()
}

#[derive(Default, Clone)]
struct Script {
    steps: Vec<(usize, u32, f32)>,
    repeat: bool,
    resets: Vec<usize>,
    key: Vec<(usize, bool)>,
}

impl Script {
    fn steady() -> Self {
        Self::default()
    }

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

struct Builtin {
    name: &'static str,
    table: &'static [ParamInfo],
    make: fn() -> Box<dyn Effect>,
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

#[derive(Debug, Clone, Copy)]
struct Pop {
    bend_db: f32,
    step_db: f32,
    steady_db: f32,
}

fn window() -> (usize, usize) {
    (EVENT.saturating_sub(2_400), EVENT + 4_800)
}

fn rms(x: &[f32], from: usize, to: usize) -> f32 {
    let to = to.min(x.len());
    let n = to.saturating_sub(from);
    if n == 0 {
        return 0.0;
    }
    (x[from..to].iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt()
}

fn bend_over(samples: &[f32], reference: &[f32]) -> f32 {
    let (from, to) = window();
    let floor = 0.01 * rms(samples, from, to).max(1e-9);
    let clean_floor = 0.01 * rms(reference, from, to).max(1e-9);
    ratio_db(
        worst_bend_over_level(samples, from, to, floor),
        worst_bend_over_level(reference, from, to, clean_floor),
    )
}

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

fn cell(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

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
