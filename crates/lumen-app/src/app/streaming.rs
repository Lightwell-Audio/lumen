//! Playing audio from disk in the app (M9 slice R6, ADR-0109): turns the
//! library's streaming on and off with "Play audio from disk", keeps the disk
//! thread's scene in step with the clips the engine plays, the tempo map, the
//! loop and the markers, and lends the transport what its start waits for
//! (the owner's L A) and the disk thread's reading (D A).

use std::sync::Arc;

use lumen_engine::{Region, Timeline};

use crate::App;
use crate::disk::plan::Scene;
use crate::media::streaming::Streaming;
use crate::transport::DiskReading;
use crate::transport::hold::Gate;

/// What a scene is built from: the count of clip lists sent, the tempo map,
/// the loop, the markers and the rate.
#[derive(PartialEq)]
pub(crate) struct SceneKey {
    sent: u64,
    timeline: Timeline,
    looped: Region,
    markers: Vec<lumen_core::time::Ticks>,
    rate: u32,
}

impl App {
    /// Per-frame: follows the switch, sends the disk thread a new scene when
    /// what it is built from changed, and reads the thread's stats.
    pub(crate) fn update_streaming(&mut self) {
        let wanted = self.settings.playback.from_disk;
        if wanted != self.lanes.library.streaming().is_some() {
            let streaming = wanted.then(|| {
                Streaming::start(
                    self.cache_dir(),
                    self.settings.cache.limit(),
                    self.transport.audio.engine.transport_reader(),
                    self.slow_disk,
                )
            });
            crate::trace::log(|| format!("streaming {}", if wanted { "on" } else { "off" }));
            self.lanes.library.set_streaming(streaming);
            self.disk_scene = None;
            self.transport.gate = None;
        }
        let Some(streaming) = self.lanes.library.streaming() else {
            self.transport.disk = None;
            return;
        };
        let stats = streaming.disk().stats();
        self.transport.disk = Some(DiskReading {
            load: stats.load(),
            underruns: streaming.pool().underruns(),
            failed: stats.failed(),
        });
        let (sent, clips) = self.lanes.streamed();
        let rate = self.transport.state().sample_rate;
        let key = SceneKey {
            sent,
            timeline: self.transport.timeline.clone(),
            looped: self.transport.loop_region,
            markers: self
                .document
                .project()
                .markers
                .iter()
                .map(|m| m.at)
                .collect(),
            rate,
        };
        if self.disk_scene.as_ref() == Some(&key) {
            return;
        }
        let timeline = &key.timeline;
        let looped = key.looped;
        let looped = (looped.enabled && looped.end > looped.start).then(|| {
            (
                timeline.sample_at(looped.start, rate),
                timeline.sample_at(looped.end, rate),
            )
        });
        // Where playback can jump to: the song's start, the loop's start and
        // the markers.
        let mut points = vec![0];
        points.extend(looped.map(|(start, _)| start));
        points.extend(key.markers.iter().map(|at| timeline.sample_at(*at, rate)));
        points.sort_unstable();
        points.dedup();
        let scene = Arc::new(Scene::new(clips, timeline, rate, looped, points));
        streaming.disk().set_scene(Arc::clone(&scene));
        self.transport.gate = Some(Gate {
            scene,
            cuer: streaming.disk().cuer(),
        });
        self.disk_scene = Some(key);
    }
}
