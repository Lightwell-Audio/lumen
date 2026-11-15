use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub const RING_SAMPLES: usize = 1 << 22;
pub const RING_CHUNKS: usize = 1 << 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub track: u64,
    pub device: u64,
    pub at: i64,
    pub frames: u32,
    pub channels: u8,
}

#[derive(Debug, Default)]
struct Stats {
    dropped_chunks: AtomicU64,
    dropped_frames: AtomicU64,
}

#[derive(Debug)]
pub struct CaptureSink {
    samples: rtrb::Producer<f32>,
    chunks: rtrb::Producer<Chunk>,
    stats: Arc<Stats>,
}

#[derive(Debug)]
pub struct CaptureReader {
    samples: rtrb::Consumer<f32>,
    chunks: rtrb::Consumer<Chunk>,
    stats: Arc<Stats>,
}

pub fn capture() -> (CaptureReader, CaptureSink) {
    capture_of(RING_SAMPLES, RING_CHUNKS)
}

fn capture_of(samples: usize, chunks: usize) -> (CaptureReader, CaptureSink) {
    let (samples_tx, samples_rx) = rtrb::RingBuffer::new(samples);
    let (chunks_tx, chunks_rx) = rtrb::RingBuffer::new(chunks);
    let stats = Arc::new(Stats::default());
    (
        CaptureReader {
            samples: samples_rx,
            chunks: chunks_rx,
            stats: Arc::clone(&stats),
        },
        CaptureSink {
            samples: samples_tx,
            chunks: chunks_tx,
            stats,
        },
    )
}

impl CaptureSink {
    pub fn push(
        &mut self,
        track: u64,
        device: u64,
        at: i64,
        left: &[f32],
        right: Option<&[f32]>,
    ) -> bool {
        let frames = left.len();
        debug_assert!(right.is_none_or(|r| r.len() == frames));
        let wanted = frames * if right.is_some() { 2 } else { 1 };
        let room = self.chunks.slots() >= 1 && self.samples.slots() >= wanted;
        if !room {
            self.stats.dropped_chunks.fetch_add(1, Ordering::Relaxed);
            self.stats
                .dropped_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
            return false;
        }
        let _ = self.samples.push_entire_slice(left);
        if let Some(right) = right {
            let _ = self.samples.push_entire_slice(right);
        }
        let _ = self.chunks.push(Chunk {
            track,
            device,
            at,
            frames: frames as u32,
            channels: if right.is_some() { 2 } else { 1 },
        });
        true
    }
}

impl CaptureReader {
    pub fn next(&mut self, samples: &mut Vec<f32>) -> Option<Chunk> {
        let chunk = self.chunks.pop().ok()?;
        let wanted = chunk.frames as usize * usize::from(chunk.channels);
        samples.clear();
        samples.resize(wanted, 0.0);
        let read = self.samples.pop_entire_slice(samples);
        debug_assert!(read.is_ok(), "a chunk's samples are in the ring");
        Some(chunk)
    }

    pub fn dropped_chunks(&self) -> u64 {
        self.stats.dropped_chunks.load(Ordering::Relaxed)
    }

    pub fn dropped_frames(&self) -> u64 {
        self.stats.dropped_frames.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(from: usize, frames: usize) -> Vec<f32> {
        (from..from + frames).map(|i| i as f32).collect()
    }

    #[test]
    fn a_chunk_comes_out_as_it_went_in() {
        let (mut reader, mut sink) = capture_of(1_000, 8);
        let mut buf = Vec::new();
        assert_eq!(reader.next(&mut buf), None);
        assert!(sink.push(7, 100, -5, &ramp(0, 4), None));
        assert!(sink.push(7, 104, -1, &ramp(10, 3), Some(&ramp(20, 3))));
        let first = reader.next(&mut buf).unwrap();
        assert_eq!(
            first,
            Chunk {
                track: 7,
                device: 100,
                at: -5,
                frames: 4,
                channels: 1
            }
        );
        assert_eq!(buf, ramp(0, 4));
        let second = reader.next(&mut buf).unwrap();
        assert_eq!((second.at, second.frames, second.channels), (-1, 3, 2));
        assert_eq!(buf, [10.0, 11.0, 12.0, 20.0, 21.0, 22.0]);
        assert_eq!(reader.next(&mut buf), None);
    }

    #[test]
    fn samples_survive_the_ring_wrapping_round() {
        let (mut reader, mut sink) = capture_of(100, 8);
        let mut buf = Vec::new();
        for round in 0..50 {
            let left = ramp(round * 7, 7);
            let right = ramp(round * 7 + 1_000, 7);
            assert!(sink.push(1, round as u64 * 7, 0, &left, Some(&right)));
            let chunk = reader.next(&mut buf).unwrap();
            assert_eq!(chunk.device, round as u64 * 7);
            assert_eq!(&buf[..7], &left[..], "round {round}");
            assert_eq!(&buf[7..], &right[..], "round {round}");
        }
    }

    #[test]
    fn a_full_ring_drops_the_whole_chunk_counts_it_and_goes_on() {
        let (mut reader, mut sink) = capture_of(10, 8);
        let mut buf = Vec::new();
        assert!(sink.push(1, 0, 0, &ramp(0, 6), None));
        assert!(!sink.push(1, 6, 6, &ramp(6, 6), None), "6 more do not fit");
        assert_eq!((reader.dropped_chunks(), reader.dropped_frames()), (1, 6));
        assert!(sink.push(1, 12, 12, &ramp(12, 4), None), "4 do");
        assert_eq!(reader.next(&mut buf).unwrap().device, 0);
        assert_eq!(reader.next(&mut buf).unwrap().device, 12);
        assert_eq!(buf, ramp(12, 4), "no sample of the dropped chunk is left");
        assert_eq!(reader.next(&mut buf), None);
    }

    #[test]
    fn too_many_chunks_are_dropped_even_when_samples_fit() {
        let (reader, mut sink) = capture_of(1_000, 2);
        assert!(sink.push(1, 0, 0, &[0.0; 4], None));
        assert!(sink.push(1, 4, 4, &[0.0; 4], None));
        assert!(!sink.push(1, 8, 8, &[0.0; 4], None));
        assert_eq!(reader.dropped_chunks(), 1);
    }
}
