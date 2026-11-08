//! The warped streams of a project, made once and kept while they play.
//!
//! A clip list is built again at every edit, and a stream is expensive: its
//! pages are what the disk thread prepares ahead of the playhead. So a stream
//! is keyed by everything its frames depend on, and an edit that changes none
//! of it (a gain, a fade, another track's clip) finds the stream it had and the
//! pages already prepared. An edit that changes it (a marker, the tempo, a move)
//! makes a new stream, and the engine keeps playing the old one until it is
//! replaced.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use lumen_core::time::{TempoMap, Ticks};
use lumen_core::warp::Marker;
use lumen_engine::{ClipAudio, PagePool, StreamAudio};
use lumen_project::model::Warp;

use super::{Origin, Warped};
use crate::media::streams;

/// What a warped clip is played from: everything the stream depends on.
pub struct Request<'a> {
    pub warp: &'a Warp,
    /// The file's audio at the device rate.
    pub source: &'a Arc<ClipAudio>,
    pub tempo: &'a TempoMap,
    pub rate: u32,
    /// The clip's content origin: where its map's ticks start.
    pub origin: Ticks,
    /// The last timeline sample any clip of the project reaches: a stream is
    /// as long as the song, rounded up, so that the pieces of one clip share
    /// it, and so does every other clip of the file.
    pub reach: i64,
}

/// The shortest a stream is: about 22 seconds at 48 kHz.
const MIN_FRAMES: u64 = 1 << 20;

/// The streams made so far, by what they are made from.
pub struct WarpedStreams {
    pool: Arc<PagePool>,
    made: HashMap<u64, Made>,
}

struct Made {
    id: u64,
    audio: Arc<ClipAudio>,
}

impl WarpedStreams {
    /// Streams with tables in `pool`. A pool that nothing reads (an offline
    /// render swaps in its own tables) can be a small one.
    pub fn new(pool: &Arc<PagePool>) -> Self {
        Self {
            pool: Arc::clone(pool),
            made: HashMap::new(),
        }
    }

    /// The pool the streams' tables are in.
    pub fn pool(&self) -> &Arc<PagePool> {
        &self.pool
    }

    /// The stream for `request`, made if it is new. `None` when the clip
    /// cannot be played warped: the file is at another rate than the device, or
    /// it holds nothing, or the song reaches no sample.
    pub fn stream(&mut self, request: &Request<'_>) -> Option<Arc<ClipAudio>> {
        if request.reach <= 0
            || request.source.sample_rate() != request.rate
            || request.source.channels() == 0
        {
            return None;
        }
        let shape = Shape::of(request);
        let key = key(request, &shape);
        if let Some(made) = self.made.get(&key) {
            return Some(Arc::clone(&made.audio));
        }
        let channels = request.source.channels().min(2);
        let origin = match &**request.source {
            ClipAudio::Stream(stream) => Origin::File(stream.id()),
            ClipAudio::Memory { .. } => Origin::Memory(Arc::clone(request.source)),
        };
        let frames = shape.frames;
        let warped = Arc::new(Warped::new(
            origin,
            channels,
            request.rate,
            (shape.markers, request.warp.original_bpm),
            request.tempo.clone(),
            (shape.anchor, frames),
        ));
        let id = streams::register_warped(warped);
        let stream = StreamAudio::new(&self.pool, id, request.rate, channels as u16, frames);
        let audio = Arc::new(ClipAudio::Stream(stream));
        self.made.insert(
            key,
            Made {
                id,
                audio: Arc::clone(&audio),
            },
        );
        Some(audio)
    }

    /// Forgets the streams not among `keep` (the ids of the streams the
    /// engine plays or is about to). Call after a list is sent.
    pub fn retain(&mut self, keep: &HashSet<u64>) {
        self.made.retain(|_, made| {
            let kept = keep.contains(&made.id);
            if !kept {
                streams::forget(made.id);
            }
            kept
        });
    }

    /// How many streams are held.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.made.len()
    }
}

impl Drop for WarpedStreams {
    fn drop(&mut self) {
        for made in self.made.values() {
            streams::forget(made.id);
        }
    }
}

/// What a request says about the line from timeline to file, without the
/// clip it came from: the same for the pieces a split leaves.
struct Shape {
    /// The tick the map is measured from (see [`Warped::anchor`]).
    anchor: Ticks,
    /// The map, measured from `anchor`.
    markers: Vec<Marker>,
    frames: u64,
}

impl Shape {
    fn of(request: &Request<'_>) -> Self {
        // A split moves a clip's origin and shifts its markers by the same
        // ticks, so the first marker's place in the song is what stays.
        let first = request.warp.markers.first().map_or(Ticks::ZERO, |m| m.at);
        let anchor = request.origin + first;
        let markers = request
            .warp
            .markers
            .iter()
            .map(|m| Marker::new(m.source, m.at - first))
            .collect();
        let frames = (request.reach as u64).max(MIN_FRAMES).next_power_of_two();
        Self {
            anchor,
            markers,
            frames,
        }
    }
}

/// Hashes everything a stream's frames depend on. The mode and the transpose
/// are left out: until S4c and S6 every mode plays as Repitch.
fn key(request: &Request<'_>, shape: &Shape) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    match &**request.source {
        ClipAudio::Stream(stream) => (0_u8, stream.id()).hash(&mut hasher),
        // The audio itself, not a copy of it: the same Arc is the same audio.
        ClipAudio::Memory { .. } => (1_u8, Arc::as_ptr(request.source) as usize).hash(&mut hasher),
    }
    request.rate.hash(&mut hasher);
    shape.frames.hash(&mut hasher);
    shape.anchor.0.hash(&mut hasher);
    request.warp.original_bpm.to_bits().hash(&mut hasher);
    for marker in &shape.markers {
        marker.source.to_bits().hash(&mut hasher);
        marker.at.0.hash(&mut hasher);
    }
    for point in request.tempo.points() {
        point.at.0.hash(&mut hasher);
        point.bpm.to_bits().hash(&mut hasher);
        point.ramp.hash(&mut hasher);
    }
    hasher.finish()
}
