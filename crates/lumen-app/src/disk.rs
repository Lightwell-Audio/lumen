pub use lumen_engine::stream_plan as plan;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use crate::media::streams::Readers;
use lumen_engine::{PAGE, PagePool, PoolWriter, StreamAudio, TransportReader};
use lumen_media::MediaError;

use plan::{Horizon, Position, Scene, Want};

const IDLE: Duration = Duration::from_millis(10);
const RETRY: Duration = Duration::from_millis(20);
const ROUND: Duration = Duration::from_millis(5);
const FULL: Duration = Duration::from_millis(2);

pub trait Pages: Send {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError>;

    fn keep(&mut self, streams: &[StreamAudio]);
}

impl Pages for Readers {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        Readers::read(self, audio, (page * PAGE) as u64, left, right)
    }

    fn keep(&mut self, streams: &[StreamAudio]) {
        Readers::keep(self, streams);
    }
}

enum Message {
    Scene(Arc<Scene>),
    Cue(Option<i64>),
    Stop,
}

#[derive(Debug, Default)]
pub struct Stats {
    load: AtomicU32,
    failed: AtomicU32,
}

impl Stats {
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    pub fn failed(&self) -> u32 {
        self.failed.load(Ordering::Relaxed)
    }
}

pub struct Disk {
    messages: Option<Sender<Message>>,
    stats: Arc<Stats>,
    thread: Option<std::thread::JoinHandle<()>>,
    failure: Option<String>,
}

impl Disk {
    pub fn start(pool: &Arc<PagePool>, pages: Box<dyn Pages>, transport: TransportReader) -> Self {
        let (messages, inbox) = channel();
        let stats = Arc::new(Stats::default());
        let worker = Worker {
            writer: PoolWriter::new(pool),
            pages,
            transport,
            horizon: Horizon::at(0),
            scene: Arc::default(),
            cue: None,
            stats: Arc::clone(&stats),
            wants: Vec::new(),
            broken: HashSet::new(),
            left: vec![0.0; PAGE],
            right: vec![0.0; PAGE],
            busy: Duration::ZERO,
            since: Instant::now(),
        };
        let spawned = crate::spawn::spawn_named("lumen-disk", move || worker.run(&inbox));
        let (messages, thread, failure) = match spawned {
            Ok(thread) => (Some(messages), Some(thread), None),
            Err(error) => {
                let why = crate::spawn::start_failed("reading audio from disk", &error);
                crate::trace::log(|| format!("disk: {why}"));
                (None, None, Some(why))
            }
        };
        Self {
            messages,
            stats,
            thread,
            failure,
        }
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn set_scene(&self, scene: impl Into<Arc<Scene>>) {
        self.send(Message::Scene(scene.into()));
    }

    pub fn cuer(&self) -> Cuer {
        Cuer(self.messages.clone())
    }

    fn send(&self, message: Message) {
        if let Some(messages) = &self.messages {
            let _ = messages.send(message);
        }
    }
}

#[derive(Clone)]
pub struct Cuer(Option<Sender<Message>>);

impl Cuer {
    #[cfg(test)]
    pub fn idle() -> Self {
        Self(None)
    }

    pub fn cue(&self, sample: Option<i64>) {
        if let Some(messages) = &self.0 {
            let _ = messages.send(Message::Cue(sample));
        }
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        if let Some(messages) = self.messages.take() {
            let _ = messages.send(Message::Stop);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    writer: PoolWriter,
    pages: Box<dyn Pages>,
    transport: TransportReader,
    horizon: Horizon,
    scene: Arc<Scene>,
    cue: Option<i64>,
    stats: Arc<Stats>,
    wants: Vec<Want>,
    broken: HashSet<(u64, usize)>,
    left: Vec<f32>,
    right: Vec<f32>,
    busy: Duration,
    since: Instant,
}

impl Worker {
    fn run(mut self, inbox: &Receiver<Message>) {
        let mut wait = Duration::ZERO;
        loop {
            match inbox.recv_timeout(wait) {
                Ok(Message::Stop) => return,
                Ok(message) => self.take(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(message) = inbox.try_recv() {
                self.take(message);
            }
            wait = self.round();
            self.report();
        }
    }

    fn take(&mut self, message: Message) {
        match message {
            Message::Scene(scene) => {
                self.pages.keep(scene.streams());
                self.broken.clear();
                if scene.rate() > 0 {
                    self.horizon = Horizon::at(scene.rate());
                }
                self.scene = scene;
            }
            Message::Cue(cue) => self.cue = cue,
            Message::Stop => {}
        }
    }

    fn round(&mut self) -> Duration {
        if self.scene.is_empty() {
            return IDLE;
        }
        let transport = self.transport.read();
        self.cue = kept_cue(self.cue, transport.sample, self.horizon);
        let at = Position {
            sample: transport.sample,
            playing: transport.playing,
            cue: self.cue,
            marker: Some(self.scene.sample_at(transport.marker)),
        };
        self.writer.tick();
        self.scene.plan(at, self.horizon, &mut self.wants);
        let streams = self.scene.streams();
        let missing: Vec<Want> = self
            .wants
            .iter()
            .copied()
            .filter(|w| {
                let audio = &streams[w.stream];
                !self.writer.want(audio, w.page) && !self.broken.contains(&(audio.id(), w.page))
            })
            .collect();
        if missing.is_empty() {
            return IDLE;
        }
        self.writer.make_room();
        let short = missing.len().saturating_sub(self.writer.free());
        if short > 0 {
            self.writer.evict(short);
        }
        let started = Instant::now();
        let mut waiting = false;
        for want in missing {
            if self.writer.make_room() == 0 {
                self.busy += started.elapsed();
                return if self.writer.retiring() > 0 {
                    FULL
                } else {
                    IDLE
                };
            }
            let audio = &self.scene.streams()[want.stream];
            let read = self
                .pages
                .read(audio, want.page, &mut self.left, &mut self.right);
            match read {
                Ok(frames) => {
                    let frames = frames.min(PAGE);
                    self.writer.load(
                        audio,
                        want.page,
                        &self.left[..frames],
                        &self.right[..frames],
                    );
                }
                Err(MediaError::NotReady) => {
                    waiting = true;
                }
                Err(_) => {
                    self.broken.insert((audio.id(), want.page));
                    self.stats.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
            if started.elapsed() >= ROUND {
                break;
            }
        }
        self.busy += started.elapsed();
        if waiting { RETRY } else { Duration::ZERO }
    }

    fn report(&mut self) {
        let elapsed = self.since.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let load = (self.busy.as_secs_f32() / elapsed.as_secs_f32()).min(1.0);
            self.stats.load.store(load.to_bits(), Ordering::Relaxed);
            self.busy = Duration::ZERO;
            self.since = Instant::now();
        }
    }
}

fn kept_cue(cue: Option<i64>, sample: i64, horizon: Horizon) -> Option<i64> {
    cue.filter(|cue| !(*cue..cue + horizon.near.max(1)).contains(&sample))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod soak;
