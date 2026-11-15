use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use super::{NONE, PAGE, PagePool, STRIDE, StreamAudio, Table};

struct Held {
    table: Weak<Table>,
    page: usize,
    used: u64,
}

pub struct PoolWriter {
    pool: Arc<PagePool>,
    free: Vec<u32>,
    held: Vec<Option<Held>>,
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

    pub fn free(&self) -> usize {
        self.free.len()
    }

    pub fn retiring(&self) -> usize {
        self.retiring.len()
    }

    pub fn used(&self) -> usize {
        self.held.iter().filter(|h| h.is_some()).count()
    }

    pub fn tick(&mut self) {
        self.clock += 1;
    }

    pub fn want(&mut self, audio: &StreamAudio, page: usize) -> bool {
        let Some(entry) = audio.entry(page) else {
            return false;
        };
        if let Some(held) = self.held[entry as usize].as_mut() {
            held.used = self.clock;
        }
        true
    }

    pub fn load(&mut self, audio: &StreamAudio, page: usize, left: &[f32], right: &[f32]) -> bool {
        assert!(
            Arc::ptr_eq(audio.pool(), &self.pool),
            "a stream is written by its own pool's writer"
        );
        if page >= audio.pages() || audio.resident(page) {
            return true;
        }
        self.make_room();
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
        audio.0.entries[page].store(entry, Ordering::SeqCst);
        true
    }

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
            self.free.push(entry);
        }
    }

    pub fn reclaim(&mut self) -> usize {
        self.retire();
        self.sweep();
        self.free.len()
    }

    pub fn make_room(&mut self) -> usize {
        self.retire();
        if self.free.is_empty() {
            self.sweep();
        }
        self.free.len()
    }

    fn retire(&mut self) {
        let now = self.pool.epoch();
        let free = &mut self.free;
        self.retiring.retain(|&(entry, epoch)| {
            let passed = epoch.is_multiple_of(2) || now != epoch;
            if passed {
                free.push(entry);
            }
            !passed
        });
    }

    fn sweep(&mut self) {
        for (entry, held) in self.held.iter_mut().enumerate() {
            if held.as_ref().is_some_and(|h| h.table.strong_count() == 0) {
                *held = None;
                self.free.push(entry as u32);
            }
        }
    }
}
