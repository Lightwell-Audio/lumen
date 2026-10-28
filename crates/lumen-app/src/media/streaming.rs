//! Playing audio from disk (M9 slice R6, ADR-0108), when "Play audio from
//! disk" is on: the live page pool the engine reads streamed clips from, the
//! disk thread that fills it, and the cache of converted files.
//!
//! What streams is automatic (the owner's M B): a file shorter than
//! [`IN_MEMORY_SECONDS`] is decoded into memory as it always was, so
//! one-shots and drum hits never wait for the disk; a longer one plays from
//! its stream file (the file itself, or a converted copy in the cache).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use lumen_engine::{ClipAudio, PagePool, StreamAudio, TransportReader};
use lumen_media::stream::cache::{Cache, Prepared};
use lumen_media::{Header, MediaError, Peaks};

use super::State;
use super::streams::{self, Readers};
use crate::disk::{Disk, Pages};

/// Files shorter than this stay in memory (M B). Settled by measurement in
/// R6 step 7.
pub const IN_MEMORY_SECONDS: f64 = 30.0;

/// Pages in the live pool: 96 MB, about 2 s ahead for each of 64 stereo
/// tracks at 48 kHz with room for loop starts and markers. Settled by
/// measurement in R6 step 7.
pub const POOL_PAGES: u32 = 3_072;

/// The pool, the disk thread and the cache, while streaming is on.
pub struct Streaming {
    pool: Arc<PagePool>,
    disk: Disk,
    cache: Arc<Cache>,
}

impl Streaming {
    /// Starts the disk thread over a new pool, following the transport
    /// `transport` reads, with the cache in `cache_dir`. Tests pass `slow`
    /// to make every page read wait that long first.
    pub fn start(cache_dir: PathBuf, transport: TransportReader, slow: Option<Duration>) -> Self {
        let pool = PagePool::new(POOL_PAGES);
        let readers = Readers::default();
        let pages: Box<dyn Pages> = match slow {
            Some(wait) => Box::new(Slow { readers, wait }),
            None => Box::new(readers),
        };
        let disk = Disk::start(&pool, pages, transport);
        Self {
            pool,
            disk,
            cache: Arc::new(Cache::new(cache_dir)),
        }
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.pool
    }

    pub fn disk(&self) -> &Disk {
        &self.disk
    }

    pub(super) fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }
}

/// Page reads that wait before each read: a slow disk, for tests of late
/// audio.
struct Slow {
    readers: Readers,
    wait: Duration,
}

impl Pages for Slow {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        std::thread::sleep(self.wait);
        Pages::read(&mut self.readers, audio, page, left, right)
    }

    fn keep(&mut self, streams: &[StreamAudio]) {
        Pages::keep(&mut self.readers, streams);
    }
}

/// Whether a file with `header` plays from disk: one at least
/// [`IN_MEMORY_SECONDS`] long, or of a length its header does not say.
pub fn streams(header: &Header) -> bool {
    match (header.frames, header.sample_rate) {
        (Some(frames), Some(rate)) if rate > 0 => {
            frames as f64 / f64::from(rate) >= IN_MEMORY_SECONDS
        }
        _ => true,
    }
}

/// What a load made, on its thread.
pub(super) enum Loaded {
    Memory(lumen_media::Audio),
    Stream(Prepared),
}

/// A load that streams: the file's own rate, channels and length, its stream
/// file (converted now if it needs to be, with `report` getting 0 to 0.9),
/// and its peaks read from that file.
pub(super) struct Streamed {
    pub source: (u32, usize, u64, f64),
    pub prepared: Prepared,
    pub peaks: Option<Peaks>,
}

/// Loads `path` to stream at `rate`, given its `header`. Blocks: the load
/// thread's work. `Err(Cancelled)` if `cancel` was set.
pub(super) fn load(
    cache: &Cache,
    path: &Path,
    header: &Header,
    rate: u32,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(f32),
) -> Result<Streamed, MediaError> {
    let prepared = cache.prepare(path, rate, cancel, &mut |p| report(p * 0.9))?;
    // Kept in the cache, so a file opened again shows its waveform at once.
    let peaks = cache.peaks(&prepared.path, cancel, &mut |p| {
        report(0.9 + p * 0.1);
    })?;
    if peaks.is_none() {
        return Err(MediaError::Cancelled);
    }
    // Within its limit, keeping what plays now and what was just made.
    let mut playing = streams::in_use();
    playing.push(prepared.path.clone());
    let cleared = cache.trim(lumen_media::stream::cache::LIMIT, &playing);
    if cleared > 0 {
        crate::trace::log(|| format!("cache: cleared {cleared} bytes"));
    }
    let own = header.sample_rate.unwrap_or(prepared.layout.sample_rate);
    let frames = header.frames.unwrap_or_else(|| {
        (prepared.layout.frames as f64 * f64::from(own) / f64::from(rate.max(1))).round() as u64
    });
    let channels = header
        .channels
        .unwrap_or(usize::from(prepared.layout.channels));
    Ok(Streamed {
        source: (own, channels, frames, frames as f64 / f64::from(own.max(1))),
        prepared,
        peaks,
    })
}

/// Forgets the stream file of `state`, if it streams: called when an item's
/// audio is replaced or the item goes.
pub(super) fn release(state: &State) {
    if let State::Ready(audio) = state
        && let ClipAudio::Stream(stream) = &**audio
    {
        streams::forget(stream.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_files_stay_in_memory_and_long_or_unknown_ones_stream() {
        let header = |frames, rate| Header {
            sample_rate: rate,
            channels: Some(2),
            frames,
        };
        assert!(!streams(&header(Some(29 * 48_000), Some(48_000))));
        assert!(streams(&header(Some(30 * 48_000), Some(48_000))));
        assert!(streams(&header(Some(31 * 44_100), Some(44_100))));
        assert!(streams(&header(None, Some(48_000))), "a length not known");
        assert!(streams(&header(Some(10), None)));
    }
}
