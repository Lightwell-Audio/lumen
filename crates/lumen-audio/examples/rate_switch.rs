//! Times sample-rate changes on an ASIO device (M2 slice 8, MT-APP-6): from
//! asking for a new rate until audio runs again. Silent.
//!
//! ```text
//! cargo run --release -p lumen-audio --example rate_switch -- "Focusrite USB ASIO"
//! ```

#[cfg(not(windows))]
fn main() {
    eprintln!("rate_switch needs Windows");
}

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};

    use lumen_audio::asio::AsioBackend;
    use lumen_audio::{AudioSystem, Block, DeviceEvent, StreamConfig, Timing};

    let name = std::env::args()
        .nth(1)
        .unwrap_or("Focusrite USB ASIO".into());
    let Some((device, _)) = AsioBackend::registered()
        .into_iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(&name))
    else {
        eprintln!("no ASIO driver named {name:?}");
        std::process::exit(1);
    };
    let audio = AudioSystem::spawn(
        || AsioBackend::new().expect("COM apartment"),
        Box::new(|_: &mut Block<'_>| {}),
        Timing::default(),
    );
    let config = |rate| StreamConfig {
        device: device.clone(),
        sample_rate: Some(rate),
        buffer_frames: None,
        input_channels: vec![],
        output_channels: vec![0, 1],
    };
    for rate in [48_000, 44_100, 48_000, 44_100, 48_000] {
        let asked = Instant::now();
        audio.configure(config(rate));
        match audio.wait_event(Duration::from_secs(10)) {
            Some(DeviceEvent::Started(info)) => {
                let started = asked.elapsed();
                // Until the first callbacks actually arrive.
                let callbacks = audio.status().callbacks();
                while audio.status().callbacks() < callbacks + 3 {
                    std::thread::sleep(Duration::from_millis(1));
                }
                println!(
                    "-> {rate} Hz: started in {:.0} ms, audio flowing after {:.0} ms ({} Hz, {} frames)",
                    started.as_secs_f64() * 1e3,
                    asked.elapsed().as_secs_f64() * 1e3,
                    info.sample_rate,
                    info.buffer_frames
                );
            }
            other => println!(
                "-> {rate} Hz: {other:?} after {:.0} ms",
                asked.elapsed().as_secs_f64() * 1e3
            ),
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    audio.stop();
}
