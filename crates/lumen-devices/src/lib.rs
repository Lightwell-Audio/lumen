//! Lumen's built-in devices (ADR-0045). Pure, real-time-safe DSP that
//! depends on `lumen-core` alone: the engine runs it, the app builds it.
//! Instruments first (M5 slice 1); the effects of M8 join them (ADR-XXXX).
//!
//! Everything here follows the audio thread's rules: no allocation, locks
//! or blocking while rendering. Devices allocate in their constructors,
//! which run on the UI side.

pub mod effect;
pub mod hosted;
pub mod instrument;
pub mod params;
pub mod synth;
pub mod utility;

pub use effect::{Block, Effect, Key, Stereo};
pub use hosted::{
    Cost, FAILED_AFTER_FRAMES, Guard, Hosted, HostedInstrument, MOST_FRAMES, MOST_POINTS,
    MOST_QUEUES, Mark, Outcome, ParamBlock, Refused, Reply, Status,
};
pub use instrument::Instrument;
pub use params::ParamInfo;
pub use synth::{Preset, Synth};
pub use utility::Utility;

/// The parameters of the built-in effect called `name`, or `None` when no
/// device has that name: the one table the card, the lanes and the engine's
/// values all read.
pub fn effect_params(name: &str) -> Option<&'static [ParamInfo]> {
    match name {
        utility::NAME => Some(utility::TABLE),
        _ => None,
    }
}

/// A built-in effect by the name a project saves. The constructor allocates:
/// call it on the interface side, then hand the device to the audio thread.
pub fn effect(name: &str) -> Option<Box<dyn Effect>> {
    match name {
        utility::NAME => Some(Box::new(Utility::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_effect_is_in_both_tables() {
        let name = utility::NAME;
        assert!(effect_params(name).is_some(), "{name} has a table");
        assert!(effect(name).is_some(), "{name} is built");
        assert!(effect_params("nothing").is_none());
        assert!(effect("nothing").is_none());
    }
}
