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

pub const IN_MEMORY_SECONDS: f64 = 30.0;

pub const POOL_PAGES: u32 = 3_072;

pub struct Streaming {
    pool: Arc<PagePool>,
    disk: Disk,
    cache: Arc<Cache>,
}

impl Streaming {
    pub fn start(
        cache_dir: PathBuf,
        limit: u64,
        transport: TransportReader,
        slow: Option<Duration>,
    ) -> Self {
        let pool = PagePool::new(POOL_PAGES);
        let readers = Readers::polling();
        let pages: Box<dyn Pages> = match slow {
            Some(wait) => Box::new(Slow { readers, wait }),
            None => Box::new(readers),
        };
        let disk = Disk::start(&pool, pages, transport);
        let cache = Cache::new(cache_dir);
        cache.set_limit(limit);
        Self {
            pool,
            disk,
            cache: Arc::new(cache),
        }
    }

    pub fn set_cache_limit(&self, bytes: u64) {
        self.cache.set_limit(bytes);
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

pub fn streams(header: &Header) -> bool {
    match (header.frames, header.sample_rate) {
        (Some(frames), Some(rate)) if rate > 0 => {
            frames as f64 / f64::from(rate) >= IN_MEMORY_SECONDS
        }
        _ => true,
    }
}

pub(super) enum Loaded {
    Memory(lumen_media::Audio),
    Stream(Prepared),
}

pub(super) struct Streamed {
    pub source: (u32, usize, u64, f64),
    pub prepared: Prepared,
    pub peaks: Option<Peaks>,
}

pub(super) fn load(
    cache: &Cache,
    path: &Path,
    header: &Header,
    rate: u32,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(f32),
) -> Result<Streamed, MediaError> {
    let prepared = cache.prepare(path, rate, cancel, &mut |p| report(p * 0.9))?;
    let peaks = cache.peaks(&prepared.path, cancel, &mut |p| {
        report(0.9 + p * 0.1);
    })?;
    if peaks.is_none() {
        return Err(MediaError::Cancelled);
    }
    let mut playing = streams::in_use();
    playing.push(prepared.path.clone());
    let cleared = cache.trim_to_limit(&playing);
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
