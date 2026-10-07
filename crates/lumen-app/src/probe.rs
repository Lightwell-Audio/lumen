//! Safe ASIO driver queries (M2 slice 8, risk R6). Asking an ASIO driver
//! what it offers means loading its code into the process, and some drivers
//! hang on a dialog or crash when their hardware is missing. So the app asks
//! a helper: itself, started as `lumen --probe-asio <clsid>`, which loads
//! the one driver, prints what it offers as JSON and exits. A helper that
//! crashes or takes longer than the timeout is reported as an unresponsive
//! driver; Lumen itself is never at risk.

use std::time::{Duration, Instant};

use lumen_audio::{BufferSizes, DeviceId, DeviceInfo, DriverKind};
use serde::{Deserialize, Serialize};

pub const FLAG: &str = "--probe-asio";
const TIMEOUT: Duration = Duration::from_secs(5);

/// What the helper reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probed {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub rates: Vec<u32>,
    pub current_rate: Option<u32>,
    /// min, max, preferred, granularity (absent: powers of two).
    pub buffers: Option<(u32, u32, u32, Option<u32>)>,
}

impl Probed {
    pub fn from_info(info: &DeviceInfo) -> Self {
        Self {
            name: info.name.clone(),
            inputs: info.input_channels.clone(),
            outputs: info.output_channels.clone(),
            rates: info.sample_rates.clone(),
            current_rate: info.current_sample_rate,
            buffers: info
                .buffer_sizes
                .map(|b| (b.min, b.max, b.preferred, b.granularity)),
        }
    }

    pub fn into_info(self, clsid: &str) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId {
                kind: DriverKind::Asio,
                id: clsid.to_owned(),
            },
            name: self.name,
            input_channels: self.inputs,
            output_channels: self.outputs,
            sample_rates: self.rates,
            current_sample_rate: self.current_rate,
            buffer_sizes: self
                .buffers
                .map(|(min, max, preferred, granularity)| BufferSizes {
                    min,
                    max,
                    preferred,
                    granularity,
                }),
        }
    }
}

/// The helper's side: probe `clsid`, print JSON, return the exit code.
#[cfg(windows)]
pub fn run_helper(clsid: &str) -> i32 {
    use lumen_audio::asio::AsioBackend;
    let Some((id, name)) = AsioBackend::registered()
        .into_iter()
        .find(|(id, _)| id.id == clsid)
    else {
        eprintln!("no ASIO driver {clsid}");
        return 2;
    };
    let backend = match AsioBackend::new() {
        Ok(backend) => backend,
        Err(e) => {
            eprintln!("{e}");
            return 3;
        }
    };
    let info = match backend.probe(&id, name) {
        Ok(info) => info,
        Err(e) => {
            // The reason goes to stdout so the app can show it.
            println!("{e}");
            return 5;
        }
    };
    match serde_json::to_string(&Probed::from_info(&info)) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            4
        }
    }
}

#[cfg(not(windows))]
pub fn run_helper(_clsid: &str) -> i32 {
    eprintln!("ASIO exists only on Windows");
    2
}

/// Asks a helper process about `clsid`. Blocks up to the timeout, so call
/// it from a background thread.
pub fn probe_asio(clsid: &str) -> Result<DeviceInfo, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .args([FLAG, clsid])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start the driver check: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read;
                    let _ = stdout.read_to_string(&mut out);
                }
                if !status.success() {
                    return Err(match status.code() {
                        Some(2) => "the driver is not installed".into(),
                        Some(5) => out.trim().to_owned(),
                        _ => "the driver crashed while being checked".into(),
                    });
                }
                return serde_json::from_str::<Probed>(out.trim())
                    .map(|p| p.into_info(clsid))
                    .map_err(|e| format!("the driver check gave an unreadable answer: {e}"));
            }
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the driver did not respond within {} s (it may be waiting on hardware \
                     that is not connected)",
                    TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_results_survive_the_trip_through_json() {
        let info = DeviceInfo {
            id: DeviceId {
                kind: DriverKind::Asio,
                id: "{ABC}".into(),
            },
            name: "Interface".into(),
            input_channels: vec!["Mic 1".into(), "Mic 2".into()],
            output_channels: vec!["Out 1".into(), "Out 2".into()],
            sample_rates: vec![44_100, 48_000],
            current_sample_rate: Some(48_000),
            buffer_sizes: Some(BufferSizes {
                min: 32,
                max: 1_024,
                preferred: 128,
                granularity: None,
            }),
        };
        let json = serde_json::to_string(&Probed::from_info(&info)).unwrap();
        let back = serde_json::from_str::<Probed>(&json)
            .unwrap()
            .into_info("{ABC}");
        assert_eq!(back, info);
    }
}
