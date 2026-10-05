//! Driver capability queries, each run in a sandboxed child process.
//!
//! Drivers whose hardware is missing may block on a modal dialog or crash,
//! so the parent only ever talks to them through a child with a timeout.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::driver::{BufferSizes, ComApartment, Driver, HiddenWindow};
use crate::ffi;
use crate::registry::{self, RegisteredDriver};

const PROBE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[derive(Debug, Serialize, Deserialize)]
pub struct DriverReport {
    pub driver_name: String,
    pub version: i32,
    pub init_ms: f64,
    pub inputs: i32,
    pub outputs: i32,
    pub buffer_sizes: Option<BufferSizes>,
    pub sample_rate: Option<f64>,
    pub supported_rates: Vec<u32>,
    pub input_names: Vec<String>,
    pub output_names: Vec<String>,
    pub sample_types: Vec<String>,
    pub clock_sources: Vec<String>,
}

/// Child-process side: loads the driver, queries it, prints one JSON line.
pub fn query_one_and_print(clsid: &str) {
    let result = query_one(clsid);
    println!(
        "{}",
        serde_json::to_string(&result).expect("report serializes")
    );
}

fn query_one(clsid: &str) -> Result<DriverReport, String> {
    let _com = ComApartment::enter()?;
    let window = HiddenWindow::create()?;
    let driver = Driver::create(clsid)?;
    let started = Instant::now();
    driver
        .init(window.0)
        .map_err(|message| format!("init failed: {message}"))?;
    let init_ms = started.elapsed().as_secs_f64() * 1e3;

    let (inputs, outputs) = driver.channels().map_err(asio_error)?;
    let mut sample_types = Vec::new();
    let mut names = |count: i32, input: bool| -> Vec<String> {
        (0..count)
            .map(|channel| match driver.channel_info(channel, input) {
                Ok(info) => {
                    let kind = ffi::sample_type_name(info.sample_type).to_string();
                    if !sample_types.contains(&kind) {
                        sample_types.push(kind);
                    }
                    ffi::c_string(&info.name)
                }
                Err(e) => format!("<{}>", ffi::error_name(e)),
            })
            .collect()
    };
    let input_names = names(inputs, true);
    let output_names = names(outputs, false);
    Ok(DriverReport {
        driver_name: driver.name(),
        version: driver.version(),
        init_ms,
        inputs,
        outputs,
        buffer_sizes: driver.buffer_sizes().ok(),
        sample_rate: driver.sample_rate().ok(),
        supported_rates: PROBE_RATES
            .into_iter()
            .filter(|&rate| driver.can_sample_rate(f64::from(rate)))
            .collect(),
        input_names,
        output_names,
        sample_types,
        clock_sources: driver
            .clock_sources()
            .map(|sources| {
                sources
                    .into_iter()
                    .map(|(name, current)| {
                        if current {
                            format!("{name} (current)")
                        } else {
                            name
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn asio_error(e: ffi::AsioError) -> String {
    format!("{} ({e})", ffi::error_name(e))
}

#[derive(Debug)]
pub enum Outcome {
    Report(Box<DriverReport>),
    DriverError(String),
    TimedOut,
    Crashed(Option<i32>),
    BadOutput(String),
}

/// Runs `query-one` for one driver in a child process with a deadline.
pub fn query_sandboxed(driver: &RegisteredDriver, timeout: Duration) -> (Outcome, Duration) {
    let started = Instant::now();
    let exe = std::env::current_exe().expect("own executable path");
    let mut child = match Command::new(exe)
        .args(["query-one", &driver.clsid])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return (
                Outcome::BadOutput(format!("spawn failed: {e}")),
                started.elapsed(),
            );
        }
    };
    // Read concurrently so a chatty child can never block on a full pipe.
    let mut stdout = child.stdout.take().expect("stdout piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let elapsed = started.elapsed();
    let Some(status) = status else {
        return (Outcome::TimedOut, elapsed);
    };
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        return (Outcome::Crashed(status.code()), elapsed);
    }
    let outcome = match serde_json::from_str::<Result<DriverReport, String>>(text.trim()) {
        Ok(Ok(report)) => Outcome::Report(Box::new(report)),
        Ok(Err(message)) => Outcome::DriverError(message),
        Err(e) => Outcome::BadOutput(format!("{e}: {text:?}")),
    };
    (outcome, elapsed)
}

/// Queries every installed driver, one sandboxed child at a time.
pub fn query_all(timeout: Duration) {
    let drivers = registry::installed_drivers();
    println!(
        "{} registered ASIO drivers; each queried in a child process (timeout {timeout:?})\n",
        drivers.len()
    );
    for driver in &drivers {
        let (outcome, elapsed) = query_sandboxed(driver, timeout);
        println!(
            "== {} ({:.0} ms)",
            driver.key_name,
            elapsed.as_secs_f64() * 1e3
        );
        match outcome {
            Outcome::Report(r) => {
                println!(
                    "   name: {} (version {}), init {:.1} ms",
                    r.driver_name, r.version, r.init_ms
                );
                println!(
                    "   channels: {} in / {} out, sample types: {}",
                    r.inputs,
                    r.outputs,
                    r.sample_types.join(", ")
                );
                if let Some(b) = r.buffer_sizes {
                    println!(
                        "   buffer: min {} / max {} / preferred {} / granularity {}",
                        b.min, b.max, b.preferred, b.granularity
                    );
                }
                let rates: Vec<String> = r.supported_rates.iter().map(u32::to_string).collect();
                println!(
                    "   rate: {:?}, supported: [{}]",
                    r.sample_rate,
                    rates.join(", ")
                );
                println!("   inputs: {}", preview(&r.input_names));
                println!("   outputs: {}", preview(&r.output_names));
                println!("   clocks: {}", r.clock_sources.join(", "));
            }
            Outcome::DriverError(message) => println!("   unavailable: {message}"),
            Outcome::TimedOut => println!("   TIMED OUT: child killed; parent unaffected"),
            Outcome::Crashed(code) => {
                println!("   CRASHED (exit code {code:?}); parent unaffected")
            }
            Outcome::BadOutput(message) => println!("   bad child output: {message}"),
        }
    }
}

fn preview(names: &[String]) -> String {
    const SHOWN: usize = 6;
    let mut text = names
        .iter()
        .take(SHOWN)
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > SHOWN {
        text.push_str(&format!(", … ({} total)", names.len()));
    }
    text
}
