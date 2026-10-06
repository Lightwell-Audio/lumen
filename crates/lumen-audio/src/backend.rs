//! The interface every driver model implements. Backends run on the audio
//! control thread (which, for ASIO, is the COM apartment the driver lives in).

use crate::types::{AudioCallback, AudioError, DeviceInfo, StreamConfig, StreamInfo};

/// Something the running stream reports to its supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent {
    /// The driver asks to be reset (buffer size or rate changed in its panel,
    /// or the device went away).
    ResetRequested,
    /// No callbacks for longer than the watchdog allows.
    Stalled,
}

/// A failed open hands the callback back so nothing is lost.
pub struct OpenError {
    pub error: AudioError,
    pub callback: Box<dyn AudioCallback>,
}

impl std::fmt::Debug for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

pub trait Backend {
    /// Devices of this driver model (may load drivers; not real-time).
    fn devices(&mut self) -> Vec<DeviceInfo>;

    /// Opens and starts a stream. The callback runs on the driver's thread
    /// until the stream is closed.
    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError>;

    /// Called regularly on the control thread (message pumping for drivers
    /// that need it).
    fn pump(&mut self) {}
}

pub trait Stream {
    fn info(&self) -> StreamInfo;

    /// Non-blocking check for driver events.
    fn poll(&mut self) -> Option<StreamEvent>;

    /// Stops the stream, releases the device and returns the callback.
    fn close(self: Box<Self>) -> Box<dyn AudioCallback>;
}
