//! Audio for the app: the device (through `lumen-audio`'s supervisor) with
//! the engine running in its callback. Until the device selector arrives
//! (M2 slice 8), the device is the system default output, or the one named
//! with `--audio`.

use lumen_audio::{AudioSystem, Backend, Block, DeviceEvent, DeviceId, StreamConfig, Timing};
use lumen_engine::{EngineHandle, Timeline, engine};

/// Which device to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The system default output (WASAPI shared on Windows).
    Default,
    /// `--audio <driver>:<name>`: `asio`, `wasapi` or `exclusive` on Windows,
    /// `cpal` elsewhere. ASIO names must match exactly; others match any
    /// part of the name.
    Named { driver: String, name: String },
}

impl Choice {
    /// Parses the `--audio` argument.
    pub fn parse(arg: &str) -> Result<Self, String> {
        let (driver, name) = arg
            .split_once(':')
            .ok_or_else(|| format!("--audio takes driver:name, not {arg:?}"))?;
        if backend(driver).is_none() {
            return Err(format!("unknown audio driver {driver:?} on this platform"));
        }
        Ok(Choice::Named {
            driver: driver.into(),
            name: name.into(),
        })
    }
}

/// What the transport bar shows about audio.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Starting,
    Running {
        device: String,
        detail: String,
    },
    /// The device went away; Lumen reconnects when it returns.
    Lost(String),
    Off(String),
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

const DEFAULT_DRIVER: &str = if cfg!(windows) { "wasapi" } else { "cpal" };

/// Finds the device and its display name. Enumeration runs on its own
/// thread, away from the UI thread's COM apartment.
fn resolve(choice: &Choice) -> Result<(MakeBackend, DeviceId, String), String> {
    let choice = choice.clone();
    std::thread::spawn(move || {
        let (driver, wanted) = match &choice {
            Choice::Default => (DEFAULT_DRIVER, None),
            Choice::Named { driver, name } => (driver.as_str(), Some(name.to_lowercase())),
        };
        let make = backend(driver).ok_or_else(|| format!("unknown audio driver {driver:?}"))?;
        #[cfg(windows)]
        if driver == "asio" {
            let wanted = wanted.unwrap_or_default();
            return lumen_audio::asio::AsioBackend::registered()
                .into_iter()
                .find(|(_, n)| n.to_lowercase() == wanted)
                .map(|(id, n)| (make, id, n))
                .ok_or_else(|| format!("no ASIO driver named {wanted:?}"));
        }
        let default = match wanted {
            Some(_) => None,
            #[cfg(windows)]
            None => lumen_audio::wasapi::WasapiBackend::default_output(),
            #[cfg(not(windows))]
            None => lumen_audio::cpal_host::CpalBackend::default_output(),
        };
        make()
            .devices()
            .into_iter()
            .find(|d| match (&wanted, &default) {
                (Some(w), _) => d.name.to_lowercase().contains(w),
                (None, Some(id)) => &d.id == id,
                (None, None) => false,
            })
            .map(|d| (make, d.id, d.name))
            .ok_or_else(|| match wanted {
                Some(w) => format!("no {driver} output matching {w:?}"),
                None => "no default audio output".into(),
            })
    })
    .join()
    .unwrap_or_else(|_| Err("audio device lookup failed".into()))
}

pub struct Audio {
    /// `None` when no device could be found; the engine then stays idle.
    system: Option<AudioSystem>,
    pub engine: EngineHandle,
    pub status: Status,
    device: String,
}

impl Audio {
    /// Starts the engine on the chosen device. Never fails: problems show up
    /// in [`Status`].
    pub fn start(choice: &Choice, timeline: Timeline) -> Self {
        let (engine_handle, mut engine) = engine(timeline, (), 48_000);
        let callback = move |block: &mut Block<'_>| {
            let (rate, channels, frames) = (block.sample_rate, block.output_channels, block.frames);
            engine.process(rate, block.outputs, channels, frames);
        };
        match resolve(choice) {
            Ok((make, device, name)) => {
                let system = AudioSystem::spawn(make, Box::new(callback), Timing::default());
                system.configure(StreamConfig {
                    device,
                    sample_rate: None,
                    buffer_frames: None,
                    input_channels: Vec::new(),
                    output_channels: vec![0, 1],
                });
                Self {
                    system: Some(system),
                    engine: engine_handle,
                    status: Status::Starting,
                    device: name,
                }
            }
            Err(message) => Self {
                system: None,
                engine: engine_handle,
                status: Status::Off(message),
                device: String::new(),
            },
        }
    }

    /// Applies device events. Call every frame; returns true if the status
    /// changed.
    pub fn poll(&mut self) -> bool {
        let Some(system) = &self.system else {
            return false;
        };
        let mut changed = false;
        while let Some(event) = system.try_event() {
            self.status = match event {
                DeviceEvent::Started(info)
                | DeviceEvent::Reset(info)
                | DeviceEvent::Restored(info) => Status::Running {
                    device: self.device.clone(),
                    detail: format!(
                        "{:.1} kHz · {} samples · {:.1} ms",
                        f64::from(info.sample_rate) / 1000.0,
                        info.buffer_frames,
                        info.round_trip_ms()
                    ),
                },
                DeviceEvent::Lost(error) => Status::Lost(error.to_string()),
                DeviceEvent::ConfigFailed { error, .. } => Status::Off(error.to_string()),
                DeviceEvent::Stopped => Status::Off("Audio is off".into()),
            };
            changed = true;
        }
        self.engine.collect_garbage();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audio_argument_names_a_driver_and_a_device() {
        assert_eq!(
            Choice::parse(&format!("{DEFAULT_DRIVER}:Speakers")),
            Ok(Choice::Named {
                driver: DEFAULT_DRIVER.into(),
                name: "Speakers".into()
            })
        );
        assert!(Choice::parse("Speakers").is_err());
        assert!(Choice::parse("nonsense:Speakers").is_err());
    }

    #[test]
    fn a_missing_device_leaves_audio_off_with_a_reason() {
        let audio = Audio::start(
            &Choice::Named {
                driver: DEFAULT_DRIVER.into(),
                name: "no such device anywhere".into(),
            },
            Timeline::default(),
        );
        match &audio.status {
            Status::Off(reason) => assert!(reason.contains("no such device"), "{reason}"),
            other => panic!("expected Off, got {other:?}"),
        }
    }
}
