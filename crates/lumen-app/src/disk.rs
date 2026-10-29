//! The disk thread (M9 slice R6, ADR-0109): keeps the pages of streamed
//! files that the engine will read next in its page pool. It plans from the
//! clip list the engine plays and the playhead the engine publishes
//! ([`plan::Scene`]), reads what is missing nearest first, and frees what is
//! furthest away when the pool is full. It never waits for the audio thread
//! and the audio thread never waits for it: a page that is late is an
//! underrun, which the clip player fades around and counts.

pub use lumen_engine::stream_plan as plan;

use std::collections::HashSet;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicI64, AtomicUsize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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
    /// The clip list the engine now plays (unit-test worker setup).
    #[cfg(test)]
    Scene(Arc<Scene>),
    /// A scene request participates in the same adoption order as cues.
    SceneRequest { scene: Arc<Scene>, generation: u64 },
    /// Direct cue for worker-level unit tests.
    #[cfg(test)]
    Cue(Option<i64>),
    /// Read from here first: the app is about to start or jump there.
    CueRequest {
        sample: Option<i64>,
        generation: u64,
    },
    /// Keep the cue until the audio thread has applied this command serial.
    FollowAfter(u64),
    /// End the thread. Sent rather than closing the channel, which the
    /// [`Cuer`]s keep open.
    Stop,
}

/// A gate is ready to release a held start only after the worker has planned
/// with the latest cue and scene, protected resident pages and filled the
/// selected cue prefix. Scene and
/// cue requests share a generation so a changed scene invalidates an older
/// adoption too. Production requests are issued serially by the UI thread;
/// generations also discard delayed messages from cloned cuers.
#[derive(Debug, Default)]
struct CueState {
    requested: AtomicU64,
    adopted: AtomicU64,
}

impl CueState {
    /// Allocates a wrapping serial for a cue or scene request. Since only
    /// the latest request matters, every live request is less than half a
    /// serial space from the one the worker last adopted.
    fn request(&self) -> u64 {
        self.requested
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    fn is_adopted(&self) -> bool {
        let requested = self.requested.load(Ordering::Acquire);
        let adopted = self.adopted.load(Ordering::Acquire);
        requested == adopted && self.requested.load(Ordering::Acquire) == requested
    }

    fn adopt(&self, generation: u64) {
        self.adopted.store(generation, Ordering::Release);
    }
}

/// What the thread reports.
#[derive(Debug, Default)]
pub struct Stats {
    /// The share of the last second spent reading, as `f32` bits.
    load: AtomicU32,
    /// Pages read that failed (a file gone or unreadable), all told.
    failed: AtomicU32,
    /// Lightweight worker state for diagnosing timing-soak misses.
    #[cfg(test)]
    worker_round: AtomicU64,
    #[cfg(test)]
    planned_sample: AtomicI64,
    #[cfg(test)]
    planned_cue: AtomicI64,
    #[cfg(test)]
    planned_cue_present: AtomicU32,
    #[cfg(test)]
    command_serial: AtomicU64,
    #[cfg(test)]
    read_stream: AtomicU64,
    #[cfg(test)]
    read_page: AtomicUsize,
    #[cfg(test)]
    read_elapsed_nanos: AtomicU64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(super) struct WorkerSnapshot {
    pub round: u64,
    pub sample: i64,
    pub cue: Option<i64>,
    pub command_serial: u64,
    pub read_stream: u64,
    pub read_page: usize,
    pub read_elapsed_nanos: u64,
}

impl Stats {
    /// The share of the last second the thread spent reading (0.0 to 1.0).
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    pub fn failed(&self) -> u32 {
        self.failed.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(super) fn worker_snapshot(&self) -> WorkerSnapshot {
        WorkerSnapshot {
            round: self.worker_round.load(Ordering::Relaxed),
            sample: self.planned_sample.load(Ordering::Relaxed),
            cue: (self.planned_cue_present.load(Ordering::Relaxed) != 0)
                .then(|| self.planned_cue.load(Ordering::Relaxed)),
            command_serial: self.command_serial.load(Ordering::Relaxed),
            read_stream: self.read_stream.load(Ordering::Relaxed),
            read_page: self.read_page.load(Ordering::Relaxed),
            read_elapsed_nanos: self.read_elapsed_nanos.load(Ordering::Relaxed),
        }
    }
}

/// The disk thread. Dropping it stops the thread.
pub struct Disk {
    messages: Option<Sender<Message>>,
    cues: Arc<CueState>,
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
        let cues = Arc::new(CueState::default());
        let stats = Arc::new(Stats::default());
        let worker = Worker::with_cues(
            pool,
            pages,
            transport,
            Arc::clone(&stats),
            Arc::clone(&cues),
        );
        let thread = std::thread::Builder::new()
            .name("lumen-disk".into())
            .spawn(move || worker.run(&inbox))
            .expect("spawn the disk thread");
        Self {
            messages: Some(messages),
            cues,
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
        let Some(messages) = &self.messages else {
            return;
        };
        let generation = self.cues.request();
        if messages
            .send(Message::SceneRequest {
                scene: scene.into(),
                generation,
            })
            .is_err()
            && self.cues.requested.load(Ordering::Acquire) == generation
        {
            self.cues.adopt(generation);
        }
    }

