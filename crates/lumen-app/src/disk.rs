//! The disk thread (M9 slice R6, ADR-0108): keeps the pages of streamed
//! files that the engine will read next in its page pool. It plans from the
//! clip list the engine plays and the playhead the engine publishes
//! ([`plan::Scene`]), reads what is missing nearest first, and frees what is
//! furthest away when the pool is full. It never waits for the audio thread
//! and the audio thread never waits for it: a page that is late is an
//! underrun, which the clip player fades around and counts.

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

/// How long the thread sleeps when it has nothing to read.
const IDLE: Duration = Duration::from_millis(10);
/// How long it reads before it plans again, so a jump is followed at once.
const ROUND: Duration = Duration::from_millis(5);
/// How long it waits when the pool is full of pages the audio thread may be
/// reading (the grace period is one block).
const FULL: Duration = Duration::from_millis(2);

/// Reads pages of streamed files. The real one reads stream files
/// ([`Readers`]); tests inject slow or failing ones.
pub trait Pages: Send {
    /// Reads `audio`'s `page` into `left` and `right` ([`PAGE`] frames each;
    /// `right` is left alone for a mono file). Returns the frames read.
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError>;

    /// The files of the scene are now `streams`: readers of others can go.
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

/// What the app tells the thread.
enum Message {
    /// The clip list the engine now plays.
    Scene(Arc<Scene>),
    /// Read from here first: the app is about to start or jump there.
    Cue(Option<i64>),
    /// End the thread. Sent rather than closing the channel, which the
    /// [`Cuer`]s keep open.
    Stop,
}

/// What the thread reports.
#[derive(Debug, Default)]
pub struct Stats {
    /// The share of the last second spent reading, as `f32` bits.
    load: AtomicU32,
    /// Pages read that failed (a file gone or unreadable), all told.
    failed: AtomicU32,
}

impl Stats {
    /// The share of the last second the thread spent reading (0.0 to 1.0).
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    pub fn failed(&self) -> u32 {
        self.failed.load(Ordering::Relaxed)
    }
}

/// The disk thread. Dropping it stops the thread.
pub struct Disk {
    messages: Option<Sender<Message>>,
    pool: Arc<PagePool>,
    stats: Arc<Stats>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Disk {
    /// Starts the thread that fills `pool`, reading through `pages`, following
    /// the transport `transport` reads. It reads ahead by the rate of each
    /// clip list it is given.
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
        let thread = std::thread::Builder::new()
            .name("lumen-disk".into())
            .spawn(move || worker.run(&inbox))
            .expect("spawn the disk thread");
        Self {
            messages: Some(messages),
            pool: Arc::clone(pool),
            stats,
            thread: Some(thread),
        }
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.pool
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The clip list the engine now plays.
    pub fn set_scene(&self, scene: impl Into<Arc<Scene>>) {
        self.send(Message::Scene(scene.into()));
    }

    /// Reads the audio after `sample` first, until the next cue.
    pub fn cue(&self, sample: Option<i64>) {
        self.send(Message::Cue(sample));
    }

    /// A handle that cues this thread from elsewhere (the transport, L A).
    pub fn cuer(&self) -> Cuer {
        Cuer(self.messages.clone())
    }

    fn send(&self, message: Message) {
        if let Some(messages) = &self.messages {
            // The thread only ends when this handle drops.
            let _ = messages.send(message);
        }
    }
}

/// Cues a disk thread: [`Disk::cue`] from another owner. Does nothing once
/// the thread has stopped.
#[derive(Clone)]
pub struct Cuer(Option<Sender<Message>>);

impl Cuer {
    /// A cuer with no disk thread behind it, for tests of what is held.
    #[cfg(test)]
    pub fn idle() -> Self {
        Self(None)
    }

    /// Reads the audio after `sample` first, until the next cue.
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
    /// Pages that could not be read, by file id: not tried again until the
    /// clip list changes.
    broken: HashSet<(u64, usize)>,
    left: Vec<f32>,
    right: Vec<f32>,
    /// Time spent reading since `since`.
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

    /// Plans, frees what is not wanted when room is needed, and reads what is
    /// missing for a while. Returns how long to wait before the next round.
    fn round(&mut self) -> Duration {
        if self.scene.is_empty() {
            return IDLE;
        }
        let transport = self.transport.read();
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
        self.writer.reclaim();
        let short = missing.len().saturating_sub(self.writer.free());
        if short > 0 {
            self.writer.evict(short);
        }
        let started = Instant::now();
        for want in missing {
            if self.writer.reclaim() == 0 {
                self.busy += started.elapsed();
                // Pages the audio thread may still read are free in a block;
                // otherwise the pool holds all it can of what is wanted.
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
                Err(_) => {
                    // Left out: the clip stays silent there, and the app
                    // shows the file as missing when it next checks.
                    self.broken.insert((audio.id(), want.page));
                    self.stats.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
            if started.elapsed() >= ROUND {
                break;
            }
        }
        self.busy += started.elapsed();
        Duration::ZERO
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod soak;
