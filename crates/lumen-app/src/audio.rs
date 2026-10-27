//! Audio for the app: the device (through `lumen-audio`'s supervisor) with
//! the engine running in its callback. The device comes from the audio
//! settings (saved), from `--audio`, or is the system default output; the
//! audio settings window changes it while Lumen runs (M2 slice 8).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use lumen_audio::fake::Pace;
use lumen_audio::{
    AudioCallback, AudioError, AudioSystem, Backend, Block, DeviceEvent, DeviceId,
    Status as Meters, StreamConfig, StreamInfo, Timing,
};
use lumen_core::product::PRODUCT_NAME;
use lumen_engine::{
    ClipsHandle, EngineHandle, InputHandle, Layered, MidiHandle, MixerHandle, Timeline,
    clip_player, engine, input_player, midi_player, mixer,
};

use crate::export_live;
use crate::midi_out::StreamClock;
use crate::settings::AudioSettings;
use crate::take_worker::TakeWorker;

/// What the test device's tap holds: 40 seconds of the fake device's
/// playback (it runs at about a quarter of real time), left and right.
const HEARD: usize = 1 << 20;

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
        TEST_DRIVER => Some(|| test_backend(Pace::Every(TEST_BLOCK_EVERY))),
        // The same, in real time (see `FakeDevice::pace`): for scenarios that
        // map wall-clock stamps to frames (recording).
        TEST_PACED_DRIVER => Some(|| test_backend(Pace::RealTime)),
        // The silent test device whose inputs a scenario plays (one per
        // process: the scenarios that use it take turns).
        TEST_INPUT_DRIVER => Some(|| Box::new(lumen_audio::fake::FakeBackend(input_device()))),
        _ => None,
    }
}

/// How often the silent test device runs a block (the supervisor polls every
/// 10 ms): a busy machine that polls late gets the blocks it missed.
const TEST_BLOCK_EVERY: Duration = Duration::from_millis(10);

fn test_backend(pace: Pace) -> Box<dyn Backend> {
    let control = lumen_audio::fake::FakeControl::new();
    control.set(|device| device.pace = Some(pace));
    Box::new(lumen_audio::fake::FakeBackend(control))
}

/// The one device behind [`TEST_INPUT_DRIVER`]: a scenario sets its
/// `input_levels` while the stream runs. Scenarios that script it take
/// [`input_device_turn`], which hands it over clean.
pub fn input_device() -> lumen_audio::fake::FakeControl {
    static DEVICE: std::sync::OnceLock<lumen_audio::fake::FakeControl> = std::sync::OnceLock::new();
    DEVICE
        .get_or_init(|| {
            let control = lumen_audio::fake::FakeControl::new();
            as_a_test_device(&control);
            control
        })
        .clone()
}

/// What makes a fake device the test input device: its slow, steady pace.
fn as_a_test_device(control: &lumen_audio::fake::FakeControl) {
    control.set(|device| device.pace = Some(Pace::Every(TEST_BLOCK_EVERY)));
}

