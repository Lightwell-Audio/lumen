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

mod capture;
mod clips;
mod declick;
mod engine;
mod envelope;
mod input;
mod metronome;
mod midi;
mod mixer;
mod playhead;
mod render;
mod scrub;
mod snapshot;
mod source;
pub mod testing;
mod timeline;

pub use capture::{
    CaptureReader, CaptureSink, Chunk as CaptureChunk, RING_CHUNKS, RING_SAMPLES, capture,
};
pub use clips::{Clip, ClipAudio, ClipPlayer, ClipShape, ClipsHandle, FadeFrames, clip_player};
pub use engine::{Command, Engine, EngineHandle, HOST_SHARE, OFFLINE_WAIT, Region, engine};
pub use envelope::ClipEnvelope;
pub use input::{
    CLIP_LEVEL as INPUT_CLIP_LEVEL, InputHandle, InputLevel, InputPlayer, InputRoute, MAX_INPUTS,
    MAX_ROUTES as MAX_INPUT_ROUTES, Monitoring, input_player,
};
pub use metronome::{ClickSound, MetronomeSettings};
pub use midi::{
    AUDITION, CLOCK_TICKS, Captured, Heard, INPUT_SLOTS, InstrumentSpec, KEYBOARD, Listen, LiveIn,
    LiveInput, MidiClip, MidiControl, MidiHandle, MidiNote, MidiOut, MidiPlayer, MidiTrack,
    RecordTiming, SOURCES as LIVE_SOURCES, Target, WRAP, control_list, midi_player, note_list,
    record_position,
};
pub use mixer::{
    AutoLane, AutoPoint, Bus, Buses, Compensation, CueOut, Destination, Device, KNOT, Layered,
    Level, MAX_DEVICE_LATENCY, MAX_HOSTS, Mixer, MixerHandle, NodeAutomation, NodeSpec, ParamLane,
    Plan, RMS_SECONDS, Routing, RoutingError, SendSpec, SidechainSpec, Tap, TrackSource,
    compile as compile_routing, db_gain, mixer,
};
pub use render::{
    ClipRender, Ended, Finished, MOST_BLOCK, SourceRender, render_clips, render_source,
};
pub use scrub::{MAX_SPEED as MAX_SCRUB_SPEED, ScrubStyle};
pub use snapshot::TransportSnapshot;
pub use source::{Inputs, Jump, Mode, Output, RenderContext, Source};
pub use timeline::Timeline;
