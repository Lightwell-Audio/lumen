//! The disk thread (M9 slice R6, ADR-0108): keeps the pages of streamed
//! files that the engine will read next in its page pool. It plans from the
//! clip list the engine plays and the playhead the engine publishes
//! ([`plan::Scene`]), reads what is missing nearest first, and frees what is
//! furthest away when the pool is full. It never waits for the audio thread
//! and the audio thread never waits for it: a page that is late is an
//! underrun, which the clip player fades around and counts.

pub mod plan;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_engine::{PAGE, PagePool, PoolWriter, StreamAudio, TransportReader};
use lumen_media::MediaError;
use lumen_media::stream::Reader;

use plan::{Horizon, Position, Scene, Want};

/// How long the thread sleeps when it has nothing to read.
const IDLE: Duration = Duration::from_millis(10);
/// How long it reads before it plans again, so a jump is followed at once.
const ROUND: Duration = Duration::from_millis(5);
/// How long it waits when the pool is full of pages the audio thread may be
/// reading (the grace period is one block).
const FULL: Duration = Duration::from_millis(2);

/// Reads pages of streamed files. The real one reads stream files
/// ([`Files`]); tests inject slow or failing ones.
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

/// Stream files on disk, by the id the library gave each.
#[derive(Default)]
pub struct Files {
    paths: Arc<Mutex<HashMap<u64, PathBuf>>>,
    open: HashMap<u64, Reader>,
}

impl Files {
    /// The paths the app registers files under, shared with the thread.
    pub fn paths(&self) -> Arc<Mutex<HashMap<u64, PathBuf>>> {
        Arc::clone(&self.paths)
    }
}

impl Pages for Files {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let id = audio.id();
        if !self.open.contains_key(&id) {
            let path = self
                .paths
                .lock()
                .map_err(|_| MediaError::Io("the stream file table was poisoned".to_owned()))?
                .get(&id)
                .cloned()
                .ok_or_else(|| MediaError::Io(format!("no stream file for media {id}")))?;
            self.open.insert(id, Reader::open(&path)?);
        }
        let reader = self.open.get_mut(&id).expect("opened above");
        let from = (page * PAGE) as u64;
        if audio.channels() == 1 || reader.layout().channels == 1 {
            reader.read(from, &mut [left])
        } else {
            reader.read(from, &mut [left, right])
        }
    }

    fn keep(&mut self, streams: &[StreamAudio]) {
        self.open
            .retain(|id, _| streams.iter().any(|s| s.id() == *id));
    }
}

/// What the app tells the thread.
enum Message {
    /// The clip list the engine now plays.
    Scene(Scene),
    /// Read from here first: the app is about to start or jump there.
    Cue(Option<i64>),
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
    /// the transport `transport` reads, at `rate`.
    pub fn start(
        pool: &Arc<PagePool>,
        pages: Box<dyn Pages>,
        transport: TransportReader,
        rate: u32,
    ) -> Self {
        let (messages, inbox) = channel();
        let stats = Arc::new(Stats::default());
        let worker = Worker {
            writer: PoolWriter::new(pool),
            pages,
            transport,
            horizon: Horizon::at(rate),
            scene: Scene::default(),
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
    pub fn set_scene(&self, scene: Scene) {
        self.send(Message::Scene(scene));
    }

    /// Reads the audio after `sample` first, until the next cue.
    pub fn cue(&self, sample: Option<i64>) {
        self.send(Message::Cue(sample));
    }

    fn send(&self, message: Message) {
        if let Some(messages) = &self.messages {
            // The thread only ends when this handle drops.
            let _ = messages.send(message);
        }
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        self.messages = None;
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
    scene: Scene,
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
                self.scene = scene;
            }
            Message::Cue(cue) => self.cue = cue,
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
