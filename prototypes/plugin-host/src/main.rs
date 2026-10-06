//! P-HOST prototype: can Lumen host a real VST3 instrument, make it sound,
//! and save and restore its state?
//!
//! ```text
//! plugin-host render <module.vst3> [--class NAME]   step 1: in this process
//! ```

#[cfg(windows)]
mod com;
#[cfg(windows)]
mod vst;

#[cfg(not(windows))]
fn main() {
    eprintln!("plugin-host only runs on Windows for now");
}

#[cfg(windows)]
fn main() {
    use std::path::PathBuf;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let class = args
        .iter()
        .position(|a| a == "--class")
        .and_then(|i| args.get(i + 1))
        .cloned();
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("render"), Some(module)) => {
            if let Err(e) = render_test(&PathBuf::from(module), class.as_deref()) {
                eprintln!("FAILED: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: plugin-host render <module.vst3> [--class NAME]");
            std::process::exit(2);
        }
    }
}

#[cfg(windows)]
const RATE: f64 = 48_000.0;
#[cfg(windows)]
const BLOCK: usize = 128;

/// Plays middle C for 1 s, then 1 s of release; returns interleaved audio
/// and the per-block processing times in microseconds.
#[cfg(windows)]
fn play(plugin: &mut vst::Plugin) -> Result<(Vec<f32>, Vec<f32>), String> {
    use vst::Note;
    let blocks = (2.0 * RATE) as usize / BLOCK;
    let off_block = blocks / 2;
    let (mut left, mut right) = (vec![0.0_f32; BLOCK], vec![0.0_f32; BLOCK]);
    let mut audio = Vec::with_capacity(blocks * BLOCK * 2);
    let mut times = Vec::with_capacity(blocks);
    for block in 0..blocks {
        let notes: &[(i32, Note)] = match block {
            0 => &[(
                0,
                Note::On {
                    pitch: 60,
                    velocity: 0.8,
                },
            )],
            b if b == off_block => &[(0, Note::Off { pitch: 60 })],
            _ => &[],
        };
        left.fill(0.0);
        right.fill(0.0);
        let started = std::time::Instant::now();
        plugin.process(notes, &mut left, &mut right)?;
        times.push(started.elapsed().as_secs_f32() * 1e6);
        for i in 0..BLOCK {
            audio.push(left[i]);
            audio.push(right[i]);
        }
    }
    Ok((audio, times))
}

#[cfg(windows)]
fn level(audio: &[f32]) -> (f32, f32, bool) {
    let finite = audio.iter().all(|s| s.is_finite());
    let peak = audio.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len().max(1) as f32).sqrt();
    let db = |x: f32| 20.0 * x.max(1e-9).log10();
    (db(peak), db(rms), finite)
}

#[cfg(windows)]
fn write_wav(path: &std::path::Path, audio: &[f32]) -> std::io::Result<()> {
    let data_len = (audio.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&3_u16.to_le_bytes()); // IEEE float
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&(RATE as u32).to_le_bytes());
    bytes.extend_from_slice(&(RATE as u32 * 8).to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in audio {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes)
}

#[cfg(windows)]
fn render_test(module: &std::path::Path, class: Option<&str>) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut plugin = vst::Plugin::load(module, class)?;
    plugin.activate(RATE, BLOCK as i32)?;
    println!(
        "loaded \"{}\" and activated in {:.0} ms",
        plugin.name,
        started.elapsed().as_secs_f64() * 1e3
    );

    let (audio, mut times) = play(&mut plugin)?;
    let (peak, rms, finite) = level(&audio);
    times.sort_by(f32::total_cmp);
    let at = |p: f64| times[((times.len() - 1) as f64 * p) as usize];
    println!(
        "render 1: peak {peak:.1} dBFS, RMS {rms:.1} dBFS, all samples finite: {finite}; process() per 128-sample block p50 {:.0} / p99 {:.0} / max {:.0} µs (budget 2,667)",
        at(0.5),
        at(0.99),
        at(1.0)
    );
    let out = PathBuf::from("prototypes/plugin-host/out");
    let _ = std::fs::create_dir_all(&out);
    let wav = out.join("render-1.wav");
    if write_wav(&wav, &audio).is_ok() {
        println!("wrote {}", wav.display());
    }

    let state = plugin.state()?;
    println!("state saved: {} bytes", state.len());
    drop(plugin);

    let mut restored = vst::Plugin::load(module, class)?;
    restored.set_state(&state)?;
    restored.activate(RATE, BLOCK as i32)?;
    let state_again = restored.state()?;
    println!(
        "fresh instance restored from state: re-saved state {} ({} bytes)",
        if state_again == state {
            "IDENTICAL"
        } else {
            "DIFFERS"
        },
        state_again.len()
    );
    let (audio2, _) = play(&mut restored)?;
    let (peak2, rms2, finite2) = level(&audio2);
    println!(
        "render 2 (restored): peak {peak2:.1} dBFS, RMS {rms2:.1} dBFS, all samples finite: {finite2}"
    );
    if !(finite && finite2) || peak < -60.0 || peak2 < -60.0 {
        return Err("plugin produced no usable audio".into());
    }
    Ok(())
}

#[cfg(windows)]
use std::path::PathBuf;
