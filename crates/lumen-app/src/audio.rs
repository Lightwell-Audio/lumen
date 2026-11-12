//! Audio for the app: the device (through `lumen-audio`'s supervisor) with
//! the engine running in its callback. The device comes from the audio
//! settings (saved), from `--audio`, or is the system default output; the
//! audio settings window changes it while Lumen runs (M2 slice 8). A
//! change runs on a worker (`change`), so a driver that is slow to let go
//! never stops the interface (ADR-0113).

mod change;
mod devices;
mod test_devices;

use devices::*;
pub use devices::{
    Choice, DEFAULT_DRIVER, MakeBackend, Status, backend, driver_label, drivers_for, listed,
};

use test_devices::is_test;
pub use test_devices::{
    BLOCKED_INPUT, InputDeviceTurn, TEST_BLOCKED_INPUT_DRIVER, TEST_DRIVER, TEST_HOLDS_DRIVER,
    TEST_LATE_DRIVER, TEST_PACED_DRIVER, TEST_REFUSES_DRIVER, TEST_SLOW_DRIVER, input_device,
    input_device_turn,
};

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use lumen_audio::{
    AudioCallback, AudioError, AudioSystem, Block, DeviceEvent, Status as Meters, StreamConfig,
    StreamInfo, Timing,
};
use lumen_engine::chime::{self, ChimeHandle, Sounding};
use lumen_engine::preview::{self, PreviewFeed, PreviewHandle};
use lumen_engine::{
    ClipsHandle, EngineHandle, InputHandle, Layered, MidiHandle, MixerHandle, Timeline,
    clip_player, engine, input_player, loopback, midi_player, mixer,
};

use crate::export::live as export_live;
use crate::measure_latency::{Cable, Measurement};
use crate::midi::out::StreamClock;
use crate::recording::take_worker::TakeWorker;
use crate::settings::{AudioSettings, MeasuredWith};

