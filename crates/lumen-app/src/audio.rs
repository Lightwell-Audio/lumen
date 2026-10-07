//! Audio for the app: the device (through `lumen-audio`'s supervisor) with
//! the engine running in its callback. Until the device selector arrives
//! (M2 slice 8), the device is the system default output, or the one named
//! with `--audio`.

use std::sync::Arc;

use lumen_core::product::PRODUCT_NAME;

use lumen_audio::{
    AudioSystem, Backend, Block, DeviceEvent, DeviceId, Status as Meters, StreamConfig, Timing,
};
use lumen_engine::{ClipsHandle, EngineHandle, Timeline, clip_player, engine};

use crate::settings::AudioSettings;

/// Which device to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The system default output (WASAPI shared on Windows).
    Default,
    /// `--audio <driver>:<name>`: `asio`, `wasapi` or `exclusive` on Windows,
    /// `cpal` elsewhere. ASIO names must match exactly; others match any
    /// part of the name.
    Named { driver: String, name: String },
    /// The device saved in the settings: found by id, then by name; if it
    /// is missing, the default output with a note saying so.
    Saved(AudioSettings),
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
/// A device found for a choice.
struct Resolved {
    make: MakeBackend,
    id: DeviceId,
    name: String,
    /// Rate, buffer and channels: saved ones, or defaults.
    sample_rate: Option<u32>,
    buffer_frames: Option<u32>,
    input_channels: Vec<usize>,
    output_channels: Vec<usize>,
    /// Something the user should know (the saved device was missing).
    notice: Option<String>,
}

impl Resolved {
    fn found(make: MakeBackend, id: DeviceId, name: String) -> Self {
        Self {
            make,
            id,
            name,
            sample_rate: None,
            buffer_frames: None,
            input_channels: Vec::new(),
            output_channels: vec![0, 1],
            notice: None,
        }
    }
}

/// (id, name) of each device of `driver`, without loading ASIO drivers.
fn listed(driver: &str, make: MakeBackend) -> Vec<(DeviceId, String)> {
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

/// Finds the device and its display name. Enumeration runs on its own
/// thread, away from the UI thread's COM apartment.
fn resolve(choice: &Choice) -> Result<Resolved, String> {
    let choice = choice.clone();
    std::thread::spawn(move || resolve_on_this_thread(&choice))
        .join()
        .unwrap_or_else(|_| Err("audio device lookup failed".into()))
}

fn resolve_on_this_thread(choice: &Choice) -> Result<Resolved, String> {
    match choice {
        Choice::Default => {
            let make = backend(DEFAULT_DRIVER).ok_or("no audio driver on this platform")?;
            #[cfg(windows)]
            let default = lumen_audio::wasapi::WasapiBackend::default_output();
            #[cfg(not(windows))]
            let default = lumen_audio::cpal_host::CpalBackend::default_output();
            let default = default.ok_or("no default audio output")?;
            listed(DEFAULT_DRIVER, make)
                .into_iter()
                .find(|(id, _)| *id == default)
                .map(|(id, name)| Resolved::found(make, id, name))
                .ok_or_else(|| "no default audio output".into())
        }
        Choice::Named { driver, name } => {
            let make = backend(driver).ok_or_else(|| format!("unknown audio driver {driver:?}"))?;
            let wanted = name.to_lowercase();
            listed(driver, make)
                .into_iter()
                .find(|(_, n)| {
                    let n = n.to_lowercase();
                    if driver == "asio" {
                        n == wanted
                    } else {
                        n.contains(&wanted)
                    }
                })
                .map(|(id, name)| Resolved::found(make, id, name))
                .ok_or_else(|| format!("no {driver} device matching {name:?}"))
        }
        Choice::Saved(saved) => {
            let found = backend(&saved.driver).and_then(|make| {
                let devices = listed(&saved.driver, make);
                devices
                    .iter()
                    .find(|(id, _)| id.id == saved.device_id)
                    .or_else(|| devices.iter().find(|(_, n)| *n == saved.device_name))
                    .cloned()
                    .map(|(id, name)| (make, id, name))
            });
            match found {
                Some((make, id, name)) => Ok(Resolved {
                    sample_rate: saved.sample_rate,
                    buffer_frames: saved.buffer_frames,
                    input_channels: saved.input_channels.clone(),
                    output_channels: saved.output_channels.clone(),
                    ..Resolved::found(make, id, name)
                }),
                None => {
                    let mut fallback = resolve_on_this_thread(&Choice::Default)?;
                    fallback.notice = Some(format!(
                        "{} is not connected, so {} is using {} for now. Connect it and \
                         choose it again in the audio settings",
                        saved.device_name, PRODUCT_NAME, fallback.name
                    ));
                    Ok(fallback)
                }
            }
        }
    }
}

pub struct Audio {
    /// `None` when no device could be found; the engine then stays idle.
    system: Option<AudioSystem>,
    pub engine: EngineHandle,
    /// Replaces the clips the engine plays.
    pub clips: ClipsHandle,
    pub status: Status,
    device: String,
    /// What "Audio on" opens again.
    config: Option<StreamConfig>,
    /// Something the user should know about the device (shown in the audio
    /// panel), such as a saved device that was missing.
    pub notice: Option<String>,
}

impl Audio {
    /// Starts the engine on the chosen device. Never fails: problems show up
    /// in [`Status`].
    pub fn start(choice: &Choice, timeline: Timeline) -> Self {
        let (clips, player) = clip_player();
        let (engine_handle, mut engine) = engine(timeline, player, 48_000);
        let callback = move |block: &mut Block<'_>| {
            let (rate, channels, frames) = (block.sample_rate, block.output_channels, block.frames);
            engine.process(rate, block.outputs, channels, frames);
        };
        match resolve(choice) {
            Ok(found) => {
                let system = AudioSystem::spawn(found.make, Box::new(callback), Timing::default());
                let config = StreamConfig {
                    device: found.id,
                    sample_rate: found.sample_rate,
                    buffer_frames: found.buffer_frames,
                    input_channels: found.input_channels,
                    output_channels: found.output_channels,
                };
                system.configure(config.clone());
                Self {
                    system: Some(system),
                    config: Some(config),
                    engine: engine_handle,
                    clips,
                    status: Status::Starting,
                    device: found.name,
                    notice: found.notice,
                }
            }
            Err(message) => Self {
                system: None,
                engine: engine_handle,
                clips,
                status: Status::Off(message),
                device: String::new(),
                config: None,
                notice: None,
            },
        }
    }

    /// CPU load, overloads and dropouts, when a device was found.
    pub fn meters(&self) -> Option<&Arc<Meters>> {
        self.system.as_ref().map(AudioSystem::status)
    }

    /// Whether the user can turn audio on or off (a device was found).
    pub fn can_switch(&self) -> bool {
        self.config.is_some()
    }

    pub fn is_on(&self) -> bool {
        !matches!(self.status, Status::Off(_))
    }

    /// The Audio On/Off switch: closes the device, or opens it again.
    pub fn set_on(&mut self, on: bool) {
        let (Some(system), Some(config)) = (&self.system, &self.config) else {
            return;
        };
        if on {
            system.configure(config.clone());
            self.status = Status::Starting;
        } else {
            system.stop();
            self.status = Status::Off("Audio is off".into());
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
        self.clips.collect_garbage();
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
