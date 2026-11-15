#[cfg(windows)]
pub mod asio;
pub mod backend;
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "the portable backend is the only user, and Windows builds other backends"
    )
)]
mod callback_slot;
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

#[cfg(test)]
lumen_rt::install_guarded_allocator!();
