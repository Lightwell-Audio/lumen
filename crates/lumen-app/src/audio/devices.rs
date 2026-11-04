//! Which device to open (M9 review M1): the user's choice, the driver
//! models of this platform, the backend each one makes, and finding the
//! device a choice names (a saved device, `--audio`, or the system's
//! default), with the fallback when it is missing.

use std::time::Duration;

use lumen_audio::{Backend, DeviceId, StreamConfig};
use lumen_core::product::PRODUCT_NAME;

use super::test_devices::{self, TEST_DRIVER, TEST_DRIVERS, TEST_INPUT_DRIVER, is_test};
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

    /// The silent test device with scripted inputs ([`TEST_INPUT_DRIVER`]).
    pub fn test_input_device() -> Self {
        Choice::Named {
            driver: TEST_INPUT_DRIVER.into(),
            name: "fake".into(),
        }
    }

    /// The silent test device ([`TEST_DRIVER`]).
    pub fn test_device() -> Self {
        Choice::Named {
            driver: TEST_DRIVER.into(),
            name: "fake".into(),
        }
    }
}

/// What the transport bar shows about audio.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Starting,
    Running {
        device: String,
        /// The rate, buffer and round trip, as the tooltip shows them.
        detail: String,
        /// The rate in kHz, as the chip shows it ("48.0 kHz").
        rate: String,
    },
    /// The device went away; Lumen reconnects when it returns.
    Lost(String),
    Off(String),
}

pub type MakeBackend = fn() -> Box<dyn Backend>;

/// The driver models on this platform: (key, label).
pub const DRIVERS: &[(&str, &str)] = if cfg!(windows) {
    &[
        ("asio", "ASIO"),
        ("wasapi", "Windows audio (shared)"),
        ("exclusive", "Windows audio (exclusive)"),
    ]
} else {
    &[("cpal", "System audio")]
};

/// The driver models to offer while `driver` is in use.
pub fn drivers_for(driver: &str) -> &'static [(&'static str, &'static str)] {
    if is_test(driver) {
        TEST_DRIVERS
    } else {
        DRIVERS
    }
}

pub fn driver_label(driver: &str) -> &str {
    DRIVERS
        .iter()
        .chain(TEST_DRIVERS)
        .find(|(key, _)| *key == driver)
        .map_or(driver, |(_, label)| label)
}

/// What a message calls the driver behind `device`: an ASIO driver by its
/// own name ("Focusrite USB ASIO"), the others by their driver model.
pub(super) fn driver_name(driver: &str, device: &str) -> String {
    if driver == "asio" {
        device.to_owned()
    } else {
        driver_label(driver).to_owned()
    }
}

/// How long a driver may take to answer before the row says it is not
/// answering: as long as a driver switch waits for its handover.
pub(super) const NOT_ANSWERING: Duration = lumen_audio::HANDOVER_TIMEOUT;

pub fn backend(driver: &str) -> Option<MakeBackend> {
    match driver {
        #[cfg(windows)]
        // If ASIO cannot start here (COM), every open fails with the reason
        // and the app shows it, rather than the audio thread panicking.
        "asio" => Some(|| match lumen_audio::asio::AsioBackend::new() {
            Ok(backend) => Box::new(backend),
            Err(e) => Box::new(lumen_audio::Unavailable::new(format!(
                "ASIO could not start: {e}"
            ))),
        }),
        #[cfg(windows)]
        "wasapi" => Some(|| Box::new(lumen_audio::wasapi::WasapiBackend::new(false))),
        #[cfg(windows)]
        "exclusive" => Some(|| Box::new(lumen_audio::wasapi::WasapiBackend::new(true))),
        #[cfg(not(windows))]
        "cpal" => Some(|| Box::new(lumen_audio::cpal_host::CpalBackend::new())),
        _ => test_devices::backend(driver),
    }
}

pub const DEFAULT_DRIVER: &str = if cfg!(windows) { "wasapi" } else { "cpal" };

