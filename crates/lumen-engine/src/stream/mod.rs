//! Audio read from disk as it plays (M9 slice R6, ADR-0109).
//!
//! A streamed file is cut into pages of [`PAGE`] frames. A [`PagePool`]
//! holds a fixed number of pages, allocated once. Each streamed file has a
//! page table ([`StreamAudio`]): for each of its pages, the pool page that
//! holds it, or none. The audio thread reads the table and the samples and
//! never waits: a page that is not there is a miss, and the clip player
//! fades the clip out and back in around it.
//!
//! One thread writes a pool ([`PoolWriter`]): it fills a free page, then
//! publishes it in the table. It takes a page out of a table before it uses
//! it again, and waits for a grace period first: the audio thread counts
//! the blocks it reads in ([`PagePool::enter`] and [`PagePool::leave`]), and
//! a page is reused only once no block that could have seen it is still
//! running. So the audio thread never reads a page while it is rewritten.
//!
//! Samples are stored as `f32` bits in atomics, read and written with
//! relaxed ordering (plain loads and stores on every target); the table
//! entries carry the ordering. No lock, no allocation and no unsafe code on
//! either side.

mod offline;
pub mod plan;
mod writer;

pub use offline::{OfflineLoader, ReadPage};
pub use writer::PoolWriter;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Frames per page: about 85 ms at 48 kHz.
pub const PAGE: usize = 4_096;
/// Samples per page: two channels, interleaved (a mono file uses the first).
const STRIDE: usize = 2;
/// A table entry with no page.
const NONE: u32 = u32::MAX;

/// A fixed set of pages and the count of blocks that read them.
pub struct PagePool {
    samples: Box<[AtomicU32]>,
    pages: u32,
    /// Odd while the audio thread is in a block that reads pages.
    epoch: AtomicU64,
    /// Times a clip had to fade out because its audio was not read in time.
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
    /// A pool of `pages` pages. Allocates; call off the audio thread.
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

    /// Bytes of sample memory the pool holds.
    pub fn memory(&self) -> usize {
        self.samples.len() * std::mem::size_of::<AtomicU32>()
    }

    /// The audio thread starts a block that may read pages. Real-time safe.
    pub fn enter(&self) {
        let was = self.epoch.fetch_add(1, Ordering::SeqCst);
        debug_assert!(was.is_multiple_of(2), "a block entered twice");
    }

    /// The audio thread ends the block. Real-time safe.
    pub fn leave(&self) {
        let was = self.epoch.fetch_add(1, Ordering::SeqCst);
        debug_assert!(!was.is_multiple_of(2), "a block left without entering");
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// Counts an underrun. Real-time safe.
    pub fn underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Underruns since the pool was made.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

struct Table {
    /// The id the app gave the file, to match it in the disk thread's plan.
    id: u64,
    sample_rate: u32,
    channels: u16,
    frames: u64,
    pool: Arc<PagePool>,
    entries: Box<[AtomicU32]>,
    /// Loads a missing page on the spot: only for an offline render's
    /// streams, never for those the audio thread plays.
    loader: Option<Arc<dyn PageLoader>>,
}

/// Loads pages of an offline render's streams when the clip player finds
/// them missing, on the rendering thread. Blocks; never given to a stream the
/// audio thread plays.
pub trait PageLoader: Send + Sync {
    /// Loads `audio`'s `page` into its pool, if it can.
    fn load(&self, audio: &StreamAudio, page: usize);
}

/// A file played from disk: its shape and its page table. Cheap to clone;
/// clones share the table.
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
    /// An empty table for a file of `frames` frames and `channels` (one or
    /// two played) channels, in `pool`. Allocates; call off the audio thread.
    pub fn new(
        pool: &Arc<PagePool>,
        id: u64,
        sample_rate: u32,
        channels: u16,
        frames: u64,
    ) -> Self {
        Self::table(pool, id, (sample_rate, channels, frames), None)
    }

    /// As [`StreamAudio::new`], for an offline render: a page the clip player
    /// finds missing is loaded on the spot by `loader`, so the render never
    /// starves. Never give one to the audio thread.
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

    /// Whether this stream loads its own pages (an offline render's).
    pub fn is_offline(&self) -> bool {
        self.0.loader.is_some()
    }

    pub fn id(&self) -> u64 {
        self.0.id
    }

    pub fn sample_rate(&self) -> u32 {
        self.0.sample_rate
    }

    /// One or two.
    pub fn channels(&self) -> usize {
        usize::from(self.0.channels)
    }

    pub fn frames(&self) -> u64 {
        self.0.frames
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.0.pool
    }

    /// Pages in the file.
    pub fn pages(&self) -> usize {
        self.0.entries.len()
    }

    #[inline]
    fn entry(&self, page: usize) -> Option<u32> {
        // Sequentially consistent, so that a page the writer takes out after
        // seeing no block in progress is never read by the next block
        // ([`PoolWriter`]); a plain load on x86 and the same as acquire on ARM.
        let entry = self.0.entries.get(page)?.load(Ordering::SeqCst);
        (entry != NONE).then_some(entry)
    }

    /// Whether the file's `page` is in memory. Real-time safe.
    #[inline]
    pub fn resident(&self, page: usize) -> bool {
        self.entry(page).is_some()
    }

    /// Whether `page` is in memory, after loading it if this is an offline
    /// stream. Real-time safe for the streams the audio thread plays, which
    /// load nothing.
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

    /// One channel's samples, read through the page table. Real-time safe.
    pub fn side(&self, channel: usize) -> StreamSide<'_> {
        StreamSide {
            audio: self,
            samples: &self.pool().samples,
            frames: self.frames(),
            channel: channel.min(self.channels() - 1),
            last: std::cell::Cell::new((usize::MAX, MISSING)),
        }
    }
}

/// Where [`StreamSide`] remembers a page that is not in memory.
const MISSING: usize = usize::MAX;

/// One channel of a streamed file as the clip player reads it, remembering
/// the last page it looked up. The pool's samples and the file's length are
/// looked up once, so a sample costs an index and a load.
pub struct StreamSide<'a> {
    audio: &'a StreamAudio,
    samples: &'a [AtomicU32],
    frames: u64,
    channel: usize,
    /// The last page looked up, and where its first sample of this channel
    /// is in `samples` ([`MISSING`]: not in memory).
    last: std::cell::Cell<(usize, usize)>,
}

impl StreamSide<'_> {
    /// The sample at `frame`, or `None` past the end or when its page is not
    /// in memory. Real-time safe.
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
            // An offline stream loads what it misses.
            let entry = match self.audio.entry(page) {
                Some(entry) => entry,
                None if self.audio.ensure(page) => self.audio.entry(page).unwrap_or(NONE),
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
