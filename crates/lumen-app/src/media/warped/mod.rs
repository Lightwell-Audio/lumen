//! Warped clips as streams (M9.5 S4b, ADR-0131).
//!
//! A warped clip plays through the same page pool and disk thread as a file
//! read from disk (ADR-0109), as a stream whose frames are computed instead of
//! read: **frame `k` of it is the song at timeline sample `k`**, the file read
//! at the position the clip's warp map gives there
//! ([`lumen_media::repitch`]). The engine plays it as any streamed clip (an
//! offset of the clip's start sample, the clip's own fades), the disk thread
//! prepares its pages ahead of the playhead, and an offline render loads them
//! on its own thread, so what is heard and what is exported are the same bits.
//!
//! A stream does not know which clip plays it, only the line from timeline to
//! file: the pieces a split leaves share one stream, so the engine hears the
//! seam between them as no seam. A looping clip is not a stream of its own:
//! it is played as the real clips it unloops into (`lanes::engine_clips_with`),
//! each following the tempo and edge-faded as any clip is.
//!
//! Streams are keyed by what they are made from ([`provider`]): an edit makes
//! a new stream, and the engine keeps playing the old one until the new
//! one's pages are there.

mod provider;
mod source;

pub use provider::{Request, WarpedStreams};
pub(super) use source::Rendering;

use std::sync::Arc;

use lumen_core::time::{TempoMap, Ticks};
use lumen_core::warp::{self, Marker};
use lumen_engine::ClipAudio;
use lumen_media::stretch::Preset;
use lumen_project::model::WarpMode;

/// Where a warped stream reads the file it plays.
#[derive(Clone)]
pub enum Origin {
    /// Audio held in memory.
    Memory(Arc<ClipAudio>),
    /// A stream file, by the id [`super::streams::register`] gave it.
    File(u64),
}

/// How a warped stream turns the file into the song's frames (the warp
/// mode, as far as it is heard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Engine {
    /// The file read faster or slower, its pitch following (S4b).
    Repitch,
    /// The file stretched without changing its pitch (S4c).
    Stretch(Preset),
}

impl Engine {
    /// The engine a mode plays with. Rhythmic is Lumen's own slicer, which
    /// comes with S5b: until then it plays as Repitch, as it did before.
    pub fn of(mode: WarpMode) -> Self {
        match mode {
            WarpMode::Repitch | WarpMode::Rhythmic => Engine::Repitch,
            WarpMode::Melodic => Engine::Stretch(Preset::Light),
            WarpMode::FullMix => Engine::Stretch(Preset::Full),
        }
    }
}

/// What a warped stream is made from: the file, the map and the tempo it
/// plays against. Everything its frames depend on.
pub struct Warped {
    pub origin: Origin,
    pub engine: Engine,
    /// Channels played, one or two.
    pub channels: usize,
    /// The rate of file and stream.
    pub rate: u32,
    /// The map, measured from the anchor.
    pub markers: Vec<Marker>,
    pub original_bpm: f64,
    pub tempo: TempoMap,
    /// The anchor is the place in the song of the first marker, which does not
    /// move when a clip is split (what lets the pieces share a stream). This is
    /// the tick of the sample nearest it (between ticks): the grid the stream
    /// is aligned to, as a plain clip is aligned to its start sample, so a clip
    /// at its file's tempo is the file's own frames. The map is measured from it.
    anchor_ticks: f64,
    /// Frames in the stream: the song up to the last sample it must reach.
    pub frames: u64,
}

impl Warped {
    pub fn new(
        origin: Origin,
        channels: usize,
        rate: u32,
        (markers, original_bpm): (Vec<Marker>, f64),
        tempo: TempoMap,
        (anchor, frames): (Ticks, u64),
    ) -> Self {
        let anchor_sample = tempo.sample_at(anchor, rate);
        let anchor_ticks = tempo.ticks_f64_at_seconds(anchor_sample as f64 / f64::from(rate));
        Self {
            origin,
            engine: Engine::Repitch,
            channels,
            rate,
            markers,
            original_bpm,
            tempo,
            anchor_ticks,
            frames,
        }
    }

    /// The frame of the file, between frames, that plays at stream frame `k`.
    /// Depends on nothing but `k` ([`lumen_media::repitch`]).
    pub fn position(&self, k: i64) -> f64 {
        let seconds = k as f64 / f64::from(self.rate);
        let ticks = self.tempo.ticks_f64_at_seconds(seconds) - self.anchor_ticks;
        warp::source_at_f64(&self.markers, self.original_bpm, ticks) * f64::from(self.rate)
    }
}

#[cfg(test)]
mod tests;
