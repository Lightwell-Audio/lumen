//! The audio supervisor: one control thread that owns the backend and keeps
//! audio running through driver resets, configuration changes and device
//! loss. The rest of Lumen talks to it through [`AudioSystem`].
//!
//! States: `Stopped` → `Running` ⇄ `Lost`. A reset request closes and reopens
//! the same configuration; if the device is gone, the supervisor reports
//! `Lost` and retries until it returns. A reopened device only counts as
//! restored once callbacks have flowed for [`Timing::restore_confirm`]:
//! some drivers accept an open while their hardware is disappearing, then
//! fail moments later. A configuration change that fails falls back to the
//! last working configuration (spec §6.9).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::backend::{Backend, Stream, StreamEvent};
use crate::types::{AudioCallback, AudioError, Block, StreamConfig, StreamInfo};

/// What the supervisor reports to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEvent {
    Started(StreamInfo),
    /// The driver asked for a reset and the stream came back.
    Reset(StreamInfo),
    /// The device disappeared; audio is off until it returns.
    Lost(AudioError),
    /// The device came back and audio resumed.
    Restored(StreamInfo),
    /// A requested configuration failed; `reverted` says whether the
    /// previous one is running again.
    ConfigFailed {
        error: AudioError,
        reverted: bool,
    },
    Stopped,
}

/// Lock-free status for meters, written by the audio callback.
#[derive(Debug, Default)]
pub struct Status {
    state: AtomicU8,
    callbacks: AtomicU64,
    /// Callback time ÷ block duration, in thousandths.
    last_load: AtomicU32,
    peak_load: AtomicU32,
    /// Blocks whose processing took longer than their duration.
    overloads: AtomicU64,
    /// Frames processed, for the starvation check.
    frames: AtomicU64,
    /// Audio that never reached the device: missed buffers, or delivery
    /// slower than real time.
    dropouts: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped = 0,
    Running = 1,
    Lost = 2,
}

impl Status {
    pub fn state(&self) -> State {
        match self.state.load(Ordering::Relaxed) {
            1 => State::Running,
            2 => State::Lost,
            _ => State::Stopped,
        }
    }
    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }
    /// Share of the block's time budget the last callback used (1.0 = all).
    pub fn load(&self) -> f32 {
        self.last_load.load(Ordering::Relaxed) as f32 / 1000.0
    }
    /// Highest load since the last call to this method.
    pub fn take_peak_load(&self) -> f32 {
        self.peak_load.swap(0, Ordering::Relaxed) as f32 / 1000.0
    }
    pub fn overloads(&self) -> u64 {
        self.overloads.load(Ordering::Relaxed)
    }
    /// Dropouts since start (or the last reset): buffers the driver skipped,
    /// plus audio delivered slower than real time, in buffers.
    pub fn dropouts(&self) -> u64 {
        self.dropouts.load(Ordering::Relaxed)
    }
    /// Zeroes the dropout and overload counters (the user's "reset").
    pub fn reset_counters(&self) {
        self.dropouts.store(0, Ordering::Relaxed);
        self.overloads.store(0, Ordering::Relaxed);
    }
    fn set_state(&self, state: State) {
        self.state.store(state as u8, Ordering::Relaxed);
    }
}

/// Wraps the user's callback to measure its load and spot skipped buffers.
/// Real-time safe.
struct Metered {
    inner: Box<dyn AudioCallback>,
    status: Arc<Status>,
    /// Where the driver's sample position should be next block, when it
    /// reports one (ASIO).
    expected_position: Option<u64>,
}

impl AudioCallback for Metered {
    fn process(&mut self, block: &mut Block<'_>) {
        let started = Instant::now();
        self.inner.process(block);
        let used = started.elapsed().as_secs_f64();
        let budget = block.frames as f64 / f64::from(block.sample_rate.max(1));
        let load = (used / budget * 1000.0) as u32;
        self.status.last_load.store(load, Ordering::Relaxed);
        self.status.peak_load.fetch_max(load, Ordering::Relaxed);
        if load > 1000 {
            self.status.overloads.fetch_add(1, Ordering::Relaxed);
        }
        // A driver position past where it should be means it skipped
        // buffers. A position behind it means the stream restarted: resync.
        if let Some(position) = block.position {
            if let Some(expected) = self.expected_position
                && position > expected
            {
                let skipped = (position - expected).div_ceil(block.frames.max(1) as u64);
                self.status.dropouts.fetch_add(skipped, Ordering::Relaxed);
            }
            self.expected_position = Some(position + block.frames as u64);
        }
        self.status.callbacks.fetch_add(1, Ordering::Relaxed);
        self.status
            .frames
            .fetch_add(block.frames as u64, Ordering::Relaxed);
    }
}

