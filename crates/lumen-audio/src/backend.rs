use crate::types::{AudioCallback, AudioError, DeviceInfo, StreamConfig, StreamInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent {
    ResetRequested,
    Stalled,
    Dropped(u64),
}

pub struct OpenError {
    pub error: AudioError,
    pub callback: Box<dyn AudioCallback>,
}

pub(crate) fn silent() -> Box<dyn AudioCallback> {
    Box::new(|_: &mut crate::types::Block<'_>| {})
}

impl std::fmt::Debug for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

pub trait Backend {
    fn devices(&mut self) -> Vec<DeviceInfo>;

    fn open(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
    ) -> Result<Box<dyn Stream>, OpenError>;

    fn pump(&mut self) {}
}

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

    fn poll(&mut self) -> Option<StreamEvent>;

    fn control_panel(&mut self) -> Result<(), String> {
        Err("this driver has no control panel".into())
    }

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
