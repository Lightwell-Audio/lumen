//! Hardware check for the engine: a click on every beat (higher on the
//! downbeat), driven from the keyboard. Listen for steady clicks, clean loop
//! wraps and tempo changes that keep the bar.
//!
//! ```text
//! cargo run --release -p lumen-engine --example transport_demo -- "Focusrite USB ASIO" [--driver D]
//! ```
//! Type a command and press Enter:
//! `p` play · `a` pause · `s` stop · `g 5` go to bar 5 · `l 2 4` loop bars 2–3
//! · `l` loop off · `t 140` tempo · `m 7/8` time signature · `q` quit.
//! Drivers as in `audio_tone`: `asio` (Windows default), `wasapi`,
//! `exclusive`, `cpal` (macOS and Linux default).

use std::io::BufRead;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lumen_audio::{AudioSystem, Backend, Block, DeviceId, StreamConfig, Timing};
use lumen_core::meter::Meter;
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{Command, Jump, Output, Region, RenderContext, Source, Timeline, engine};

/// Short sine bursts on every beat.
#[derive(Default)]
struct Click {
    /// Samples left in the current click, and its frequency.
    remaining: usize,
    length: usize,
    frequency: f32,
    phase: f32,
}

impl Click {
    fn start(&mut self, downbeat: bool, sample_rate: u32) {
        self.length = sample_rate as usize / 40; // 25 ms
        self.remaining = self.length;
        self.frequency = if downbeat { 1_760.0 } else { 880.0 };
        self.phase = 0.0;
    }

    fn write(&mut self, out: &mut Output<'_>, from: usize, to: usize, sample_rate: u32) {
        let step = self.frequency / sample_rate as f32;
        for i in from..to {
            if self.remaining == 0 {
                return;
            }
            let envelope = self.remaining as f32 / self.length as f32;
            let value = (self.phase * std::f32::consts::TAU).sin() * envelope * envelope * 0.3;
            for channel in 0..out.channels().min(2) {
                out.channel(channel)[i] += value;
            }
            self.phase = (self.phase + step).fract();
            self.remaining -= 1;
        }
    }
}

impl Source for Click {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        let (timeline, rate) = (cx.timeline, cx.sample_rate);
        let end = cx.start + out.frames() as i64;
        let mut written = 0;
        // Start a quarter early so a beat that rounds onto `cx.start` is kept.
        let mut beat = timeline
            .meter
            .next_beat(timeline.ticks_at(cx.start, rate) - Ticks::quarters(1));
        loop {
            let at = timeline.sample_at(beat, rate);
            if at >= end {
                break;
            }
            if at >= cx.start {
                let offset = (at - cx.start) as usize;
                self.write(out, written, offset, rate);
                written = offset;
                self.start(timeline.meter.bbt_at(beat).beat == 0, rate);
            }
            beat = timeline.meter.next_beat(beat + Ticks(1));
        }
        self.write(out, written, out.frames(), rate);
    }

    fn locate(&mut self, _: &Timeline, _: u32, _: i64, _: Jump) {
        self.remaining = 0;
    }
}

type MakeBackend = fn() -> Box<dyn Backend>;

fn backend(driver: &str) -> Option<MakeBackend> {
    match driver {
        #[cfg(windows)]
        "asio" => Some(|| {
            Box::new(lumen_audio::asio::AsioBackend::new().expect("COM apartment for ASIO"))
        }),
        #[cfg(windows)]
        "wasapi" => Some(|| Box::new(lumen_audio::wasapi::WasapiBackend::new(false))),
        #[cfg(windows)]
        "exclusive" => Some(|| Box::new(lumen_audio::wasapi::WasapiBackend::new(true))),
        #[cfg(not(windows))]
        "cpal" => Some(|| Box::new(lumen_audio::cpal_host::CpalBackend::new())),
        _ => None,
    }
}

fn find_device(driver: &str, make: MakeBackend, name: &str) -> Option<DeviceId> {
    #[cfg(windows)]
    if driver == "asio" {
        return lumen_audio::asio::AsioBackend::registered()
            .into_iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|(id, _)| id);
    }
    let _ = driver;
    let name = name.to_lowercase();
    make()
        .devices()
        .into_iter()
        .find(|d| d.name.to_lowercase().contains(&name))
        .map(|d| d.id)
}