enum Command {
    Configure(StreamConfig),
    Stop,
    Shutdown,
    /// Open the running driver's settings window; the result comes back.
    ControlPanel(Sender<Result<(), String>>),
    /// Close the stream, give the callback back and end the thread, so a
    /// new control thread can run it with another driver.
    Handover(Sender<Box<dyn AudioCallback>>),
}

/// Timing knobs; tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How often to try reopening a lost device.
    pub retry_interval: Duration,
    /// How often the control thread polls the stream and pumps messages.
    pub poll_interval: Duration,
    /// How long a reopened lost device must deliver audio before it is
    /// reported `Restored`.
    pub restore_confirm: Duration,
    /// How often delivery is checked against real time (`None` turns the
    /// check off). The first window after any (re)open is ignored.
    pub starvation_window: Option<Duration>,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            retry_interval: Duration::from_secs(1),
            poll_interval: Duration::from_millis(10),
            restore_confirm: Duration::from_secs(1),
            starvation_window: Some(Duration::from_secs(1)),
        }
    }
}

/// Handle to the audio supervisor. Dropping it stops audio and joins the thread.
pub struct AudioSystem {
    commands: Sender<Command>,
    events: Receiver<DeviceEvent>,
    /// Cloned into each control thread, so events keep arriving after a
    /// driver switch.
    event_tx: Sender<DeviceEvent>,
    status: Arc<Status>,
    thread: Option<JoinHandle<()>>,
    timing: Timing,
}

