use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::backend::{Backend, OpenError, Stream, StreamEvent, silent};
use crate::types::{AudioCallback, AudioError, Block, StreamConfig, StreamInfo};

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEvent {
    Started(StreamInfo),
    Reset(StreamInfo),
    Lost(AudioError),
    Restored(StreamInfo),
    ConfigFailed {
        error: AudioError,
        reverted: bool,
    },
    Stopped,
}

#[derive(Debug, Default)]
pub struct Status {
    state: AtomicU8,
    callbacks: AtomicU64,
    last_load: AtomicU32,
    peak_load: AtomicU32,
    window_blocks: AtomicU32,
    window_used: AtomicU64,
    window_budget: AtomicU64,
    overloads: AtomicU64,
    frames: AtomicU64,
    dropouts: AtomicU64,
    slow: AtomicU64,
    resync: AtomicBool,
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
    pub fn load(&self) -> f32 {
        self.last_load.load(Ordering::Relaxed) as f32 / 1000.0
    }
    pub fn take_peak_load(&self) -> f32 {
        self.peak_load.swap(0, Ordering::Relaxed) as f32 / 1000.0
    }
    pub fn take_block_time(&self) -> BlockTime {
        let blocks = self.window_blocks.swap(0, Ordering::Relaxed);
        let used = self.window_used.swap(0, Ordering::Relaxed);
        let budget = self.window_budget.swap(0, Ordering::Relaxed);
        let mean = |sum: u64| Duration::from_nanos(sum.checked_div(u64::from(blocks)).unwrap_or(0));
        BlockTime {
            blocks,
            used: mean(used),
            block: mean(budget),
        }
    }
    pub fn overloads(&self) -> u64 {
        self.overloads.load(Ordering::Relaxed)
    }
    pub fn dropouts(&self) -> u64 {
        self.dropouts.load(Ordering::Relaxed)
    }
    pub fn slow_dropouts(&self) -> u64 {
        self.slow.load(Ordering::Relaxed)
    }
    pub fn count_dropouts(&self, n: u64) {
        self.dropouts.fetch_add(n, Ordering::Relaxed);
    }
    pub fn reset_counters(&self) {
        self.dropouts.store(0, Ordering::Relaxed);
        self.slow.store(0, Ordering::Relaxed);
        self.overloads.store(0, Ordering::Relaxed);
    }
    fn set_state(&self, state: State) {
        self.state.store(state as u8, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockTime {
    pub blocks: u32,
    pub used: Duration,
    pub block: Duration,
}

struct Metered {
    inner: Box<dyn AudioCallback>,
    status: Arc<Status>,
    expected_position: Option<u64>,
}

impl AudioCallback for Metered {
    fn process(&mut self, block: &mut Block<'_>) {
        let started = Instant::now();
        self.inner.process(block);
        let used = started.elapsed().as_secs_f64();
        let budget = block.frames as f64 / f64::from(block.sample_rate.max(1));
        let load = (used / budget * 1000.0) as u32;
        self.status.window_blocks.fetch_add(1, Ordering::Relaxed);
        self.status
            .window_used
            .fetch_add((used * 1e9) as u64, Ordering::Relaxed);
        self.status
            .window_budget
            .fetch_add((budget * 1e9) as u64, Ordering::Relaxed);
        self.status.last_load.store(load, Ordering::Relaxed);
        self.status.peak_load.fetch_max(load, Ordering::Relaxed);
        if load > 1000 {
            self.status.overloads.fetch_add(1, Ordering::Relaxed);
        }
        if self.status.resync.swap(false, Ordering::Relaxed) || block.position.is_none() {
            self.expected_position = None;
        }
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
    ControlPanel(Sender<Result<(), String>>),
    Handover(Sender<Box<dyn AudioCallback>>),
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub retry_interval: Duration,
    pub poll_interval: Duration,
    pub restore_confirm: Duration,
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

pub const HANDOVER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchError {
    Stopped,
    NoHandover,
    NotStarted,
}

impl std::fmt::Display for SwitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SwitchError::Stopped => "the audio thread has stopped",
            SwitchError::NoHandover => "the audio thread did not hand over in time",
            SwitchError::NotStarted => "the audio thread could not start",
        })
    }
}

impl std::error::Error for SwitchError {}

pub struct AudioSystem {
    commands: Sender<Command>,
    events: Receiver<DeviceEvent>,
    event_tx: Sender<DeviceEvent>,
    status: Arc<Status>,
    thread: Option<JoinHandle<()>>,
    timing: Timing,
}

impl AudioSystem {
    pub fn spawn<B, F>(
        make_backend: F,
        callback: Box<dyn AudioCallback>,
        timing: Timing,
    ) -> Result<Self, OpenError>
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
        )?;
        Ok(Self {
            commands,
            events,
            event_tx,
            status,
            thread: Some(thread),
            timing,
        })
    }

    fn start_thread<B, F>(
        make_backend: F,
        callback: Box<dyn AudioCallback>,
        events: Sender<DeviceEvent>,
        status: Arc<Status>,
        timing: Timing,
    ) -> Result<(Sender<Command>, JoinHandle<()>), OpenError>
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (commands, command_rx) = mpsc::channel();
        let slot = Arc::new(Mutex::new(Some(callback)));
        let handed = Arc::clone(&slot);
        let thread = std::thread::Builder::new()
            .name("lumen-audio-control".into())
            .spawn(move || {
                let taken = handed.lock().unwrap_or_else(PoisonError::into_inner).take();
                let Some(callback) = taken else {
                    return;
                };
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
            });
        match thread {
            Ok(thread) => Ok((commands, thread)),
            Err(error) => {
                let callback = slot
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                    .unwrap_or_else(silent);
                Err(OpenError {
                    error: AudioError::Driver(format!(
                        "the audio control thread could not start: {error}"
                    )),
                    callback,
                })
            }
        }
    }

    pub fn switch<B, F>(
        &mut self,
        make_backend: F,
        config: StreamConfig,
        handed: impl FnOnce(),
    ) -> Result<(), SwitchError>
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.commands
            .send(Command::Handover(tx))
            .map_err(|_| SwitchError::Stopped)?;
        let callback = rx.recv_timeout(HANDOVER_TIMEOUT).map_err(|e| match e {
            RecvTimeoutError::Timeout => SwitchError::NoHandover,
            RecvTimeoutError::Disconnected => SwitchError::Stopped,
        })?;
        handed();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        while self.events.try_recv().is_ok() {}
        let (commands, thread) = Self::start_thread(
            make_backend,
            callback,
            self.event_tx.clone(),
            Arc::clone(&self.status),
            self.timing,
        )
        .map_err(|_| {
            SwitchError::NotStarted
        })?;
        self.commands = commands;
        self.thread = Some(thread);
        self.configure(config);
        Ok(())
    }

    pub fn configure(&self, config: StreamConfig) {
        let _ = self.commands.send(Command::Configure(config));
    }

    pub fn request_control_panel(&self) -> Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        if let Err(mpsc::SendError(Command::ControlPanel(tx))) =
            self.commands.send(Command::ControlPanel(tx))
        {
            let _ = tx.send(Err("the audio thread has stopped".into()));
        }
        rx
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    pub fn try_event(&self) -> Option<DeviceEvent> {
        self.events.try_recv().ok()
    }

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
    callback: Option<Box<dyn AudioCallback>>,
    stream: Option<Box<dyn Stream>>,
    config: Option<StreamConfig>,
    lost_since: Option<Instant>,
    confirming: Option<Confirming>,
    running: Option<StreamInfo>,
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

    fn try_open(&mut self, config: &StreamConfig) -> Result<StreamInfo, AudioError> {
        let callback = self
            .callback
            .take()
            .expect("callback is held while no stream runs");
        self.status.resync.store(true, Ordering::Relaxed);
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
                self.status.slow.fetch_add(lost, Ordering::Relaxed);
            }
        }
        self.window = Some((now, frames, false));
    }

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
                    Some(StreamEvent::Dropped(dropped)) => self.status.count_dropouts(dropped),
                    Some(_) => self.retry_later(),
                }
                return;
            }
            match event {
                None => {}
                Some(StreamEvent::Dropped(dropped)) => {
                    self.status.count_dropouts(dropped);
                }
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
