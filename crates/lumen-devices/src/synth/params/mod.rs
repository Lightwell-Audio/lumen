//! Flare's controls (M8 slice B4): the parameter table a project saves, the
//! words a value reads as, and the five presets as parameter sets.
//!
//! Each parameter has a stable `u32` id that is never reused (a new meaning
//! gets a new id), the name a person reads, the plain name a drawn face puts
//! in a tooltip ([`SHORT`], the round-2 labels), its range and taper, its
//! default and how its value reads and is read back.
//!
//! The five presets are **parameter sets**: [`preset_values_into`] gives the
//! values a project holds for one, in the table's own normalised units, so a
//! preset is exactly what a person could have dialled by hand, and changing
//! any control afterwards is an ordinary edit to the device's values.

mod presets;
mod table;
mod words;

pub use presets::{preset_values, preset_values_into};
pub use table::*;

#[cfg(test)]
mod tests;
