//! Lumen's audio device layer (ADR-0006).
//!
//! - [`types`]: devices, configurations, the real-time [`AudioCallback`].
//! - [`backend`]: the interface each driver model implements. Backends:
//!   `asio` and `wasapi` on Windows, `cpal_host` (CoreAudio, ALSA) elsewhere
//!   (ADR-0006, ADR-0016).
//! - [`supervisor`]: [`AudioSystem`], which keeps audio running through
//!   driver resets, configuration changes and device loss.
//! - [`fake`]: a scriptable backend for tests.
//! - [`priority`]: scheduling for threads that keep time.

#[cfg(windows)]
pub mod asio;
pub mod backend;
mod convert;
#[cfg(not(windows))]
pub mod cpal_host;
#[cfg(windows)]
mod drift;
pub mod fake;
pub mod priority;
pub mod supervisor;
pub mod types;
#[cfg(windows)]
pub mod wasapi;
#[cfg(windows)]
mod wasapi_input;

pub use backend::{Backend, OpenError, Stream, StreamEvent, Unavailable};
pub use supervisor::{
    AudioSystem, BlockTime, DeviceEvent, HANDOVER_TIMEOUT, State, Status, SwitchError, Timing,
};
pub use types::{
    ALL_INPUTS, AudioCallback, AudioError, Block, BufferSizes, DeviceId, DeviceInfo, DriverKind,
    MOST_INPUTS, StreamConfig, StreamInfo,
};
