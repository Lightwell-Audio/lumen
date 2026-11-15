use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use lumen_core::automation::Lane;
use lumen_core::time::{TempoMap, Ticks};
use lumen_core::warp::Marker;
use lumen_engine::{ClipAudio, PagePool, StreamAudio};
use lumen_project::model::Warp;

use super::{Engine, Origin, Pitch, Warped};
use crate::media::streams;

pub struct Request<'a> {
    pub warp: &'a Warp,
    pub source: &'a Arc<ClipAudio>,
    pub tempo: &'a TempoMap,
    pub rate: u32,
    pub origin: Ticks,
    pub reach: i64,
    pub pitch: Option<&'a Lane>,
}

const MIN_FRAMES: u64 = 1 << 20;

pub struct WarpedStreams {
    pool: Arc<PagePool>,
    made: HashMap<u64, Made>,
    scans: HashMap<u64, Arc<super::SliceScan>>,
}

struct Made {
    id: u64,
    audio: Arc<ClipAudio>,
}

impl WarpedStreams {
    pub fn new(pool: &Arc<PagePool>) -> Self {
        Self {
            pool: Arc::clone(pool),
            made: HashMap::new(),
            scans: HashMap::new(),
        }
    }

    pub fn pool(&self) -> &Arc<PagePool> {
        &self.pool
    }

    pub fn stream(&mut self, request: &Request<'_>) -> Option<Arc<ClipAudio>> {
        if request.reach <= 0
            || request.source.sample_rate() != request.rate
            || request.source.channels() == 0
        {
            return None;
        }
        let shape = Shape::of(request);
        let pitch = Pitch::of(request.warp, request.pitch);
        let key = key(request, &shape, &pitch);
        if let Some(made) = self.made.get(&key) {
            return Some(Arc::clone(&made.audio));
        }
        let channels = request.source.channels().min(2);
        let origin = match &**request.source {
            ClipAudio::Stream(stream) => Origin::File(stream.id()),
            ClipAudio::Memory { .. } => Origin::Memory(Arc::clone(request.source)),
        };
        let frames = shape.frames;
        let mut warped = Warped::new(
            origin,
            channels,
            request.rate,
            (shape.markers, request.warp.original_bpm),
            request.tempo.clone(),
            (shape.anchor, frames),
        );
        warped.engine = Engine::of(request.warp.mode, &pitch);
        warped.pitch = pitch;
        if warped.engine == Engine::Slices {
            let scan = self.scans.entry(source_key(request.source)).or_default();
            warped.scan = Arc::clone(scan);
        }
        let warped = Arc::new(warped);
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

    pub fn retain(&mut self, keep: &HashSet<u64>) {
        self.made.retain(|_, made| {
            let kept = keep.contains(&made.id);
            if !kept {
                streams::forget(made.id);
            }
            kept
        });
        self.scans.retain(|_, scan| Arc::strong_count(scan) > 1);
    }

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

struct Shape {
    anchor: Ticks,
    markers: Vec<Marker>,
    frames: u64,
}

impl Shape {
    fn of(request: &Request<'_>) -> Self {
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

fn source_key(source: &Arc<ClipAudio>) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    match &**source {
        ClipAudio::Stream(stream) => (0_u8, stream.id()).hash(&mut hasher),
        ClipAudio::Memory { .. } => (1_u8, Arc::as_ptr(source) as usize).hash(&mut hasher),
    }
    hasher.finish()
}

fn key(request: &Request<'_>, shape: &Shape, pitch: &Pitch) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    Engine::of(request.warp.mode, pitch).hash(&mut hasher);
    pitch.hash_into(&mut hasher);
    source_key(request.source).hash(&mut hasher);
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
