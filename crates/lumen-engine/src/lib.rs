//! Lumen's real-time engine (ADR-0003, ADR-0017).
//!
//! - [`engine`]: creates the [`EngineHandle`] (UI side) and [`Engine`]
//!   (audio thread) pair. The engine owns the transport: play, pause, stop,
//!   locate, loop and punch, all sample-accurate, and scrubbing while
//!   stopped ([`ScrubStyle`]).
//! - [`Timeline`]: tempo and time-signature maps, swapped in whole.
//! - [`Source`]: what the engine plays: the [`Mixer`], which runs the
//!   routing ([`Routing`]: tracks, groups, returns, the master) over what a
//!   [`TrackSource`] plays into each track; [`ClipPlayer`] plays audio
//!   clips.
//! - [`TransportSnapshot`]: the transport as the UI sees it.
//!
//! The engine runs inside the device callback and never allocates, locks or
//! blocks there; `tests/realtime.rs` checks that, and RealtimeSanitizer
//! checks it again in Linux CI.

mod clips;
mod declick;
mod engine;
mod metronome;
mod mixer;
mod scrub;
mod snapshot;
mod source;
pub mod testing;
mod timeline;

pub use clips::{Clip, ClipAudio, ClipPlayer, ClipShape, ClipsHandle, FadeFrames, clip_player};
pub use engine::{Command, Engine, EngineHandle, Region, engine};
pub use metronome::{ClickSound, MetronomeSettings};
pub use mixer::{
    Bus, Buses, CueOut, Destination, Mixer, MixerHandle, NodeSpec, Plan, Routing, RoutingError,
    SendSpec, TrackSource, compile as compile_routing, mixer,
};
pub use scrub::{MAX_SPEED as MAX_SCRUB_SPEED, ScrubStyle};
pub use snapshot::TransportSnapshot;
pub use source::{Jump, Output, RenderContext, Source};
pub use timeline::Timeline;
