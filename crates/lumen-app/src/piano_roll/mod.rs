//! The piano roll's drawing (M5 slice 3b, the owner's look A, "studio
//! keys"): notes as boxes on the GPU (`notes`, ADR-0026's box layer), the
//! lanes, grid and key column (`draw`), and the notes inside arrangement
//! clips (`mini`). Rendering only: input, the Clip tab and the pop-out
//! window are slice 3c.

pub(crate) mod demo;
pub(crate) mod draw;
pub(crate) mod mini;
pub(crate) mod notes;

/// MIDI pitches 0 to 127.
pub(crate) const PITCHES: u32 = 128;
