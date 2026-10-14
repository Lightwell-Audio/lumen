//! The interface a driver model implements. Backends run on the MIDI worker
//! thread and may block.

use crate::types::{MidiError, PortId, PortInfo};

/// Called on the driver's thread with a timestamp (microseconds) and the
/// bytes of each message.
pub type InputSink = Box<dyn FnMut(u64, &[u8]) + Send>;

/// An open input; dropping it closes the port.
pub trait Connection {}

/// An open output.
pub trait Output {
    fn send(&mut self, bytes: &[u8]) -> Result<(), MidiError>;
}

pub trait Backend {
    /// Every input and output now present.
    fn ports(&mut self) -> Vec<PortInfo>;
    fn open_input(
        &mut self,
        id: &PortId,
        sink: InputSink,
    ) -> Result<Box<dyn Connection>, MidiError>;
    fn open_output(&mut self, id: &PortId) -> Result<Box<dyn Output>, MidiError>;
}
