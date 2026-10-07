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

/// Lets the driver model be chosen at run time.
impl<B: Backend + ?Sized> Backend for Box<B> {
    fn devices(&mut self) -> Vec<DeviceInfo> {
        (**self).devices()
    }

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        (**self).open(config, callback)
    }

    fn pump(&mut self) {
        (**self).pump();
    }
}

/// A driver model that could not start (for example, Windows could not set
/// up COM for ASIO). It lists no devices and refuses every open with the
/// reason, so the app shows "audio could not start" with a way forward
/// instead of the audio thread failing.
#[derive(Debug, Clone)]
pub struct Unavailable {
    reason: String,
}

impl Unavailable {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Backend for Unavailable {
    fn devices(&mut self) -> Vec<DeviceInfo> {
        Vec::new()
    }

    fn open(
        &mut self,
        _config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError> {
        Err(OpenError {
            error: AudioError::Driver(self.reason.clone()),
            callback,
        })
    }
}

pub trait Stream {
    fn info(&self) -> StreamInfo;

    /// Non-blocking check for driver events.
    fn poll(&mut self) -> Option<StreamEvent>;

    /// Opens the driver's own settings window (ASIO). Changes made there
    /// arrive as a reset request.
    fn control_panel(&mut self) -> Result<(), String> {
        Err("this driver has no control panel".into())
    }

    /// Stops the stream, releases the device and returns the callback.
    fn close(self: Box<Self>) -> Box<dyn AudioCallback>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Block;

    #[test]
    fn an_unavailable_driver_refuses_with_its_reason_and_returns_the_callback() {
        let mut backend = Unavailable::new("ASIO could not start: no COM");
        assert!(backend.devices().is_empty());
        let config = StreamConfig {
            device: crate::types::DeviceId {
                kind: crate::types::DriverKind::Asio,
                id: "x".into(),
            },
            sample_rate: None,
            buffer_frames: None,
            input_channels: vec![],
            output_channels: vec![0, 1],
        };
        let Err(failed) = backend.open(&config, Box::new(|_: &mut Block<'_>| {})) else {
            panic!("an unavailable driver must not open");
        };
        assert!(failed.error.to_string().contains("no COM"));
    }
}
