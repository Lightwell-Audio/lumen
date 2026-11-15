mod offline;
pub mod plan;
mod writer;

pub use offline::{OfflineLoader, ReadPage};
pub use writer::PoolWriter;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const PAGE: usize = 4_096;
const STRIDE: usize = 2;
const NONE: u32 = u32::MAX;

pub struct PagePool {
    samples: Box<[AtomicU32]>,
    pages: u32,
    epoch: AtomicU64,
    underruns: AtomicU64,
}

impl std::fmt::Debug for PagePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PagePool")
            .field("pages", &self.pages)
            .finish_non_exhaustive()
    }
}

impl PagePool {
    pub fn new(pages: u32) -> Arc<Self> {
        let pages = pages.min(NONE - 1);
        let samples = (0..pages as usize * PAGE * STRIDE)
            .map(|_| AtomicU32::new(0))
            .collect();
        Arc::new(Self {
            samples,
            pages,
            epoch: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        })
    }

    pub fn pages(&self) -> u32 {
        self.pages
    }

    pub fn memory(&self) -> usize {
        self.samples.len() * std::mem::size_of::<AtomicU32>()
    }

    pub fn enter(&self) {
        let was = self.epoch.fetch_add(1, Ordering::SeqCst);
        debug_assert!(was.is_multiple_of(2), "a block entered twice");
    }

    pub fn leave(&self) {
        let was = self.epoch.fetch_add(1, Ordering::SeqCst);
        debug_assert!(!was.is_multiple_of(2), "a block left without entering");
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    pub fn underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

struct Table {
    id: u64,
    sample_rate: u32,
    channels: u16,
    frames: u64,
    pool: Arc<PagePool>,
    entries: Box<[AtomicU32]>,
    loader: Option<Arc<dyn PageLoader>>,
}

pub trait PageLoader: Send + Sync {
    fn load(&self, audio: &StreamAudio, page: usize);
}

#[derive(Clone)]
pub struct StreamAudio(Arc<Table>);

impl std::fmt::Debug for StreamAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamAudio")
            .field("id", &self.0.id)
            .field("sample_rate", &self.0.sample_rate)
            .field("channels", &self.0.channels)
            .field("frames", &self.0.frames)
            .finish_non_exhaustive()
    }
}

impl PartialEq for StreamAudio {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl StreamAudio {
    pub fn new(
        pool: &Arc<PagePool>,
        id: u64,
        sample_rate: u32,
        channels: u16,
        frames: u64,
    ) -> Self {
        Self::table(pool, id, (sample_rate, channels, frames), None)
    }

    pub fn offline(
        pool: &Arc<PagePool>,
        id: u64,
        (sample_rate, channels, frames): (u32, u16, u64),
        loader: Arc<dyn PageLoader>,
    ) -> Self {
        Self::table(pool, id, (sample_rate, channels, frames), Some(loader))
    }

    fn table(
        pool: &Arc<PagePool>,
        id: u64,
        (sample_rate, channels, frames): (u32, u16, u64),
        loader: Option<Arc<dyn PageLoader>>,
    ) -> Self {
        let pages = frames.div_ceil(PAGE as u64) as usize;
        Self(Arc::new(Table {
            id,
            sample_rate,
            channels: channels.clamp(1, 2),
            frames,
            pool: Arc::clone(pool),
            entries: (0..pages).map(|_| AtomicU32::new(NONE)).collect(),
            loader,
        }))
    }

    pub fn is_offline(&self) -> bool {
        self.0.loader.is_some()
    }

    pub fn id(&self) -> u64 {
        self.0.id
    }

    pub fn sample_rate(&self) -> u32 {
        self.0.sample_rate
    }

    pub fn channels(&self) -> usize {
        usize::from(self.0.channels)
    }

    pub fn frames(&self) -> u64 {
        self.0.frames
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.0.pool
    }

    pub fn pages(&self) -> usize {
        self.0.entries.len()
    }

    #[inline]
    fn entry(&self, page: usize) -> Option<u32> {
        let entry = self.0.entries.get(page)?.load(Ordering::SeqCst);
        (entry != NONE).then_some(entry)
    }

    #[inline]
    pub fn resident(&self, page: usize) -> bool {
        self.entry(page).is_some()
    }

    #[inline]
    pub fn ensure(&self, page: usize) -> bool {
        if self.resident(page) {
            return true;
        }
        match &self.0.loader {
            Some(loader) if page < self.pages() => {
                loader.load(self, page);
                self.resident(page)
            }
            _ => false,
        }
    }

    pub fn side(&self, channel: usize) -> StreamSide<'_> {
        self.side_with::<NoLoader>(channel)
    }

    pub fn side_with<L: PageLoad>(&self, channel: usize) -> StreamSide<'_, L> {
        StreamSide {
            audio: self,
            samples: &self.pool().samples,
            frames: self.frames(),
            channel: channel.min(self.channels() - 1),
            last: std::cell::Cell::new((usize::MAX, MISSING)),
            policy: std::marker::PhantomData,
        }
    }
}

const MISSING: usize = usize::MAX;

pub trait PageLoad: Send {
    fn ensure(stream: &StreamAudio, page: usize) -> bool;
}

pub struct NoLoader;

impl PageLoad for NoLoader {
    #[inline]
    fn ensure(stream: &StreamAudio, page: usize) -> bool {
        stream.resident(page)
    }
}

pub struct LoadPages;

impl PageLoad for LoadPages {
    fn ensure(stream: &StreamAudio, page: usize) -> bool {
        stream.ensure(page)
    }
}

pub struct StreamSide<'a, L = NoLoader> {
    audio: &'a StreamAudio,
    samples: &'a [AtomicU32],
    frames: u64,
    channel: usize,
    last: std::cell::Cell<(usize, usize)>,
    policy: std::marker::PhantomData<L>,
}

impl<L: PageLoad> StreamSide<'_, L> {
    #[inline]
    pub fn at(&self, frame: usize) -> Option<f32> {
        if frame as u64 >= self.frames {
            return None;
        }
        let page = frame / PAGE;
        let (seen, base) = self.last.get();
        let base = if seen == page {
            base
        } else {
            let entry = match self.audio.entry(page) {
                Some(entry) => entry,
                None if L::ensure(self.audio, page) => self.audio.entry(page).unwrap_or(NONE),
                None => NONE,
            };
            let base = if entry == NONE {
                MISSING
            } else {
                entry as usize * PAGE * STRIDE + self.channel
            };
            self.last.set((page, base));
            base
        };
        if base == MISSING {
            return None;
        }
        let at = base + (frame % PAGE) * STRIDE;
        Some(f32::from_bits(self.samples[at].load(Ordering::Relaxed)))
    }
}

#[cfg(test)]
mod tests;
