//! Hardware check for the device layer: an ASIO stream under the supervisor,
//! printing every device event. Unplug and replug the interface while it runs
//! to exercise loss and recovery.
//!
//! ```text
//! cargo run --release -p lumen-audio --example asio_tone -- list
//! cargo run --release -p lumen-audio --example asio_tone -- "Focusrite USB ASIO" [--seconds 60] [--tone] [--buffer N]
//! ```
//! Silent unless `--tone` is given (440 Hz at -24 dBFS on outputs 1 and 2).

#[cfg(not(windows))]
fn main() {
    eprintln!("asio_tone needs Windows");
}

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};

    use lumen_audio::asio::AsioBackend;
    use lumen_audio::{AudioSystem, Block, State, StreamConfig, Timing};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    let Some(target) = args.first() else {
        eprintln!("usage: asio_tone list | asio_tone <driver name> [--seconds S] [--tone] [--buffer N]");
        std::process::exit(2);
    };
    let drivers = AsioBackend::registered();
    if target == "list" {
        for (_, name) in &drivers {
            println!("{name}");
        }
        return;
    }
    let Some((device, name)) = drivers.into_iter().find(|(_, n)| n.eq_ignore_ascii_case(target)) else {
        eprintln!("no ASIO driver named {target:?}; try `list`");
        std::process::exit(1);
    };
    let seconds: u64 = value("--seconds").and_then(|s| s.parse().ok()).unwrap_or(30);
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
    let audio = AudioSystem::spawn(
        || AsioBackend::new().expect("COM apartment for ASIO"),
        Box::new(callback),
        Timing::default(),
    );
    audio.configure(StreamConfig {
        device,
        sample_rate: None,
        buffer_frames: value("--buffer").and_then(|s| s.parse().ok()),
        input_channels: vec![0, 1],
        output_channels: vec![0, 1],
    });
    println!("{name}: running for {seconds} s{}", if tone { " with a tone" } else { " (silent)" });
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut next_report = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        while let Some(event) = audio.try_event() {
            println!("[{:>5.1} s] event: {event:?}", seconds as f64 - (deadline - Instant::now()).as_secs_f64());
        }
        if Instant::now() >= next_report {
            let status = audio.status();
            if status.state() == State::Running {
                println!(
                    "          running: {} callbacks, peak load {:.2}%, overloads {}",
                    status.callbacks(),
                    status.take_peak_load() * 100.0,
                    status.overloads()
                );
            } else {
                println!("          state: {:?}", status.state());
            }
            next_report += Duration::from_secs(1);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    audio.stop();
    if let Some(event) = audio.wait_event(Duration::from_secs(2)) {
        println!("event: {event:?}");
    }
}
