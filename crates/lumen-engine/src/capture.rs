//! The audio thread's half of recording (M9 slice R2, ADR-0102): input that an
//! armed track records is copied into a ring, and a worker thread writes it to
//! disk. The audio thread never waits for the disk (hard rule 1): when the
//! ring is full the chunk is dropped and counted, and the worker pads the gap
//! with silence so the take stays in time.
//!
//! The ring holds *chunks*: the frames one render part took from one route,
//! stamped with where they are.
//!
//! - `device` is the stream's frame count at the chunk's first frame. Chunks of
//!   one track follow each other without a jump in it, unless a chunk was
//!   dropped: that is how the worker tells a gap from a loop wrap.
//! - `at` is the timeline sample *heard* at that frame, before the device's own
//!   latency (as MIDI messages are placed, ADR-0046). It jumps back at a loop
//!   wrap and may be negative while playback prerolls.
//!
//! Samples are pushed first, the header after, so a header always has its
//! samples in the ring. A chunk is all or nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Samples the ring holds, all channels together: 5.5 s of 16 channels at
/// 48 kHz, or 43 s of a stereo pair. Reserved, not touched, until a take uses it.
pub const RING_SAMPLES: usize = 1 << 22;
/// Chunks the ring holds: a part per armed track per block, so 32 tracks at
/// 64 frames fill it in 5.5 s, about when the samples do. 32 bytes each.
pub const RING_CHUNKS: usize = 1 << 17;

/// The frames one route took in one render part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// The track recording it.
    pub track: u64,
    /// The stream's frame count at the first frame.
    pub device: u64,
    /// The timeline sample heard at the first frame.
    pub at: i64,
    /// Frames per channel.
    pub frames: u32,
    /// One for a mono input, two for a pair.
    pub channels: u8,
}

/// What the audio side could not hand over.
#[derive(Debug, Default)]
struct Stats {
    /// Chunks dropped because the ring was full.
    dropped_chunks: AtomicU64,
    /// Frames in them.
    dropped_frames: AtomicU64,
}

/// Audio side. Real-time safe.
#[derive(Debug)]
pub struct CaptureSink {
    samples: rtrb::Producer<f32>,
    chunks: rtrb::Producer<Chunk>,
    stats: Arc<Stats>,
}

/// Worker side: reads what the audio side kept.
#[derive(Debug)]
pub struct CaptureReader {
    samples: rtrb::Consumer<f32>,
    chunks: rtrb::Consumer<Chunk>,
    stats: Arc<Stats>,
}

/// Makes the pair. Allocates: call it off the audio thread.
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
    /// Keeps a chunk: `left`, and `right` for a pair (the same length).
    /// Returns false when the ring had no room and the chunk was dropped.
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
        // Room was checked, so neither push can fail.
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
    /// The oldest chunk, with its samples in `samples` (the left channel, then
    /// the right for a pair), or `None` when the ring is empty. May allocate.
    pub fn next(&mut self, samples: &mut Vec<f32>) -> Option<Chunk> {
        let chunk = self.chunks.pop().ok()?;
        let wanted = chunk.frames as usize * usize::from(chunk.channels);
        samples.clear();
        samples.resize(wanted, 0.0);
        // Samples go in before the header, so they are all there.
        let read = self.samples.pop_entire_slice(samples);
        debug_assert!(read.is_ok(), "a chunk's samples are in the ring");
        Some(chunk)
    }

    /// Chunks dropped so far because the ring was full.
    pub fn dropped_chunks(&self) -> u64 {
        self.stats.dropped_chunks.load(Ordering::Relaxed)
    }

    /// Frames in the chunks dropped so far.
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
