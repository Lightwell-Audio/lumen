//! Warped clips as streams (M9.5 S4b, ADR-0131).
//!
//! A warped clip plays through the same page pool and disk thread as a file
//! read from disk (ADR-0109), as a stream whose frames are computed instead of
//! read: **frame `k` of it is the song at timeline sample `k`**, the file read
//! at the position the clip's warp map gives there
//! ([`lumen_media::repitch`]). The engine plays it as any streamed clip (an
//! offset of the clip's start sample, the clip's own fades), the disk thread
//! prepares its pages ahead of the playhead, and an offline render loads them
//! on its own thread, so what is heard and what is exported are the same bits.
//!
//! A stream does not know which clip plays it, only the line from timeline to
//! file: the pieces a split leaves share one stream, so the engine hears the
//! seam between them as no seam. A looping clip is not a stream of its own:
//! it is played as the real clips it unloops into (`lanes::engine_clips_with`),
//! each following the tempo and edge-faded as any clip is.
//!
//! Streams are keyed by what they are made from ([`provider`]): an edit makes
//! a new stream, and the engine keeps playing the old one until the new
//! one's pages are there.

mod provider;
mod source;

pub use provider::{Request, WarpedStreams};
pub(super) use source::Rendering;

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use lumen_core::time::{TempoMap, Ticks};
use lumen_core::warp::{self, Marker};
use lumen_engine::ClipAudio;
use lumen_media::slicer;
use lumen_media::stretch::Preset;
use lumen_project::model::WarpMode;

/// Where a warped stream reads the file it plays.
#[derive(Clone)]
pub enum Origin {
    /// Audio held in memory.
    Memory(Arc<ClipAudio>),
    /// A stream file, by the id [`super::streams::register`] gave it.
    File(u64),
}

/// How a warped stream turns the file into the song's frames (the warp
/// mode, as far as it is heard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Engine {
    /// The file read faster or slower, its pitch following (S4b).
    Repitch,
    /// The file stretched without changing its pitch (S4c).
    Stretch(Preset),
    /// The file cut at its transients, each slice placed where the map puts
    /// it and played at the file's own speed (S5b, Rhythmic).
    Slices,
}

impl Engine {
    /// The engine a mode plays with. Rhythmic is Lumen's own slicer.
    pub fn of(mode: WarpMode) -> Self {
        match mode {
            WarpMode::Repitch => Engine::Repitch,
            WarpMode::Rhythmic => Engine::Slices,
            WarpMode::Melodic => Engine::Stretch(Preset::Light),
            WarpMode::FullMix => Engine::Stretch(Preset::Full),
        }
    }
}

/// What a warped stream is made from: the file, the map and the tempo it
/// plays against. Everything its frames depend on.
pub struct Warped {
    pub origin: Origin,
    pub engine: Engine,
    /// The pitch shift in cents the stretch engines play with (S6), 0 for
    /// none. Only the stretch engines take it (see `provider::transpose_of`).
    pub transpose_cents: i32,
    /// Channels played, one or two.
    pub channels: usize,
    /// The rate of file and stream.
    pub rate: u32,
    /// The map, measured from the anchor.
    pub markers: Vec<Marker>,
    pub original_bpm: f64,
    pub tempo: TempoMap,
    /// The anchor is the place in the song of the first marker, which does not
    /// move when a clip is split (what lets the pieces share a stream). This is
    /// the tick of the sample nearest it (between ticks): the grid the stream
    /// is aligned to, as a plain clip is aligned to its start sample, so a clip
    /// at its file's tempo is the file's own frames. The map is measured from it.
    anchor_ticks: f64,
    /// Frames in the stream: the song up to the last sample it must reach.
    pub frames: u64,
    /// The first frames of the slices of a Rhythmic stream: the file's
    /// transients and the markers' places, found the first time a worker reads
    /// the stream and then kept, so every reader cuts the file alike.
    slices: OnceLock<Vec<i64>>,
    /// The transients of the file, shared by every stream made from it.
    scan: Arc<SliceScan>,
}

/// The transients of one file, found once for all the streams made from it
/// (an edit to a map makes a new stream; the file is not read again).
#[derive(Default)]
pub struct SliceScan {
    state: Mutex<Scan>,
}

#[derive(Default)]
enum Scan {
    #[default]
    Idle,
    /// A worker is reading the file.
    Running,
    Done(Arc<Vec<f64>>),
    /// The worker could not read it. Said once to the reader that asks, then
    /// the next asks again.
    Failed(String),
}

/// Where a scan stands, for a reader that does not wait.
enum Poll {
    Ready(Arc<Vec<f64>>),
    Pending,
    Failed(String),
}

impl SliceScan {
    fn lock(&self) -> std::sync::MutexGuard<'_, Scan> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The transients, in seconds, read through `input` now if nobody has yet
    /// (a render, which has nothing else to do while it waits). Whoever
    /// finishes first is kept: the result is the same.
    fn hits(
        &self,
        input: &mut dyn lumen_media::repitch::Source,
        rate: u32,
    ) -> Result<Arc<Vec<f64>>, lumen_media::MediaError> {
        if let Scan::Done(found) = &*self.lock() {
            return Ok(Arc::clone(found));
        }
        let found = Arc::new(slicer::scan(input, rate)?);
        *self.lock() = Scan::Done(Arc::clone(&found));
        Ok(found)
    }