/// What the test device's tap holds: 40 seconds of the fake device's
/// playback (it runs at about a quarter of real time), left and right.
const HEARD: usize = 1 << 20;

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
    /// `parked`. Held by the change's worker while it switches drivers.
    system: Option<AudioSystem>,
    /// The engine's callback while no audio system runs it.
    parked: Option<Box<dyn AudioCallback>>,
    /// The audio system's meters, kept while a worker holds the system.
    meters: Option<Arc<Meters>>,
    /// A device change running on its worker (ADR-0113).
    change: Option<change::Change>,
    /// When the device was last asked to open, and what messages call its
    /// driver: until it answers, no other change starts.
    asked: Option<(Instant, String)>,
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
    /// The loopback helper's measurement (M9 slice R2b).
    pub(crate) measurement: Measurement,
    /// The sound check's chime (M9 slice R7b).
    chime: ChimeHandle,
    /// The browser's file preview (M9 R7c): the UI's end, and the decode
    /// thread's until the browser takes it.
    pub(crate) preview: PreviewHandle,
    preview_feed: Option<PreviewFeed>,
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
        if testing {
            // A test device waits for its plugin hosts as an offline render
            // does (ADR-0122): a block is never played dry because a busy
            // CI machine kept a host from answering in a share of a period,
            // so a test hears what the code decides, not how loaded the
            // machine was. Missed deadlines are the engine's tests' to prove.
            engine.set_offline(Some(lumen_engine::OFFLINE_WAIT));
        }
        let (mut tap, heard) = if testing {
            let (tap, heard) = rtrb::RingBuffer::new(HEARD);
            (Some(tap), Some(heard))
        } else {
            (None, None)
        };
        let clock = StreamClock::new();
        let (export, mut export_tap) = export_live::channel();
        let (measure, mut clicks) = loopback::loopback();
        let (chime, mut chime_player) = chime::chime();
        let (preview, preview_feed, mut preview_player) = preview::preview();
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
            // Heard, not exported.
            chime_player.process(rate, block.outputs, channels, frames);
            preview_player.process(rate, block.outputs, channels, frames);
            // While measuring, only the clicks are heard.
            clicks.process(loopback::Block {
                rate,
                inputs: block.inputs,
                input_channels: block.input_channels,
                outputs: &mut *block.outputs,
                output_channels: channels,
                frames,
            });
            if let Some(tap) = &mut tap {
                listen(tap, block.outputs, channels, frames);
            }
        });
        let mut audio = Self {
            system: None,
            parked: Some(callback),
            meters: None,
            change: None,
            asked: None,
            engine: engine_handle,
            clips,
            midi,
            clock,
            mixer,
            inputs,
            takes,
            export,
            measurement: Measurement::new(measure),
            chime,
            preview,
            preview_feed: Some(preview_feed),
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
                audio.open(found, String::new());
            }
            Err(message) => audio.status = Status::Off(message),
        }
        audio
    }

    /// Opens `found`: reconfigures the running driver model, or starts the
    /// first audio system, neither of which waits. Another driver model is
    /// switched to on a worker; `from` names the driver left.
    fn open(&mut self, found: Resolved, from: String) {
        if self.system.is_some() && found.driver != self.driver {
            let names = change::Names {
                device: found.name.clone(),
                from,
                to: driver_name(&found.driver, &found.name),
            };
            let system = self.system.take();
            self.change = Some(change::Change::start(
                change::Target::Found(found),
                system,
                None,
                None,
                names,
            ));
            return;
        }
        let config = found.config();
        if let Some(system) = &self.system {
            system.configure(config.clone());
        } else if let Some(callback) = self.parked.take() {
            match AudioSystem::spawn(found.make, callback, Timing::default()) {
                Ok(system) => {
                    system.configure(config.clone());
                    self.meters = Some(Arc::clone(system.status()));
                    self.system = Some(system);
                }
                Err(failed) => {
                    // The callback comes back so audio can be tried again.
                    // Audio stays off, and the bar says why.
                    self.parked = Some(failed.callback);
                    self.status = Status::Off(failed.error.to_string());
                    return;
                }
            }
        }
        self.opened(found);
    }

    /// Records `found` as the setup in use, asked to open.
    fn opened(&mut self, found: Resolved) {
        self.asked = Some((Instant::now(), driver_name(&found.driver, &found.name)));
        self.config = Some(found.config());
        self.driver = found.driver;
        self.device = found.name;
        self.cue_channels = found.cue_channels;
        self.extra_channels = Vec::new();
        self.info = None;
        self.last_error = None;
        self.status = Status::Starting;
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

    /// The device channels open as outputs, in the order the engine plays
    /// them: the main pair, the cue pair, then further pairs.
    pub fn open_outputs(&self) -> Vec<usize> {
        match (&self.config, &self.info) {
            (Some(config), Some(_)) => config.output_channels.clone(),
            _ => Vec::new(),
        }
    }

    /// Why the loopback helper cannot measure now, if it cannot.
    pub fn measure_unavailable(&self) -> Result<(), &'static str> {
        match (&self.status, &self.info) {
            (Status::Running { .. }, Some(info)) if info.input_channels == 0 => {
                Err("The audio device has no inputs open")
            }
            (Status::Running { .. }, Some(_)) => Ok(()),
            _ => Err("Audio is off"),
        }
    }

    /// Measures the round trip through `cable` on the running device.
    pub fn measure(&mut self, cable: Cable) {
        let (Some(info), Some(settings)) = (self.info.clone(), self.settings()) else {
            return;
        };
        let with = MeasuredWith {
            driver: settings.driver,
            device_id: settings.device_id,
            device_name: settings.device_name,
            sample_rate: info.sample_rate,
            buffer_frames: info.buffer_frames,
        };
        self.measurement.start(&info, cable, with);
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
        self.meters.as_ref()
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
        if let Some(change) = &self.change {
            return Some(match change.step {
                change::Step::Looking => format!("Starting {}…", change.device),
                change::Step::HandingOver | change::Step::Closing => {
                    format!("Closing {}…", change.from)
                }
            });
        }
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
        let from = driver_name(&self.driver, &tried);
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
            self.open(resolved, from);
            self.notice = notice;
            self.notice_detail = detail;
        }
    }

    /// Whether the user can turn audio on or off: a device was found, no
    /// worker holds the audio system, and no device is still to answer. A
    /// stop sent to a driver that has not answered would wait behind it,
    /// and would free the change lock while the driver still works.
    pub fn can_switch(&self) -> bool {
        self.config.is_some() && self.system.is_some() && !self.waiting()
    }

    pub fn is_on(&self) -> bool {
        !matches!(self.status, Status::Off(_))
    }

    /// Audio was switched off (Audio On/Off), rather than failing.
    pub fn switched_off(&self) -> bool {
        matches!(&self.status, Status::Off(why) if why == AUDIO_OFF)
    }

    /// A device was found and chosen at some point (none on a machine
    /// with no output).
    pub fn has_device(&self) -> bool {
        self.config.is_some()
    }

    /// A test device runs (tests and `--audio test:…`).
    pub fn on_test_device(&self) -> bool {
        is_test(&self.driver)
    }

    /// Another application holds the device.
    pub fn is_busy(&self) -> bool {
        matches!(self.status, Status::Off(_))
            && matches!(self.last_error, Some(AudioError::DeviceBusy(_)))
    }

    /// Plays the sound check's chime on the main pair (M9 slice R7b). It
    /// ends by itself, on Stop, and when the device stops running.
    pub fn sound_check(&mut self) {
        let pair = self
            .config
            .as_ref()
            .is_some_and(|config| self.main_channels(config).len() >= 2);
        self.chime.play(pair);
    }

    /// Why the sound check cannot play now, if it cannot.
    pub fn sound_check_unavailable(&self) -> Result<(), &'static str> {
        match self.status {
            Status::Running { .. } => Ok(()),
            _ => Err("Audio is off"),
        }
    }

    /// Silences the sound check.
    pub fn stop_sound_check(&self) {
        self.chime.stop();
    }

    /// What the sound check sounds, and its peaks, left and right. Silent
    /// whenever the device is not running: a device that stopped mid-note
    /// may never call back to clear what it last published.
    pub fn sound_check_sounding(&self) -> (Sounding, [f32; 2]) {
        if !matches!(self.status, Status::Running { .. }) {
            return (Sounding::Silent, [0.0; 2]);
        }
        (self.chime.sounding(), self.chime.peaks())
    }

    /// The decode thread's end of the browser's preview: taken once.
    pub(crate) fn take_preview_feed(&mut self) -> Option<PreviewFeed> {
        self.preview_feed.take()
    }

    /// The first channel previews are heard on: the cue pair's when one is
    /// open, the main pair's otherwise (the owner's O A).
    pub(crate) fn preview_channel(&self) -> usize {
        if self.cue_ready() {
            self.main_channels_len()
        } else {
            0
        }
    }

    /// How many of the opened outputs are the main pair's.
    fn main_channels_len(&self) -> usize {
        self.config
            .as_ref()
            .map_or(0, |config| self.main_channels(config).len())
    }

    /// The Audio On/Off switch: closes the device, or opens it again.
    pub fn set_on(&mut self, on: bool) {
        if !self.can_switch() {
            return;
        }
        let asked = (Instant::now(), driver_name(&self.driver, &self.device));
        let (Some(system), Some(config)) = (&self.system, &self.config) else {
            return;
        };
        if on {
            system.configure(config.clone());
            self.asked = Some(asked);
            self.status = Status::Starting;
        } else {
            system.stop();
            self.status = Status::Off(AUDIO_OFF.into());
        }
    }

    /// Applies device events. Call every frame; returns true if the status
    /// changed.
    pub fn poll(&mut self) -> bool {
        // A chime asked of a device that stopped would sound when it
        // starts again.
        if !matches!(self.status, Status::Running { .. }) {
            self.chime.stop();
        }
        let measured = self
            .measurement
            .poll(matches!(self.status, Status::Running { .. }));
        // Taken first: handling a failure may reopen the previous setup.
        // And applied before the change's answer: they are about the setup
        // the change replaces, and read after it they would pass for the
        // new device's answer.
        let events: Vec<DeviceEvent> = self.system.as_ref().map_or_else(Vec::new, |system| {
            std::iter::from_fn(|| system.try_event()).collect()
        });
        let mut changed = !events.is_empty();
        for event in events {
            // The device answered.
            self.asked = None;
            match event {
                DeviceEvent::Started(info)
                | DeviceEvent::Reset(info)
                | DeviceEvent::Restored(info) => {
                    self.status = Status::Running {
                        device: self.device.clone(),
                        rate: format!("{:.1} kHz", f64::from(info.sample_rate) / 1000.0),
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
                DeviceEvent::Stopped => self.status = Status::Off(AUDIO_OFF.into()),
            }
        }
        changed |= self.poll_change();
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
        changed || measured
    }
}

impl Drop for Audio {
    /// Quitting never waits on a driver that has not answered: an audio
    /// system still opening is dropped on a thread of its own, which the
    /// process does not wait for. So is the change under way: an answer it
    /// sent and nobody read can hold a system still opening its device. A
    /// worker still holding one is left as it is.
    fn drop(&mut self) {
        if !self.waiting() {
            return;
        }
        let system = self.system.take();
        let change = self.change.take();
        let _ = std::thread::Builder::new()
            .name("lumen-audio-close".into())
            .spawn(move || drop((change, system)));
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