/// Start of a one-based bar, as the user types it.
fn bar(meter: Meter, n: i64) -> Ticks {
    Ticks((n - 1) * meter.bar().0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let default_driver = if cfg!(windows) { "asio" } else { "cpal" };
    let driver = args
        .iter()
        .position(|a| a == "--driver")
        .and_then(|i| args.get(i + 1))
        .map_or(default_driver, String::as_str);
    let (Some(name), Some(make)) = (args.first(), backend(driver)) else {
        eprintln!("usage: transport_demo <device> [--driver asio|wasapi|exclusive|cpal]");
        std::process::exit(2);
    };
    let Some(device) = find_device(driver, make, name) else {
        eprintln!("no {driver} device matching {name:?}");
        std::process::exit(1);
    };

    let mut bpm = 120.0;
    let mut meter = Meter::FOUR_FOUR;
    let (mut handle, mut engine) = engine(Timeline::constant(bpm, meter), Click::default(), 48_000);
    let callback = move |block: &mut Block<'_>| {
        let (rate, channels, frames) = (block.sample_rate, block.output_channels, block.frames);
        engine.process(rate, block.outputs, channels, frames);
    };
    let audio = AudioSystem::spawn(make, Box::new(callback), Timing::default());
    audio.configure(StreamConfig {
        device,
        sample_rate: None,
        buffer_frames: None,
        input_channels: vec![],
        output_channels: vec![0, 1],
    });

    let (lines_tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    println!("p play · a pause · s stop · g <bar> · l <from> <to> · l · t <bpm> · m <n>/<d> · q");

    let mut last_shown = String::new();
    let mut next_show = Instant::now();
    loop {
        while let Some(event) = audio.try_event() {
            println!("device: {event:?}");
        }
        if let Ok(line) = lines.try_recv() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let number = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok());
            let command = match words.as_slice() {
                ["q"] => break,
                ["p"] => Some(Command::Play),
                ["a"] => Some(Command::Pause),
                ["s"] => Some(Command::Stop),
                ["g", _] => number(1).map(|b| Command::Locate(bar(meter, b as i64))),
                ["l"] => Some(Command::SetLoop(Region::default())),
                ["l", _, _] => match (number(1), number(2)) {
                    (Some(from), Some(to)) => Some(Command::SetLoop(Region::new(
                        bar(meter, from as i64),
                        bar(meter, to as i64),
                        true,
                    ))),
                    _ => None,
                },
                ["t", _] => {
                    match number(1).and_then(|b| TempoMap::constant(b).ok().map(|_| b)) {
                        Some(b) => bpm = b,
                        None => println!("tempo must be 20–999"),
                    }
                    let _ = handle.set_timeline(Timeline::constant(bpm, meter));
                    None
                }
                ["m", sig] => {
                    let parsed = sig.split_once('/').and_then(|(n, d)| {
                        Some(Meter {
                            numerator: n.parse().ok()?,
                            denominator: d.parse().ok()?,
                        })
                    });
                    match parsed.filter(|m| m.is_valid()) {
                        Some(m) => meter = m,
                        None => println!("time signature like 4/4, 7/8 or 3/4"),
                    }
                    let _ = handle.set_timeline(Timeline::constant(bpm, meter));
                    None
                }
                _ => {
                    println!("?");
                    None
                }
            };
            if let Some(command) = command {
                let _ = handle.send(command);
            }
        }
        if Instant::now() >= next_show {
            let t = handle.transport();
            let bbt = Timeline::constant(bpm, meter).meter.bbt_at(t.ticks);
            let shown = format!(
                "{} {bbt}  {bpm} BPM {meter}  sample {} at {} Hz{}",
                if t.playing { "▶" } else { "■" },
                t.sample,
                t.sample_rate,
                if t.punching { "  punch" } else { "" }
            );
            if shown != last_shown {
                println!("{shown}");
            }
            last_shown = shown;
            next_show += Duration::from_millis(250);
        }
        handle.collect_garbage();
        std::thread::sleep(Duration::from_millis(10));
    }
    audio.stop();
}
