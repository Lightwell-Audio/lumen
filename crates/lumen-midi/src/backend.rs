use crate::types::{MidiError, PortId, PortInfo};

pub type InputSink = Box<dyn FnMut(u64, &[u8]) + Send>;

pub trait Connection {}

pub trait Output {
    fn send(&mut self, bytes: &[u8]) -> Result<(), MidiError>;
}

pub trait Backend {
    fn ports(&mut self) -> Vec<PortInfo>;
    fn open_input(
        &mut self,
        id: &PortId,
        sink: InputSink,
    ) -> Result<Box<dyn Connection>, MidiError>;
    fn open_output(&mut self, id: &PortId) -> Result<Box<dyn Output>, MidiError>;
}