/// A scenario's turn on the input test device, which every scenario that
/// plays it shares (it is one device per process). Held for the scenario's
/// life: the next one waits for it to be dropped.
pub struct InputDeviceTurn {
    device: lumen_audio::fake::FakeControl,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl InputDeviceTurn {
    /// Scripts the device (`FakeDevice::input_levels`, `input_ramp`, ...).
    pub fn set(&self, f: impl FnOnce(&mut lumen_audio::fake::FakeDevice)) {
        self.device.set(f);
    }
}

/// Waits for the input test device and takes it, put back as it first was:
/// nothing a scenario before this one scripted (a ramp, levels, a missing
/// device) is left on it, whichever scenario ran before and whatever it set.
pub fn input_device_turn() -> InputDeviceTurn {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A scenario that failed holds the turn while it unwinds; the device is
    // reset below, so a poisoned turn is as good as a free one.
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let device = input_device();
    device.reset();
    as_a_test_device(&device);
    InputDeviceTurn {
        device,
        _turn: turn,
    }
}

/// Whether `driver` is one of the silent test devices.
fn is_test(driver: &str) -> bool {
    driver == TEST_DRIVER || driver == TEST_PACED_DRIVER || driver == TEST_INPUT_DRIVER
}

pub const DEFAULT_DRIVER: &str = if cfg!(windows) { "wasapi" } else { "cpal" };
/// The silent test device's driver key.
pub const TEST_DRIVER: &str = "test";
/// The silent test device that keeps to real time.
pub const TEST_PACED_DRIVER: &str = "test-paced";
/// The silent test device with inputs a scenario plays ([`input_device`]).
pub const TEST_INPUT_DRIVER: &str = "test-input";

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
    /// Every input the device has (`input_channels` is then empty).
    all_inputs: bool,
    output_channels: Vec<usize>,
    /// The cue pair, opened after the main pair (empty: none).
    cue_channels: Vec<usize>,
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
            all_inputs: true,
            output_channels: vec![0, 1],
            cue_channels: Vec::new(),
            notice: None,
        }
    }

    /// The stream to open: the main pair, then the cue pair, so the cue is
    /// always on the third and fourth channels the engine sees.
    fn config(&self) -> StreamConfig {
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
        all_inputs: saved.all_inputs,
        output_channels: saved.output_channels.clone(),
        cue_channels: saved.cue_channels.clone(),
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

/// Copies a block of planar output into the test tap, left and right per
/// frame (mono twice). Whole blocks only, so a full tap leaves a gap
/// between blocks, never inside one. Real-time safe.
fn listen(tap: &mut rtrb::Producer<f32>, outputs: &[f32], channels: usize, frames: usize) {
    if channels == 0 {
        return;
    }
    let Ok(chunk) = tap.write_chunk_uninit(4 * frames) else {
        return;
    };
    // The main pair, then the cue pair (silent when there is none).
    let right = usize::from(channels > 1) * frames;
    let at = |channel: usize, i: usize| {
        if channel < channels {
            outputs[channel * frames + i]
        } else {
            0.0
        }
    };
    chunk.fill_from_iter(
        (0..frames).flat_map(|i| [outputs[i], outputs[right + i], at(2, i), at(3, i)]),
    );
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
    /// Replaces the MIDI tracks it plays (M5).
    pub midi: MidiHandle,
    /// Where the stream is and when, for timing MIDI to hardware.
    pub clock: Arc<StreamClock>,
    /// Replaces the routing they play through (M4).
    pub mixer: MixerHandle,
    /// Where audio tracks take their input from, and what the inputs read
    /// (M9 slice R1).
    pub inputs: InputHandle,
    /// Writes what recording audio tracks take to disk (M9 slice R2).
    pub(crate) takes: TakeWorker,
    /// Installs callback-driven real-time export sessions from worker threads.
    pub(crate) export: Arc<Mutex<export_live::Handle>>,
    /// What the test device played, left and right frame by frame, for the
    /// end-to-end tests to listen to (`testing`). Real devices have none.
    pub(crate) heard: Option<rtrb::Consumer<f32>>,
    pub status: Status,
    /// The driver model in use (`asio`, `wasapi`, ...).
    pub driver: String,
    pub device: String,
    /// What "Audio on" opens again.
    config: Option<StreamConfig>,
    /// The cue pair within `config`'s outputs (after the main pair).
    cue_channels: Vec<usize>,
    /// Further device outputs, after the cue pair: pairs something in the
    /// project plays to (M4 slice 5, the owner's B6).
    extra_channels: Vec<usize>,
    /// Further outputs being opened now, and ones the device refused.
    trying_extra: Option<Vec<usize>>,
    refused_extra: Option<Vec<usize>>,
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
    cue_channels: Vec<usize>,
    extra_channels: Vec<usize>,
    info: Option<StreamInfo>,
    status: Status,
}

impl Audio {
    /// Starts the engine on the chosen device. Never fails: problems show up
    /// in [`Status`].
    pub fn start(choice: &Choice, timeline: Timeline) -> Self {
        let (clips, player) = clip_player();
        let (midi, midi_play) = midi_player();
        let (mut inputs, input_play) = input_player();
        let takes = TakeWorker::spawn(
            inputs
                .take_capture()
                .expect("a new input player still has its capture end"),
        );
        let (mixer, source) = mixer(Layered(player, Layered(midi_play, input_play)));
        let (engine_handle, mut engine) = engine(timeline, source, 48_000);
        let testing = match choice {
            Choice::Named { driver, .. } => is_test(driver),
            Choice::Saved(saved) => is_test(&saved.driver),
            Choice::Default => false,
        };
        let (mut tap, heard) = if testing {
            let (tap, heard) = rtrb::RingBuffer::new(HEARD);
            (Some(tap), Some(heard))
        } else {
            (None, None)
        };
        let clock = StreamClock::new();
        let (export, mut export_tap) = export_live::channel();
        let export = Arc::new(Mutex::new(export));
        let stream = Arc::clone(&clock);
        let mut processed = 0_u64;
        let callback: Box<dyn AudioCallback> = Box::new(move |block: &mut Block<'_>| {
            let (rate, channels, frames) = (block.sample_rate, block.output_channels, block.frames);
            // MIDI to hardware is timed against this clock (midi_out.rs).
            stream.publish(processed, frames, rate);
            processed += frames as u64;
            engine.process_with_input(
                rate,
                block.inputs,
                block.input_channels,
                block.outputs,
                channels,
                frames,
            );
            export_tap.process(block);
            if let Some(tap) = &mut tap {
                listen(tap, block.outputs, channels, frames);
            }
        });
        let mut audio = Self {
            system: None,
            parked: Some(callback),
            engine: engine_handle,
            clips,
            midi,
            clock,
            mixer,
            inputs,
            takes,
            export,
            heard,
            status: Status::Off(String::new()),
            driver: String::new(),
            device: String::new(),
            config: None,
            cue_channels: Vec::new(),
            extra_channels: Vec::new(),
            trying_extra: None,
            refused_extra: None,
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
        self.cue_channels = found.cue_channels;
        self.extra_channels = Vec::new();
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
                    cue_channels: self.cue_channels.clone(),
                    extra_channels: self.extra_channels.clone(),
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
            input_channels: if config.wants_all_inputs() {
                Vec::new()
            } else {
                config.input_channels.clone()
            },
            all_inputs: config.wants_all_inputs(),
            output_channels: self.main_channels(config),
            cue_channels: self.cue_channels.clone(),
        })
    }

    /// The device channels open as inputs, in the order the engine sees
    /// them: none until the device runs.
    pub fn open_inputs(&self) -> Vec<usize> {
        match (&self.config, &self.info) {
            (Some(config), Some(info)) => config.inputs_on(info.input_channels),
            _ => Vec::new(),
        }
    }

    /// The main pair: the opened outputs before the cue pair (and any
    /// further pairs).
    fn main_channels(&self, config: &StreamConfig) -> Vec<usize> {
        let main = config
            .output_channels
            .len()
            .saturating_sub(self.cue_channels.len() + self.extra_channels.len());
        config.output_channels[..main.max(1).min(config.output_channels.len())].to_vec()
    }

    /// Where the device's output pairs sit in the stream: the main pair,
    /// the cue pair, then the further pairs open for the project.
    pub fn outputs(&self) -> crate::routing::Outputs {
        let pair = |channels: &[usize]| channels.first().map(|&c| (c / 2) as u16);
        crate::routing::Outputs {
            main: self
                .config
                .as_ref()
                .and_then(|config| pair(&self.main_channels(config))),
            cue: pair(&self.cue_channels),
            extra: self
                .extra_channels
                .chunks(2)
                .filter_map(|c| c.first().map(|&c| (c / 2) as u16))
                .collect(),
        }
    }

    /// The driver and the device in use, while one is.
    pub fn device_key(&self) -> Option<(&str, &str)> {
        let config = self.config.as_ref()?;
        Some((self.driver.as_str(), config.device.id.as_str()))
    }

    /// Opens `pairs` of the device's outputs (it has `outputs` channels)
    /// after the main and cue pairs, so tracks can play to them; reopens
    /// the stream only when that set changes. Pairs the device does not
    /// have, the main and the cue pair are left out.
    pub fn want_pairs(&mut self, pairs: &std::collections::BTreeSet<u16>, outputs: usize) {
        let (Some(config), Some(system)) = (self.config.as_ref(), &self.system) else {
            return;
        };
        if !matches!(self.status, Status::Running { .. }) {
            return;
        }
        let main = self.main_channels(config);
        let taken = |c: usize| main.contains(&c) || self.cue_channels.contains(&c);
        let extra: Vec<usize> = pairs
            .iter()
            .flat_map(|&p| [usize::from(p) * 2, usize::from(p) * 2 + 1])
            .filter(|&c| c < outputs && !taken(c))
            .collect();
        if extra == self.extra_channels || self.refused_extra.as_ref() == Some(&extra) {
            return;
        }
        // Back to this setup if the device will not open them.
        self.previous = Some(Previous {
            driver: self.driver.clone(),
            device: self.device.clone(),
            config: config.clone(),
            cue_channels: self.cue_channels.clone(),
            extra_channels: self.extra_channels.clone(),
            info: self.info.clone(),
            status: self.status.clone(),
        });
        self.trying_extra = Some(extra.clone());
        let mut config = config.clone();
        config.output_channels = main
            .iter()
            .chain(&self.cue_channels)
            .chain(&extra)
            .copied()
            .collect();
        system.configure(config.clone());
        self.config = Some(config);
        self.extra_channels = extra;
        self.status = Status::Starting;
    }

    /// Whether a cue pair is open: chosen in the audio settings and running
    /// as the third and fourth channels.
    pub fn cue_ready(&self) -> bool {
        !self.cue_channels.is_empty()
            && matches!(self.status, Status::Running { .. })
            && self.info.as_ref().is_some_and(|i| i.output_channels >= 4)
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
            self.cue_channels = previous.cue_channels;
            self.extra_channels = previous.extra_channels;
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
                all_inputs: previous.config.wants_all_inputs(),
                output_channels: previous.config.output_channels
                    [..previous.config.output_channels.len() - previous.cue_channels.len()]
                    .to_vec(),
                cue_channels: previous.cue_channels.clone(),
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
                    self.trying_extra = None;
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
                    // Further outputs the device would not open are not
                    // asked for again.
                    self.refused_extra = self.trying_extra.take();
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
        self.midi.collect_garbage();
        self.mixer.collect_garbage();
        changed
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_turn_on_the_input_device_starts_from_a_clean_one() {
        // A scenario scripts everything it can and ends, as one that fails does.
        {
            let turn = input_device_turn();
            turn.set(|d| {
                d.input_ramp = true;
                d.input_levels = vec![0.5, 0.25];
                d.present = false;
                d.inputs = 8;
                d.pace = None;
            });
        }
        // The next one finds none of it.
        let turn = input_device_turn();
        let device = input_device();
        let mut seen = None;
        device.set(|d| seen = Some((d.input_ramp, d.input_levels.clone(), d.present, d.inputs)));
        assert_eq!(seen, Some((false, Vec::new(), true, 2)));
        // And it is still the slow, steady test device.
        device.set(|d| assert_eq!(d.pace, Some(Pace::Every(TEST_BLOCK_EVERY))));
        drop(turn);
    }

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
