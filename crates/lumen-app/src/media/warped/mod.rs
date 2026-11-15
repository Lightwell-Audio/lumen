mod pitch;
mod provider;
mod source;

pub use pitch::{Identity, Part, Pitch, merge as merge_pitch};
pub use provider::{Request, WarpedStreams};
pub(super) use source::Rendering;

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use lumen_core::time::{TempoMap, Ticks};
use lumen_core::warp::{self, Marker};
use lumen_engine::ClipAudio;
use lumen_media::slicer;
use lumen_media::stretch::Preset;
use lumen_project::model::WarpMode;

#[derive(Clone)]
pub enum Origin {
    Memory(Arc<ClipAudio>),
    File(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Engine {
    Repitch,
    Stretch(Preset),
    Slices,
}

impl Engine {
    pub fn of(mode: WarpMode, pitch: &Pitch) -> Self {
        match mode {
            WarpMode::Repitch => Engine::Repitch,
            WarpMode::Rhythmic => Engine::Slices,
            WarpMode::Melodic if pitch::needs_full_preset(mode, pitch) => {
                Engine::Stretch(Preset::Full)
            }
            WarpMode::Melodic => Engine::Stretch(Preset::Light),
            WarpMode::FullMix => Engine::Stretch(Preset::Full),
        }
    }
}

pub struct Warped {
    pub origin: Origin,
    pub engine: Engine,
    pub pitch: Pitch,
    pub channels: usize,
    pub rate: u32,
    pub markers: Vec<Marker>,
    pub original_bpm: f64,
    pub tempo: TempoMap,
    anchor_ticks: f64,
    pub frames: u64,
    slices: OnceLock<Vec<i64>>,
    scan: Arc<SliceScan>,
}

#[derive(Default)]
pub struct SliceScan {
    state: Mutex<Scan>,
}

#[derive(Default)]
enum Scan {
    #[default]
    Idle,
    Running,
    Done(Arc<Vec<f64>>),
    Failed(String),
}

enum Poll {
    Ready(Arc<Vec<f64>>),
    Pending,
    Failed(String),
}

impl SliceScan {
    fn lock(&self) -> std::sync::MutexGuard<'_, Scan> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
            pitch: Pitch::default(),
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

    fn cut(&self, hits: &[f64], frames: u64) -> &[i64] {
        let pinned = self.markers.iter().map(|m| m.source);
        let starts = slicer::starts(hits.iter().copied(), pinned, self.rate, frames);
        self.slices.get_or_init(|| starts)
    }

    #[cfg(test)]
    fn slice_starts_for_test(&self) -> &[i64] {
        self.slices.get().map_or(&[][..], Vec::as_slice)
    }

    pub fn cents_at(&self, k: i64) -> f64 {
        let seconds = k as f64 / f64::from(self.rate);
        self.pitch
            .cents_at(self.tempo.ticks_f64_at_seconds(seconds))
    }

    pub fn position(&self, k: i64) -> f64 {
        let seconds = k as f64 / f64::from(self.rate);
        let ticks = self.tempo.ticks_f64_at_seconds(seconds) - self.anchor_ticks;
        warp::source_at_f64(&self.markers, self.original_bpm, ticks) * f64::from(self.rate)
    }
}

#[cfg(test)]
mod tests;
