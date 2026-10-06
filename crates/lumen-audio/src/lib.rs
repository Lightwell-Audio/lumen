//! Lumen's audio device layer (ADR-0006).
//!
//! - [`types`]: devices, configurations, the real-time [`AudioCallback`].
//! - [`backend`]: the interface each driver model implements.
//! - [`supervisor`]: [`AudioSystem`], which keeps audio running through
//!   driver resets, configuration changes and device loss.
//! - [`fake`]: a scriptable backend for tests.

pub mod backend;
pub mod fake;
pub mod supervisor;
pub mod types;

pub use backend::{Backend, OpenError, Stream, StreamEvent};
pub use supervisor::{AudioSystem, DeviceEvent, State, Status, Timing};
pub use types::{
    AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind, StreamConfig,
    StreamInfo,
};