/// Why audio is off after Audio On/Off turned it off.
pub(super) const AUDIO_OFF: &str = "Audio is off";
/// A device found for a choice.
pub(super) struct Resolved {
    pub(super) driver: String,
    pub(super) make: MakeBackend,
    pub(super) id: DeviceId,
    pub(super) name: String,
    /// Rate, buffer and channels: saved ones, or defaults.
    pub(super) sample_rate: Option<u32>,
    pub(super) buffer_frames: Option<u32>,
    pub(super) input_channels: Vec<usize>,
    /// Every input the device has (`input_channels` is then empty).
    pub(super) all_inputs: bool,
    pub(super) output_channels: Vec<usize>,
    /// The cue pair, opened after the main pair (empty: none).
    pub(super) cue_channels: Vec<usize>,
    /// Something the user should know (the saved device was missing).
    pub(super) notice: Option<String>,
}

impl Resolved {
    pub(super) fn found(driver: &str, make: MakeBackend, id: DeviceId, name: String) -> Self {
        Self {
            driver: driver.to_owned(),
            make,
            id,
            name,
            sample_rate: None,
            buffer_frames: None,
            input_channels: Vec::new(),
            all_inputs: true,
            output_channels: vec![0, 1],
            cue_channels: Vec::new(),
            notice: None,
        }
    }

    /// The stream to open: the main pair, then the cue pair, so the cue is
    /// always on the third and fourth channels the engine sees.
    pub(super) fn config(&self) -> StreamConfig {
        let cue = self
            .cue_channels
            .iter()
            .filter(|c| !self.output_channels.contains(c));
        StreamConfig {
            device: self.id.clone(),
            sample_rate: self.sample_rate,
            buffer_frames: self.buffer_frames,
            input_channels: if self.all_inputs {
                vec![lumen_audio::ALL_INPUTS]
            } else {
                self.input_channels.clone()
            },
            output_channels: self.output_channels.iter().chain(cue).copied().collect(),
        }
    }
}

/// (id, name) of each device of `driver`, without loading ASIO drivers.
pub fn listed(driver: &str, make: MakeBackend) -> Vec<(DeviceId, String)> {
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
pub(super) fn resolve(choice: &Choice) -> Result<Resolved, String> {
    let choice = choice.clone();
    std::thread::spawn(move || resolve_on_this_thread(&choice))
        .join()
        .unwrap_or_else(|_| Err("audio device lookup failed".into()))
}

/// The saved device, if it is there: by id, then by name.
pub(super) fn find_saved(saved: &AudioSettings) -> Option<Resolved> {
    let make = backend(&saved.driver)?;
    let devices = listed(&saved.driver, make);
    let (id, name) = devices
        .iter()
        .find(|(id, _)| id.id == saved.device_id)
        .or_else(|| devices.iter().find(|(_, n)| *n == saved.device_name))
        .cloned()?;
    Some(Resolved {
        sample_rate: saved.sample_rate,
        buffer_frames: saved.buffer_frames,
        input_channels: saved.input_channels.clone(),
        all_inputs: saved.all_inputs,
        output_channels: saved.output_channels.clone(),
        cue_channels: saved.cue_channels.clone(),
        ..Resolved::found(&saved.driver, make, id, name)
    })
}

pub(super) fn resolve_on_this_thread(choice: &Choice) -> Result<Resolved, String> {
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
                .map(|(id, name)| Resolved::found(DEFAULT_DRIVER, make, id, name))
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
                .map(|(id, name)| Resolved::found(driver, make, id, name))
                .ok_or_else(|| format!("no {driver} device matching {name:?}"))
        }
        Choice::Saved(saved) => match find_saved(saved) {
            Some(found) => Ok(found),
            None => {
                let mut fallback = resolve_on_this_thread(&Choice::Default)?;
                fallback.notice = Some(format!(
                    "{} is not connected, so {} is using {} for now. Connect it and \
                     choose it again in the audio settings",
                    saved.device_name, PRODUCT_NAME, fallback.name
                ));
                Ok(fallback)
            }
        },
    }
}
