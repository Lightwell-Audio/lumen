//! Lumen's real-time engine (ADR-0003, ADR-0017).
//!
//! - [`engine`]: creates the [`EngineHandle`] (UI side) and [`Engine`]
//!   (audio thread) pair. The engine owns the transport: play, pause, stop,
//!   locate, loop and punch, all sample-accurate.
//! - [`Timeline`]: tempo and time-signature maps, swapped in whole.
//! - [`Source`]: what the engine plays; [`ClipPlayer`] plays audio clips.
//! - [`TransportSnapshot`]: the transport as the UI sees it.
//!
//! The engine runs inside the device callback and never allocates, locks or
//! blocks there; `tests/realtime.rs` checks that, and RealtimeSanitizer
//! checks it again in Linux CI.

mod clips;
mod declick;
mod engine;
mod snapshot;
mod source;
mod timeline;

pub use clips::{Clip, ClipAudio, ClipPlayer, ClipsHandle, clip_player};
pub use engine::{Command, Engine, EngineHandle, Region, engine};
pub use snapshot::TransportSnapshot;
pub use source::{Jump, Output, RenderContext, Source};
pub use timeline::Timeline;