impl AudioSystem {
    /// Starts the control thread. `make_backend` runs on that thread, so
    /// thread-affine drivers (ASIO's COM apartment) live there.
    pub fn spawn<B, F>(make_backend: F, callback: Box<dyn AudioCallback>, timing: Timing) -> Self
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (event_tx, events) = mpsc::channel();
        let status = Arc::new(Status::default());
        let metered: Box<dyn AudioCallback> = Box::new(Metered {
            inner: callback,
            status: Arc::clone(&status),
            expected_position: None,
        });
        let (commands, thread) = Self::start_thread(
            make_backend,
            metered,
            event_tx.clone(),
            Arc::clone(&status),
            timing,
        );
        Self {
            commands,
            events,
            event_tx,
            status,
            thread: Some(thread),
            timing,
        }
    }

    fn start_thread<B, F>(
        make_backend: F,
        callback: Box<dyn AudioCallback>,
        events: Sender<DeviceEvent>,
        status: Arc<Status>,
        timing: Timing,
    ) -> (Sender<Command>, JoinHandle<()>)
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("lumen-audio-control".into())
            .spawn(move || {
                let backend = make_backend();
                Supervisor {
                    backend,
                    callback: Some(callback),
                    stream: None,
                    config: None,
                    lost_since: None,
                    confirming: None,
                    running: None,
                    window: None,
                    next_retry: Instant::now(),
                    events,
                    status,
                    timing,
                }
                .run(command_rx);
            })
            .expect("spawn audio control thread");
        (commands, thread)
    }

    /// Switches to another driver model (ASIO to WASAPI, for example) and
    /// opens `config` on it. The running callback, with all its state, moves
    /// to a fresh control thread: driver models need differently set-up
    /// threads (COM apartments), which cannot change once set. Results
    /// arrive as events. Fails only if the old control thread is gone.
    pub fn switch<B, F>(&mut self, make_backend: F, config: StreamConfig) -> Result<(), String>
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.commands
            .send(Command::Handover(tx))
            .map_err(|_| "the audio thread has stopped".to_owned())?;
        let callback = rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "the audio thread did not hand over in time".to_owned())?;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let (commands, thread) = Self::start_thread(
            make_backend,
            callback,
            self.event_tx.clone(),
            Arc::clone(&self.status),
            self.timing,
        );
        self.commands = commands;
        self.thread = Some(thread);
        self.configure(config);
        Ok(())
    }

    /// Opens (or switches to) a configuration. Results arrive as events.
    pub fn configure(&self, config: StreamConfig) {
        let _ = self.commands.send(Command::Configure(config));
    }

    /// Opens the running driver's own settings window (ASIO). Waits for the
    /// driver to open it, at most a few seconds.
    pub fn open_control_panel(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.commands
            .send(Command::ControlPanel(tx))
            .map_err(|_| "the audio thread has stopped".to_owned())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "the driver did not respond".to_owned())?
    }

    /// Audio off (the "Audio On/Off" switch).
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    /// Next event without waiting.
    pub fn try_event(&self) -> Option<DeviceEvent> {
        self.events.try_recv().ok()
    }

    /// Next event, waiting up to `timeout`.
    pub fn wait_event(&self, timeout: Duration) -> Option<DeviceEvent> {
        self.events.recv_timeout(timeout).ok()
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Supervisor<B: Backend> {
    backend: B,
    /// Held here whenever no stream owns it.
    callback: Option<Box<dyn AudioCallback>>,
    stream: Option<Box<dyn Stream>>,
    /// The last configuration that worked (or is wanted, while lost).
    config: Option<StreamConfig>,
    lost_since: Option<Instant>,
    /// A lost device that reopened and is on probation.
    confirming: Option<Confirming>,
    /// The running stream's rate and block size, for the starvation check.
    running: Option<StreamInfo>,
    /// The current starvation window: when it began, frames by then, and
    /// whether it is the warm-up window after an open (ignored).
    window: Option<(Instant, u64, bool)>,
    next_retry: Instant,
    events: Sender<DeviceEvent>,
    status: Arc<Status>,
    timing: Timing,
}

struct Confirming {
    info: StreamInfo,
    callbacks_at_open: u64,
    until: Instant,
}

impl<B: Backend> Supervisor<B> {
    fn run(mut self, commands: Receiver<Command>) {
        loop {
            match commands.recv_timeout(self.timing.poll_interval) {
                Ok(Command::Configure(config)) => self.configure(config),
                Ok(Command::Stop) => self.stop(),
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    self.stop();
                    return;
                }
                Ok(Command::ControlPanel(reply)) => {
                    let result = match self.stream.as_mut() {
                        Some(stream) => stream.control_panel(),
                        None => Err("audio is not running".into()),
                    };
                    let _ = reply.send(result);
                }
                Ok(Command::Handover(reply)) => {
                    self.close();
                    self.lost_since = None;
                    self.status.set_state(State::Stopped);
                    if let Some(callback) = self.callback.take() {
                        let _ = reply.send(callback);
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.backend.pump();
            self.watch();
        }
    }

    fn emit(&self, event: DeviceEvent) {
        let _ = self.events.send(event);
    }

    /// Opens `config`; on failure the callback comes back into `self`.
    fn try_open(&mut self, config: &StreamConfig) -> Result<StreamInfo, AudioError> {
        let callback = self
            .callback
            .take()
            .expect("callback is held while no stream runs");
        match self.backend.open(config, callback) {
            Ok(stream) => {
                let info = stream.info();
                self.stream = Some(stream);
                self.running = Some(info.clone());
                self.window = None;
                Ok(info)
            }
            Err(failed) => {
                self.callback = Some(failed.callback);
                Err(failed.error)
            }
        }
    }

    fn close(&mut self) {
        self.confirming = None;
        self.running = None;
        self.window = None;
        if let Some(stream) = self.stream.take() {
            self.callback = Some(stream.close());
        }
    }

    fn configure(&mut self, wanted: StreamConfig) {
        let previous = if self.lost_since.is_some() {
            None
        } else {
            self.config.clone()
        };
        self.close();
        self.lost_since = None;
        match self.try_open(&wanted) {
            Ok(info) => {
                self.config = Some(wanted);
                self.status.set_state(State::Running);
                self.emit(DeviceEvent::Started(info));
            }
            Err(error) => {
                let reverted = match &previous {
                    Some(previous) => self.try_open(previous).is_ok(),
                    None => false,
                };
                if reverted {
                    self.status.set_state(State::Running);
                } else {
                    self.status.set_state(State::Stopped);
                    self.config = previous;
                }
                self.emit(DeviceEvent::ConfigFailed { error, reverted });
            }
        }
    }

    fn stop(&mut self) {
        let was_active = self.stream.is_some() || self.lost_since.is_some();
        self.close();
        self.lost_since = None;
        self.status.set_state(State::Stopped);
        if was_active {
            self.emit(DeviceEvent::Stopped);
        }
    }

    fn mark_lost(&mut self, error: AudioError) {
        self.lost_since = Some(Instant::now());
        self.next_retry = Instant::now() + self.timing.retry_interval;
        self.status.set_state(State::Lost);
        self.emit(DeviceEvent::Lost(error));
    }

    /// Compares audio delivered with real time over each window; a
    /// shortfall beyond 3% counts as dropouts, in buffers. Catches drivers
    /// that run but starve the device (MT-AUDIO-4).
    fn check_starvation(&mut self) {
        let (Some(window), Some(info)) = (self.timing.starvation_window, &self.running) else {
            return;
        };
        let frames = self.status.frames.load(Ordering::Relaxed);
        let now = Instant::now();
        let Some((started, at_start, warm_up)) = self.window else {
            self.window = Some((now, frames, true));
            return;
        };
        let elapsed = now.duration_since(started);
        if elapsed < window {
            return;
        }
        if !warm_up {
            let expected = elapsed.as_secs_f64() * f64::from(info.sample_rate);
            let delivered = (frames - at_start) as f64;
            let block = f64::from(info.buffer_frames.max(1));
            let missing = expected * 0.97 - delivered;
            if missing > 2.0 * block {
                let lost = ((expected - delivered) / block).ceil() as u64;
                self.status.dropouts.fetch_add(lost, Ordering::Relaxed);
            }
        }
        self.window = Some((now, frames, false));
    }

    /// A reopened lost device failed its probation: back to quiet retries.
    fn retry_later(&mut self) {
        self.close();
        self.next_retry = Instant::now() + self.timing.retry_interval;
    }

    fn watch(&mut self) {
        let Some(config) = self.config.clone() else {
            return;
        };
        if let Some(stream) = self.stream.as_mut() {
            let event = stream.poll();
            if event.is_none() && self.confirming.is_none() {
                self.check_starvation();
            }
            if self.confirming.is_some() {
                match event {
                    None => self.confirm_restore(),
                    Some(_) => self.retry_later(),
                }
                return;
            }
            match event {
                None => {}
                Some(StreamEvent::ResetRequested) => {
                    self.close();
                    match self.try_open(&config) {
                        Ok(info) => self.emit(DeviceEvent::Reset(info)),
                        Err(error) => self.mark_lost(error),
                    }
                }
                Some(StreamEvent::Stalled) => {
                    self.close();
                    self.mark_lost(AudioError::DeviceNotPresent(
                        "the driver stopped delivering audio".into(),
                    ));
                }
            }
        } else if self.lost_since.is_some() && Instant::now() >= self.next_retry {
            match self.try_open(&config) {
                Ok(info) => {
                    self.confirming = Some(Confirming {
                        info,
                        callbacks_at_open: self.status.callbacks(),
                        until: Instant::now() + self.timing.restore_confirm,
                    });
                }
                Err(_) => self.next_retry = Instant::now() + self.timing.retry_interval,
            }
        }
    }

    /// Ends probation once its window has passed: restored if audio flowed.
    fn confirm_restore(&mut self) {
        let Some(confirming) = &self.confirming else {
            return;
        };
        if Instant::now() < confirming.until {
            return;
        }
        if self.status.callbacks() == confirming.callbacks_at_open {
            self.retry_later();
            return;
        }
        let info = confirming.info.clone();
        self.confirming = None;
        self.lost_since = None;
        self.status.set_state(State::Running);
        self.emit(DeviceEvent::Restored(info));
    }
}