    /// Reads the audio after `sample` first, until the next cue.
    pub fn cue(&self, sample: Option<i64>) {
        self.cuer().cue(sample);
    }

    /// A handle that cues this thread from elsewhere (the transport, L A).
    pub fn cuer(&self) -> Cuer {
        Cuer(self.messages.as_ref().map(|sender| CuePort {
            sender: sender.clone(),
            state: Arc::clone(&self.cues),
        }))
    }
}

/// Cues a disk thread: [`Disk::cue`] from another owner. Does nothing once
/// the thread has stopped.
#[derive(Clone)]
pub struct Cuer(Option<CuePort>);

#[derive(Clone)]
struct CuePort {
    sender: Sender<Message>,
    state: Arc<CueState>,
}

impl Cuer {
    /// A cuer with no disk thread behind it, for tests of what is held.
    #[cfg(test)]
    pub fn idle() -> Self {
        Self(None)
    }

    /// Reads the audio after `sample` first, until the next cue.
    pub fn cue(&self, sample: Option<i64>) {
        if let Some(port) = &self.0 {
            let generation = port.state.request();
            if port
                .sender
                .send(Message::CueRequest { sample, generation })
                .is_err()
                && port.state.requested.load(Ordering::Acquire) == generation
            {
                port.state.adopt(generation);
            }
        }
    }

    /// Whether the worker has protected the latest plan and filled its
    /// capacity-limited cue read-ahead prefix. An idle cuer has no worker and
    /// imposes no wait.
    pub fn adopted(&self) -> bool {
        self.0.as_ref().is_none_or(|port| port.state.is_adopted())
    }

    /// Keeps the current cue prioritized until `serial` has been applied by
    /// the audio thread. Does nothing once the disk thread has stopped.
    pub fn follow_after(&self, serial: u64) {
        if let Some(port) = &self.0 {
            let _ = port.sender.send(Message::FollowAfter(serial));
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
    /// Latest accepted cue generation, for rejecting delayed cue messages.
    cue_generation: u64,
    /// Latest accepted cue or scene generation, whose plan gets acknowledged.
    plan_generation: u64,
    cues: Arc<CueState>,
    /// A committed cue is released only after its queued transport command
    /// appears in the audio thread's published snapshot.
    follow_after: Option<u64>,
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
    #[cfg(test)]
    fn new(
        pool: &Arc<PagePool>,
        pages: Box<dyn Pages>,
        transport: TransportReader,
        stats: Arc<Stats>,
    ) -> Self {
        Self::with_cues(pool, pages, transport, stats, Arc::new(CueState::default()))
    }

    fn with_cues(
        pool: &Arc<PagePool>,
        pages: Box<dyn Pages>,
        transport: TransportReader,
        stats: Arc<Stats>,
        cues: Arc<CueState>,
    ) -> Self {
        Self {
            writer: PoolWriter::new(pool),
            pages,
            transport,
            horizon: Horizon::at(0),
            scene: Arc::default(),
            cue: None,
            cue_generation: 0,
            plan_generation: 0,
            cues,
            follow_after: None,
            stats,
            wants: Vec::new(),
            broken: HashSet::new(),
            left: vec![0.0; PAGE],
            right: vec![0.0; PAGE],
            busy: Duration::ZERO,
            since: Instant::now(),
        }
    }

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
            #[cfg(test)]
            Message::Scene(scene) => {
                self.install_scene(scene);
            }
            Message::SceneRequest { scene, generation } => {
                if !serial_reached(generation, self.plan_generation) {
                    return;
                }
                self.install_scene(scene);
                self.plan_generation = generation;
            }
            #[cfg(test)]
            Message::Cue(cue) => {
                self.cue = cue;
                self.follow_after = None;
            }
            Message::CueRequest { sample, generation } => {
                // Cloned cuers can enqueue concurrently, so a delayed older
                // message must not replace a newer cue or scene request.
                if !serial_reached(generation, self.plan_generation)
                    || !serial_reached(generation, self.cue_generation)
                {
                    return;
                }
                self.cue = sample;
                self.follow_after = None;
                self.cue_generation = generation;
                self.plan_generation = generation;
            }
            Message::FollowAfter(serial) => self.follow_after = Some(serial),
            Message::Stop => {}
        }
    }

