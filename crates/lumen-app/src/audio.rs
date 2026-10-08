//! Audio for the app: the device (through `lumen-audio`'s supervisor) with
//! the engine running in its callback. The device comes from the audio
//! settings (saved), from `--audio`, or is the system default output; the
//! audio settings window changes it while Lumen runs (M2 slice 8).

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use lumen_audio::{
    AudioCallback, AudioError, AudioSystem, Backend, Block, DeviceEvent, DeviceId,
    Status as Meters, StreamConfig, StreamInfo, Timing,
};
use lumen_core::product::PRODUCT_NAME;
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
        detail: String,
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

pub fn driver_label(driver: &str) -> &str {
    DRIVERS
        .iter()
        .find(|(key, _)| *key == driver)
        .map_or(driver, |(_, label)| label)
}

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
        // A silent device for automated tests (`--audio test:fake`, the E2E
        // suite): it runs the engine a block every few milliseconds without
        // sound hardware. Not in `DRIVERS`, so users never see it.
        TEST_DRIVER => Some(|| {
            Box::new(lumen_audio::fake::FakeBackend(
                lumen_audio::fake::FakeControl::new(),
            ))
        }),
        _ => None,
    }
}

pub const DEFAULT_DRIVER: &str = if cfg!(windows) { "wasapi" } else { "cpal" };
/// The silent test device's driver key.
pub const TEST_DRIVER: &str = "test";

