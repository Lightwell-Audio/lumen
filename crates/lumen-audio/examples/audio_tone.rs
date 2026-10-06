//! Hardware check for the device layer: a stream under the supervisor,
//! printing every device event. Unplug and replug the interface, or change
//! its settings, while it runs to exercise recovery.
//!
//! ```text
//! cargo run --release -p lumen-audio --example audio_tone -- list [--driver D]
//! cargo run --release -p lumen-audio --example audio_tone -- <device> [--driver D] [--seconds 60] [--tone] [--buffer N] [--rate R]
//! ```
//! Drivers: `asio` (the Windows default), `wasapi`, `exclusive` (WASAPI
//! exclusive mode), `cpal` (the macOS and Linux default). ASIO devices are
//! matched by exact name, others by any part of the name.
//! Silent unless `--tone` is given (440 Hz at -24 dBFS on outputs 1 and 2).

use std::time::{Duration, Instant};

use lumen_audio::{
    AudioSystem, Backend, Block, DeviceEvent, DeviceId, State, StreamConfig, Timing,
};

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

/// (id, name) of every device, without loading ASIO drivers.
fn devices(driver: &str, make: MakeBackend) -> Vec<(DeviceId, String)> {
    #[cfg(windows)]
    if driver == "asio" {
        return lumen_audio::asio::AsioBackend::registered();
    }
    let _ = driver;
    make()
        .devices()
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    let default_driver = if cfg!(windows) { "asio" } else { "cpal" };
    let driver = value("--driver").map_or(default_driver, String::as_str);
    let Some(make) = backend(driver) else {
        eprintln!("unknown driver {driver:?} on this platform");
        std::process::exit(2);
    };
    let Some(target) = args.first() else {
        eprintln!(
            "usage: audio_tone list [--driver D] | audio_tone <device> [--driver D] \
             [--seconds S] [--tone] [--buffer N] [--rate R]"
        );
        std::process::exit(2);
    };
    if target == "list" {
        if driver == "asio" {
            for (_, name) in devices(driver, make) {
                println!("{name}");
            }
        } else {
            for d in make().devices() {
                println!(
                    "{}\n    {} outputs, rates {:?} (now {:?}), buffers {:?}",
                    d.name,
                    d.output_channels.len(),
                    d.sample_rates,
                    d.current_sample_rate,
                    d.buffer_sizes
                );
            }
        }
        return;
    }
    let wanted = target.to_lowercase();
    let Some((device, name)) = devices(driver, make).into_iter().find(|(_, n)| {
        let n = n.to_lowercase();
        if driver == "asio" {
            n == wanted
        } else {
            n.contains(&wanted)
        }
    }) else {
        eprintln!("no {driver} device matching {target:?}; try `list --driver {driver}`");
        std::process::exit(1);
    };
    let seconds: u64 = value("--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let tone = args.iter().any(|a| a == "--tone");

    let mut phase = 0.0_f32;
    let callback = move |block: &mut Block<'_>| {
        if !tone {
            return;
        }
        let step = std::f32::consts::TAU * 440.0 / block.sample_rate as f32;
        let start = phase;
        for channel in 0..block.output_channels.min(2) {
            let mut p = start;
            for sample in block.output(channel) {
                *sample = p.sin() * 0.063;
                p = (p + step) % std::f32::consts::TAU;
            }
            phase = p;
        }
    };
    let audio = AudioSystem::spawn(make, Box::new(callback), Timing::default());
    audio.configure(StreamConfig {
        device,
        sample_rate: value("--rate").and_then(|s| s.parse().ok()),
        buffer_frames: value("--buffer").and_then(|s| s.parse().ok()),
        // Only ASIO opens inputs so far (the others arrive in M2 slice 8).
        input_channels: if driver == "asio" { vec![0, 1] } else { vec![] },
        output_channels: vec![0, 1],
    });
    println!(
        "{name} ({driver}): running for {seconds} s{}",
        if tone { " with a tone" } else { " (silent)" }
    );
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);
    let mut next_report = started + Duration::from_secs(1);
    // Callbacks per second the stream should deliver, from its StreamInfo.
    let mut expected = None;
    let mut last_count = 0;
    while Instant::now() < deadline {
        while let Some(event) = audio.try_event() {
            println!(
                "[{:>5.1} s] event: {event:?}",
                started.elapsed().as_secs_f64()
            );
            if let DeviceEvent::Started(info)
            | DeviceEvent::Reset(info)
            | DeviceEvent::Restored(info) = &event
            {
                expected = (info.buffer_frames > 0)
                    .then(|| f64::from(info.sample_rate) / f64::from(info.buffer_frames));
            }
        }
        if Instant::now() >= next_report {
            let status = audio.status();
            let count = status.callbacks();
            if status.state() == State::Running {
                let rate = match expected {
                    Some(e) => format!("{}/s, expected {e:.0}", count - last_count),
                    None => format!("{}/s", count - last_count),
                };
                println!(
                    "          running: {count} callbacks ({rate}), peak load {:.2}%, overloads {}",
                    status.take_peak_load() * 100.0,
                    status.overloads()
                );
            } else {
                println!("          state: {:?}", status.state());
            }
            last_count = count;
            next_report += Duration::from_secs(1);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    audio.stop();
    if let Some(event) = audio.wait_event(Duration::from_secs(2)) {
        println!("event: {event:?}");
    }
}
