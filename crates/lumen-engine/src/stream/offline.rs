use std::sync::{Arc, Mutex};

use super::{PAGE, PageLoader, PagePool, PoolWriter, StreamAudio};

pub type ReadPage = dyn FnMut(&StreamAudio, usize, &mut [f32], &mut [f32]) -> Option<usize> + Send;

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