/// A device found for a choice.
struct Resolved {
    driver: String,
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
    fn found(driver: &str, make: MakeBackend, id: DeviceId, name: String) -> Self {
        Self {
            driver: driver.to_owned(),
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

    fn config(&self) -> StreamConfig {
        StreamConfig {
            device: self.id.clone(),
            sample_rate: self.sample_rate,
            buffer_frames: self.buffer_frames,
            input_channels: self.input_channels.clone(),
            output_channels: self.output_channels.clone(),
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
fn resolve(choice: &Choice) -> Result<Resolved, String> {
    let choice = choice.clone();
    std::thread::spawn(move || resolve_on_this_thread(&choice))
        .join()
        .unwrap_or_else(|_| Err("audio device lookup failed".into()))
}

/// The saved device, if it is there: by id, then by name.
fn find_saved(saved: &AudioSettings) -> Option<Resolved> {
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
        output_channels: saved.output_channels.clone(),
        ..Resolved::found(&saved.driver, make, id, name)
    })
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

pub struct Audio {
    /// `None` until a device has been found; the engine then waits in
    /// `parked`.
    system: Option<AudioSystem>,
    /// The engine's callback while no audio system runs it.
    parked: Option<Box<dyn AudioCallback>>,
    pub engine: EngineHandle,
    /// Replaces the clips the engine plays.
    pub clips: ClipsHandle,
    pub status: Status,
    /// The driver model in use (`asio`, `wasapi`, ...).
    pub driver: String,
    pub device: String,
    /// What "Audio on" opens again.
    config: Option<StreamConfig>,
    /// The device's latencies and block size, once running.
    pub info: Option<StreamInfo>,
    /// The last device error, to tell "busy" apart from other failures.
    pub last_error: Option<AudioError>,
    /// Settings chosen in the audio settings window, waiting for the device
    /// to start before they are saved.
    pending: Option<AudioSettings>,
    /// Settings that worked: the app saves them.
    pub confirmed: Option<AudioSettings>,
    /// Something the user should know about the device (shown in the audio
    /// panel), such as a saved device that was missing.
    pub notice: Option<String>,
    /// The driver's own words behind `notice`, shown on hover.
    pub notice_detail: Option<String>,
    /// The setup in use before `apply`, to return to if the new one fails.
    previous: Option<Previous>,
    /// A control-panel request the driver has not answered yet.
    control_panel: Option<Receiver<Result<(), String>>>,
    /// Why the control panel did not open, if it did not.
    pub control_panel_error: Option<String>,
}

/// A setup to return to.
struct Previous {
    driver: String,
    device: String,
    config: StreamConfig,
    info: Option<StreamInfo>,
    status: Status,
}

impl Audio {
    /// Starts the engine on the chosen device. Never fails: problems show up
    /// in [`Status`].
    pub fn start(choice: &Choice, timeline: Timeline) -> Self {
        let (clips, player) = clip_player();
        let (engine_handle, mut engine) = engine(timeline, player, 48_000);
        let callback: Box<dyn AudioCallback> = Box::new(move |block: &mut Block<'_>| {
            let (rate, channels, frames) = (block.sample_rate, block.output_channels, block.frames);
            engine.process(rate, block.outputs, channels, frames);
        });
        let mut audio = Self {
            system: None,
            parked: Some(callback),
            engine: engine_handle,
            clips,
            status: Status::Off(String::new()),
            driver: String::new(),
            device: String::new(),
            config: None,
            info: None,
            last_error: None,
            pending: None,
            confirmed: None,
            notice: None,
            notice_detail: None,
            previous: None,
            control_panel: None,
            control_panel_error: None,
        };
        match resolve(choice) {
            Ok(found) => {
                audio.notice = found.notice.clone();
                audio.open(found);
            }
            Err(message) => audio.status = Status::Off(message),
        }
        audio
    }

    /// Opens `found`: reconfigures the running driver model, switches to
    /// another, or starts the first audio system.
    fn open(&mut self, found: Resolved) {
        let config = found.config();
        if let Some(system) = &mut self.system {
            if found.driver == self.driver {
                system.configure(config.clone());
            } else if let Err(e) = system.switch(found.make, config.clone()) {
                self.status = Status::Off(format!("Could not switch the audio driver: {e}"));
                return;
            }
        } else if let Some(callback) = self.parked.take() {
            let system = AudioSystem::spawn(found.make, callback, Timing::default());
            system.configure(config.clone());
            self.system = Some(system);
        }
        self.driver = found.driver;
        self.device = found.name;
        self.config = Some(config);
        self.info = None;
        self.last_error = None;
        self.status = Status::Starting;
    }

    /// Uses `settings` (from the audio settings window). Saved once the
    /// device starts; on failure the reason shows and nothing is saved.
    pub fn apply(&mut self, settings: AudioSettings) {
        let lookup = settings.clone();
        let found = std::thread::spawn(move || find_saved(&lookup))
            .join()
            .ok()
            .flatten();
        match found {
            Some(found) => {
                self.dismiss_notice();
                self.previous = self.config.clone().map(|config| Previous {
                    driver: self.driver.clone(),
                    device: self.device.clone(),
                    config,
                    info: self.info.clone(),
                    status: self.status.clone(),
                });
                self.pending = Some(settings);
                self.open(found);
            }
            None => {
                self.notice = Some(format!("{} is not available", settings.device_name));
            }
        }
    }

    /// The setup in use, as settings.
    pub fn settings(&self) -> Option<AudioSettings> {
        let config = self.config.as_ref()?;
        Some(AudioSettings {
            driver: self.driver.clone(),
            device_id: config.device.id.clone(),
            device_name: self.device.clone(),
            sample_rate: config.sample_rate,
            buffer_frames: config.buffer_frames,
            input_channels: config.input_channels.clone(),
            output_channels: config.output_channels.clone(),
        })
    }

    /// CPU load, overloads and dropouts, when a device was found.
    pub fn meters(&self) -> Option<&Arc<Meters>> {
        self.system.as_ref().map(AudioSystem::status)
    }

    /// Asks the running ASIO driver to open its own settings window. The
    /// answer comes back through `poll`.
    pub fn open_control_panel(&mut self) {
        self.control_panel_error = None;
        match &self.system {
            Some(system) => self.control_panel = Some(system.request_control_panel()),
            None => self.control_panel_error = Some("audio is not running".into()),
        }
    }

    /// The control panel was asked for and the driver has not answered.
    pub fn control_panel_opening(&self) -> bool {
        self.control_panel.is_some()
    }

    /// What is being changed right now, while the device restarts: shown so
    /// a slow change (an interface re-locking its clock) reads as progress.
    pub fn changing(&self) -> Option<String> {
        if !matches!(self.status, Status::Starting) {
            return None;
        }
        let pending = self.pending.as_ref()?;
        let previous = self.previous.as_ref();
        let was = |f: fn(&StreamConfig) -> Option<u32>| previous.and_then(|p| f(&p.config));
        if previous.is_some_and(|p| p.config.device.id == pending.device_id)
            && pending.sample_rate.is_some()
            && pending.sample_rate
                != was(|c| c.sample_rate)
                    .or(previous.and_then(|p| p.info.as_ref().map(|i| i.sample_rate)))
        {
            let rate = f64::from(pending.sample_rate.unwrap_or_default()) / 1000.0;
            return Some(format!(
                "Changing to {rate:.1} kHz… the interface is re-locking its clock"
            ));
        }
        Some(format!("Starting {}…", pending.device_name))
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
        self.notice_detail = None;
    }

    /// Goes back to the setup used before a change that failed.
    fn restore_previous(&mut self, failed: &AudioError) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        let tried = std::mem::replace(&mut self.device, previous.device.clone());
        self.notice = Some(format!(
            "{tried} could not start. Still using {}.",
            previous.device
        ));
        self.notice_detail = Some(failed.to_string());
        if previous.driver == self.driver {
            // The supervisor already reopened it.
            self.config = Some(previous.config);
            self.info = previous.info;
            self.status = previous.status;
        } else if let Some(make) = backend(&previous.driver) {
            // A failed driver switch: switch back.
            let resolved = Resolved {
                driver: previous.driver.clone(),
                make,
                id: previous.config.device.clone(),
                name: previous.device.clone(),
                sample_rate: previous.config.sample_rate,
                buffer_frames: previous.config.buffer_frames,
                input_channels: previous.config.input_channels.clone(),
                output_channels: previous.config.output_channels.clone(),
                notice: None,
            };
            let notice = self.notice.take();
            let detail = self.notice_detail.take();
            self.open(resolved);
            self.notice = notice;
            self.notice_detail = detail;
        }
    }

    /// Whether the user can turn audio on or off (a device was found).
    pub fn can_switch(&self) -> bool {
        self.config.is_some()
    }

    pub fn is_on(&self) -> bool {
        !matches!(self.status, Status::Off(_))
    }

    /// Another application holds the device.
    pub fn is_busy(&self) -> bool {
        matches!(self.status, Status::Off(_))
            && matches!(self.last_error, Some(AudioError::DeviceBusy(_)))
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
        // Taken first: handling a failure may reopen the previous setup.
        let events: Vec<DeviceEvent> = std::iter::from_fn(|| system.try_event()).collect();
        let mut changed = false;
        for event in events {
            match event {
                DeviceEvent::Started(info)
                | DeviceEvent::Reset(info)
                | DeviceEvent::Restored(info) => {
                    self.status = Status::Running {
                        device: self.device.clone(),
                        detail: format!(
                            "{:.1} kHz · {} samples · {:.1} ms",
                            f64::from(info.sample_rate) / 1000.0,
                            info.buffer_frames,
                            info.round_trip_ms()
                        ),
                    };
                    self.info = Some(info);
                    self.last_error = None;
                    self.previous = None;
                    if let Some(settings) = self.pending.take() {
                        self.confirmed = Some(settings);
                    }
                }
                DeviceEvent::Lost(error) => {
                    self.status = Status::Lost(error.to_string());
                    self.last_error = Some(error);
                }
                DeviceEvent::ConfigFailed { error, reverted } => {
                    self.pending = None;
                    let can_restore = self.previous.is_some();
                    if reverted || can_restore {
                        // Back to the setup that worked, saying why.
                        self.restore_previous(&error);
                    } else {
                        self.status = Status::Off(error.to_string());
                    }
                    self.last_error = Some(error);
                }
                DeviceEvent::Stopped => self.status = Status::Off("Audio is off".into()),
            }
            changed = true;
        }
        if let Some(rx) = &self.control_panel
            && let Ok(result) = rx.try_recv()
        {
            self.control_panel = None;
            self.control_panel_error = result.err();
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
        assert!(audio.settings().is_none());
    }

    #[test]
    fn every_listed_driver_has_a_backend_and_a_label() {
        for (key, label) in DRIVERS {
            assert!(backend(key).is_some(), "{key}");
            assert_eq!(driver_label(key), *label);
        }
        assert!(DRIVERS.iter().any(|(key, _)| *key == DEFAULT_DRIVER));
    }
}
