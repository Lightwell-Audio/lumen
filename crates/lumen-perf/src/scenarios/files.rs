//! The file-side scenarios: the disk thread's plan, tick and sample
//! conversions, and a quality-bar project's serialization.

use std::sync::Arc;

use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_engine::stream_plan::Scene;
use lumen_engine::{Clip, ClipAudio, PagePool, StreamAudio, Timeline};
use lumen_project::model::Project;
use lumen_project::testing::synthetic;

use super::RATE;

/// The disk thread's scene: 64 streamed files of a minute each, one per
/// track, with a loop and two markers.
pub(crate) fn streamed_scene() -> Scene {
    let pool = PagePool::new(1);
    let frames = 60 * u64::from(RATE);
    let clips: Vec<Clip> = (0..64u64)
        .map(|i| Clip {
            envelope: Default::default(),
            id: i,
            start: Ticks::ZERO,
            offset: 0,
            length: frames as i64,
            gain: 1.0,
            track: i,
            audio: Arc::new(ClipAudio::Stream(StreamAudio::new(
                &pool,
                i + 1,
                RATE,
                2,
                frames,
            ))),
            shape: Default::default(),
        })
        .collect();
    // A loop of two seconds from 8 s; markers at 20 s and 40 s.
    let rate = i64::from(RATE);
    Scene::new(
        &clips,
        &Timeline::default(),
        RATE,
        Some((8 * rate, 10 * rate)),
        vec![0, 8 * rate, 20 * rate, 40 * rate],
    )
}

pub(crate) fn busy_tempo_map() -> TempoMap {
    TempoMap::new(
        (0..64)
            .map(|i| TempoPoint {
                at: Ticks::quarters(i * 8),
                bpm: 80.0 + (i % 16) as f64 * 7.5,
                ramp: false,
            })
            .collect(),
    )
    .expect("valid tempo map")
}

pub(crate) fn quality_bar_project() -> Project {
    synthetic(42, 200, 100, 10, 4_096)
}