    /// Where the scan stands; `start` is run (once, by the first to ask) to
    /// begin it on a worker.
    fn poll(&self, start: impl FnOnce()) -> Poll {
        let mut state = self.lock();
        match &*state {
            Scan::Done(found) => Poll::Ready(Arc::clone(found)),
            Scan::Running => Poll::Pending,
            Scan::Failed(message) => {
                let message = message.clone();
                *state = Scan::Idle;
                Poll::Failed(message)
            }
            Scan::Idle => {
                *state = Scan::Running;
                drop(state);
                start();
                Poll::Pending
            }
        }
    }

    /// The worker's answer.
    fn finish(&self, result: Result<Vec<f64>, lumen_media::MediaError>) {
        *self.lock() = match result {
            Ok(found) => Scan::Done(Arc::new(found)),
            Err(error) => Scan::Failed(error.to_string()),
        };
    }
}

impl Warped {
    pub fn new(
        origin: Origin,
        channels: usize,
        rate: u32,
        (markers, original_bpm): (Vec<Marker>, f64),
        tempo: TempoMap,
        (anchor, frames): (Ticks, u64),
    ) -> Self {
        let anchor_sample = tempo.sample_at(anchor, rate);
        let anchor_ticks = tempo.ticks_f64_at_seconds(anchor_sample as f64 / f64::from(rate));
        Self {
            origin,
            engine: Engine::Repitch,
            transpose_cents: 0,
            channels,
            rate,
            markers,
            original_bpm,
            tempo,
            anchor_ticks,
            frames,
            slices: OnceLock::new(),
            scan: Arc::default(),
        }
    }

    /// Where a Rhythmic stream's slices start, in frames of the file: the
    /// transients of the file (read through `input` the first time anything
    /// plays this file, then shared by every stream of it) and the places the
    /// markers pin. For a reader that can wait. A scan that fails is an
    /// error, kept nowhere, so the next read tries again and no stream is ever
    /// cut differently from another.
    pub(super) fn slice_starts(
        &self,
        input: &mut dyn lumen_media::repitch::Source,
    ) -> Result<&[i64], lumen_media::MediaError> {
        if let Some(starts) = self.slices.get() {
            return Ok(starts);
        }
        let hits = self.scan.hits(input, self.rate)?;
        Ok(self.cut(&hits, input.frames()))
    }

    /// [`Warped::slice_starts`] for a reader that must not wait (the disk
    /// thread's): the file is read on a worker of its own, and until that has
    /// finished the answer is [`lumen_media::MediaError::NotReady`].
    pub(super) fn slice_starts_polled(this: &Arc<Self>) -> Result<&[i64], lumen_media::MediaError> {
        use lumen_media::MediaError;
        if let Some(starts) = this.slices.get() {
            return Ok(starts);
        }
        let worker = Arc::clone(this);
        match this.scan.poll(move || worker.start_scan()) {
            Poll::Ready(hits) => {
                let frames = source::frames_of(&this.origin).ok_or(MediaError::NotReady)?;
                Ok(this.cut(&hits, frames))
            }
            Poll::Pending => Err(MediaError::NotReady),
            Poll::Failed(message) => Err(MediaError::Io(message)),
        }
    }

    /// Begins the scan of this stream's file on a worker.
    fn start_scan(self: Arc<Self>) {
        let this = Arc::clone(&self);
        let spawned = std::thread::Builder::new()
            .name("lumen slice scan".into())
            .spawn(move || {
                let result = source::scan_file(&this.origin, this.channels, this.rate);
                this.scan.finish(result);
            });
        if let Err(error) = spawned {
            self.scan
                .finish(Err(lumen_media::MediaError::Io(error.to_string())));
        }
    }

    /// The slice starts for `hits` in a file of `frames`, kept.
    fn cut(&self, hits: &[f64], frames: u64) -> &[i64] {
        let pinned = self.markers.iter().map(|m| m.source);
        let starts = slicer::starts(hits.iter().copied(), pinned, self.rate, frames);
        self.slices.get_or_init(|| starts)
    }

    /// The slice starts a reader has found (tests).
    #[cfg(test)]
    fn slice_starts_for_test(&self) -> &[i64] {
        self.slices.get().map_or(&[][..], Vec::as_slice)
    }

    /// The frame of the file, between frames, that plays at stream frame `k`.
    /// Depends on nothing but `k` ([`lumen_media::repitch`]).
    pub fn position(&self, k: i64) -> f64 {
        let seconds = k as f64 / f64::from(self.rate);
        let ticks = self.tempo.ticks_f64_at_seconds(seconds) - self.anchor_ticks;
        warp::source_at_f64(&self.markers, self.original_bpm, ticks) * f64::from(self.rate)
    }
}

#[cfg(test)]
mod tests;