    fn install_scene(&mut self, scene: Arc<Scene>) {
        self.pages.keep(scene.streams());
        self.broken.clear();
        if scene.rate() > 0 {
            self.horizon = Horizon::at(scene.rate());
        }
        self.scene = scene;
    }

    /// Plans, frees what is not wanted when room is needed, and reads what is
    /// missing for a while. Returns how long to wait before the next round.
    fn round(&mut self) -> Duration {
        self.round_for(ROUND)
    }

    /// Direct regressions use an unlimited budget to test a complete logical
    /// round independently of scheduler preemption. The live worker always
    /// passes the five-millisecond read budget above.
    fn round_for(&mut self, budget: Duration) -> Duration {
        #[cfg(test)]
        self.stats.worker_round.fetch_add(1, Ordering::Relaxed);
        if self.scene.is_empty() {
            self.cues.adopt(self.plan_generation);
            return IDLE;
        }
        let transport = self.transport.read();
        #[cfg(test)]
        {
            self.stats
                .planned_sample
                .store(transport.sample, Ordering::Relaxed);
            self.stats
                .planned_cue
                .store(self.cue.unwrap_or_default(), Ordering::Relaxed);
            self.stats
                .planned_cue_present
                .store(u32::from(self.cue.is_some()), Ordering::Relaxed);
            self.stats
                .command_serial
                .store(transport.command_serial, Ordering::Relaxed);
        }
        if self
            .follow_after
            .is_some_and(|serial| serial_reached(transport.command_serial, serial))
        {
            self.cue = None;
            self.follow_after = None;
        }
        let at = Position {
            sample: transport.sample,
            playing: transport.playing,
            cue: self.cue,
            marker: Some(self.scene.sample_at(transport.marker)),
        };
        self.writer.tick();
        let cue_prefix = self.scene.plan(at, self.horizon, &mut self.wants);
        // The plan is ordered by priority. Do not let pages in a lower-priority
        // tail protect themselves from eviction when the pool cannot hold the
        // whole plan: only the prefix that can fit is wanted this round.
        self.wants.truncate(self.writer.pool().pages() as usize);
        let cue_prefix = cue_prefix.min(self.wants.len());
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
        // `want` above protects resident pages, but a cue is not adopted
        // until its capacity-limited priority prefix is resident. This lets
        // the existing read-ahead horizon prefill before a held start is
        // released. `Cuer::adopted` still rejects this acknowledgement if a
        // newer request arrived while the round was in flight.
        self.adopt_if_cue_prefix_resident(cue_prefix);
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
            if self.writer.free() == 0 && self.writer.reclaim() == 0 {
                self.busy += started.elapsed();
                // Pages the audio thread may still read are free in a block;
                // otherwise the pool holds all it can of what is wanted.
                self.adopt_if_cue_prefix_resident(cue_prefix);
                return if self.writer.retiring() > 0 {
                    FULL
                } else {
                    IDLE
                };
            }
            let audio = &self.scene.streams()[want.stream];
            #[cfg(test)]
            let read_started = Instant::now();
            let read = self
                .pages
                .read(audio, want.page, &mut self.left, &mut self.right);
            #[cfg(test)]
            {
                self.stats.read_stream.store(audio.id(), Ordering::Relaxed);
                self.stats.read_page.store(want.page, Ordering::Relaxed);
                self.stats.read_elapsed_nanos.store(
                    read_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
                    Ordering::Relaxed,
                );
            }
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
            if started.elapsed() >= budget {
                break;
            }
        }
        self.busy += started.elapsed();
        self.adopt_if_cue_prefix_resident(cue_prefix);
        Duration::ZERO
    }

    /// Publishes adoption only when every selected cue page is resident.
    /// With no cue, the normal planning round is enough to acknowledge a
    /// scene request. A pool smaller than the full cue horizon acknowledges
    /// its resident capacity-limited prefix; the gate separately checks the
    /// pages its start actually needs.
    fn adopt_if_cue_prefix_resident(&self, cue_prefix: usize) {
        if self.cue.is_none()
            || self.wants[..cue_prefix]
                .iter()
                .all(|want| self.scene.streams()[want.stream].resident(want.page))
        {
            self.cues.adopt(self.plan_generation);
        }
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

/// Whether `current` is at or after `target` in a wrapping 64-bit serial
/// space. Command and cue requests cannot span half the serial space.
fn serial_reached(current: u64, target: u64) -> bool {
    current.wrapping_sub(target) < (1_u64 << 63)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "disk/cue_tests.rs"]
mod cue_tests;

#[cfg(test)]
mod soak;
