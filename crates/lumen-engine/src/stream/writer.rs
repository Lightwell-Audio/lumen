//! The one writer of a page pool, on the disk thread (or an offline render's
//! own thread). Not real-time code: it may allocate and is never called from
//! the audio thread.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use super::{NONE, PAGE, PagePool, STRIDE, StreamAudio, Table};

/// What a pool page holds.
struct Held {
    table: Weak<Table>,
    page: usize,
    /// The writer's clock when the page was last wanted.
    used: u64,
}

/// Fills, publishes and frees the pages of one pool. There must be only one
/// per pool: the grace period counts one reading thread.
pub struct PoolWriter {
    pool: Arc<PagePool>,
    free: Vec<u32>,
    held: Vec<Option<Held>>,
    /// Pages taken out of their tables, and the epoch when they were: free
    /// once no block that began before is still running.
    retiring: Vec<(u32, u64)>,
    clock: u64,
}

impl PoolWriter {
    pub fn new(pool: &Arc<PagePool>) -> Self {
        Self {
            pool: Arc::clone(pool),
            free: (0..pool.pages).rev().collect(),
            held: (0..pool.pages).map(|_| None).collect(),
            retiring: Vec::new(),
            clock: 0,
        }
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.pool
    }

    /// Pages free to fill now.
    pub fn free(&self) -> usize {
        self.free.len()
    }

    /// Pages taken out of their tables and not yet free.
    pub fn retiring(&self) -> usize {
        self.retiring.len()
    }

    /// Pages holding audio.
    pub fn used(&self) -> usize {
        self.held.iter().filter(|h| h.is_some()).count()
    }

    /// Starts a new round of planning: pages wanted from now on count as
    /// more recent than all before.
    pub fn tick(&mut self) {
        self.clock += 1;
    }

    /// Marks `audio`'s `page` as wanted, if it is in memory; returns whether
    /// it is.
    pub fn want(&mut self, audio: &StreamAudio, page: usize) -> bool {
        let Some(entry) = audio.entry(page) else {
            return false;
        };
        if let Some(held) = self.held[entry as usize].as_mut() {
            held.used = self.clock;
        }
        true
    }

    /// Puts `left` and `right` (one channel each, at most [`PAGE`] frames;
    /// `right` is ignored for a mono file) into a free page and publishes it
    /// as `audio`'s `page`. Returns false when no page is free; a page
    /// already in memory is left as it is.
    pub fn load(&mut self, audio: &StreamAudio, page: usize, left: &[f32], right: &[f32]) -> bool {
        assert!(
            Arc::ptr_eq(audio.pool(), &self.pool),
            "a stream is written by its own pool's writer"
        );
        if page >= audio.pages() || audio.resident(page) {
            return true;
        }
        self.reclaim();
        let Some(entry) = self.free.pop() else {
            return false;
        };
        let base = entry as usize * PAGE * STRIDE;
        let frames = left.len().min(PAGE);
        let samples = &self.pool.samples[base..base + PAGE * STRIDE];
        for (i, frame) in samples.as_chunks::<STRIDE>().0.iter().enumerate() {
            let l = if i < frames { left[i] } else { 0.0 };
            let r = right.get(i).copied().filter(|_| i < frames).unwrap_or(l);
            frame[0].store(l.to_bits(), Ordering::Relaxed);
            frame[1].store(r.to_bits(), Ordering::Relaxed);
        }
        self.held[entry as usize] = Some(Held {
            table: Arc::downgrade(&audio.0),
            page,
            used: self.clock,
        });
        // Publishes the samples stored above (sequentially consistent, which
        // includes release).
        audio.0.entries[page].store(entry, Ordering::SeqCst);
        true
    }

    /// Takes up to `count` pages out of their tables, least recently wanted
    /// first, skipping those wanted in the current round. They become free
    /// after the grace period ([`PoolWriter::reclaim`]). Returns how many.
    pub fn evict(&mut self, count: usize) -> usize {
        let mut candidates: Vec<(u64, u32)> = self
            .held
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                let h = h.as_ref()?;
                (h.used < self.clock).then_some((h.used, i as u32))
            })
            .collect();
        candidates.sort_unstable();
        let mut evicted = 0;
        for &(_, entry) in candidates.iter().take(count) {
            self.unpublish(entry);
            evicted += 1;
        }
        evicted
    }

    /// Takes every page of `audio` out of its table.
    pub fn drop_stream(&mut self, audio: &StreamAudio) {
        for page in 0..audio.pages() {
            if let Some(entry) = audio.entry(page) {
                self.unpublish(entry);
            }
        }
    }

    fn unpublish(&mut self, entry: u32) {
        let Some(held) = self.held[entry as usize].take() else {
            return;
        };
        if let Some(table) = held.table.upgrade() {
            table.entries[held.page].store(NONE, Ordering::SeqCst);
            self.retiring.push((entry, self.pool.epoch()));
        } else {
            // No clip holds the file any more, so no block can read it.
            self.free.push(entry);
        }
    }

    /// Frees retired pages whose grace period has passed, and pages of files
    /// no clip holds. Returns the pages free.
    pub fn reclaim(&mut self) -> usize {
        let now = self.pool.epoch();
        let free = &mut self.free;
        self.retiring.retain(|&(entry, epoch)| {
            // Even: no block was running when the page was taken out, and any
            // later block reads the table after it. Odd: wait for that block
            // to end.
            let passed = epoch.is_multiple_of(2) || now != epoch;
            if passed {
                free.push(entry);
            }
            !passed
        });
        for (entry, held) in self.held.iter_mut().enumerate() {
            if held.as_ref().is_some_and(|h| h.table.strong_count() == 0) {
                *held = None;
                self.free.push(entry as u32);
            }
        }
        self.free.len()
    }
}
