//! What the engine plays, derived from the project: the clip list
//! (`lanes`), the routing (`routing`), automation on the device's sample
//! clock (`lane_bake`) and the MIDI captured while not recording (`played`).
//!
//! The translation is pure and tested on its own; the app decides when to
//! rebuild it (sweep M11: one folder per concern).

pub(crate) mod lane_bake;
pub(crate) mod lanes;
pub(crate) mod played;
pub(crate) mod routing;
