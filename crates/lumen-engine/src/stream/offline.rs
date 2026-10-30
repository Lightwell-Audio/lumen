//! Streams for offline renders (bounce, join, freeze, export): a render's own
//! pool, filled on the render's thread as the clip player reads, so a render
//! never starves and plays exactly what is in the files. Not real-time code.

use std::sync::{Arc, Mutex};

use super::{PAGE, PageLoader, PagePool, PoolWriter, StreamAudio};

/// Reads `audio`'s `page` into `left` and `right` ([`PAGE`] frames each;
/// `right` is left alone for a mono file); returns the frames read, or
/// `None` when the file cannot be read (that page stays silent).
pub type ReadPage = dyn FnMut(&StreamAudio, usize, &mut [f32], &mut [f32]) -> Option<usize> + Send;

/// A [`PageLoader`] over one pool, reading through a function.
pub struct OfflineLoader {
    pool: Arc<PagePool>,
    inner: Mutex<Inner>,
}

struct Inner {
    writer: PoolWriter,
    read: Box<ReadPage>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl OfflineLoader {
    /// A loader of `pool`'s pages, reading with `read`. The pool must have
    /// at least two pages and no other writer.
    pub fn new(pool: &Arc<PagePool>, read: Box<ReadPage>) -> Arc<Self> {
        Arc::new(Self {
            pool: Arc::clone(pool),
            inner: Mutex::new(Inner {
                writer: PoolWriter::new(pool),
                read,
                left: vec![0.0; PAGE],
                right: vec![0.0; PAGE],
            }),
        })
    }

    /// `stream` as an offline stream of this loader's pool, with the same id
    /// and shape.
    pub fn stream(self: &Arc<Self>, stream: &StreamAudio) -> StreamAudio {
        StreamAudio::offline(
            &self.pool,
            stream.id(),
            (
                stream.sample_rate(),
                stream.channels() as u16,
                stream.frames(),
            ),
            Arc::clone(self) as Arc<dyn PageLoader>,
        )
    }
}

impl PageLoader for OfflineLoader {
    fn load(&self, audio: &StreamAudio, page: usize) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let Inner {
            writer,
            read,
            left,
            right,
        } = &mut *inner;
        writer.tick();
        // A page taken out while a block renders is free only after the
        // block, so half the pool is kept free for what a block reads.
        let reserve = (writer.pool().pages() as usize / 2).max(1);
        if writer.reclaim() < reserve {
            writer.evict(reserve);
            writer.reclaim();
        }
        if let Some(frames) = read(audio, page, left, right) {
            let frames = frames.min(PAGE);
            writer.load(audio, page, &left[..frames], &right[..frames]);
        }
    }
}
