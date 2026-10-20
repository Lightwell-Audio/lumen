//! Lumen's built-in devices (ADR-0045). Pure, real-time-safe DSP that
//! depends on `lumen-core` alone: the engine runs it, the app builds it.
//! Instruments first (M5 slice 1); the effects of M8 join them.
//!
//! Everything here follows the audio thread's rules: no allocation, locks
//! or blocking while rendering. Devices allocate in their constructors,
//! which run on the UI side.

pub mod hosted;
pub mod instrument;
pub mod synth;

pub use hosted::{
    Cost, FAILED_AFTER_FRAMES, Guard, Hosted, HostedInstrument, MOST_FRAMES, MOST_POINTS,
    MOST_QUEUES, Mark, Outcome, ParamBlock, Refused, Reply, Status,
};
pub use instrument::Instrument;
pub use synth::{Preset, Synth};
