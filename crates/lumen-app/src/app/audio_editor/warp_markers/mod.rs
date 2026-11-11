//! Warp markers in the audio editor (M9.5 S5b, the owner's G B and P A): the
//! warp strip's pointer, its keys, its Tab stops and its drawing.
//!
//! - **Hover** the strip: a ghost marker at the nearest transient, or at the
//!   pointer. Pressing the ghost makes a marker and moves it in one gesture;
//!   double-click adds one without moving it. A marker is dragged by its flag;
//!   Shift and drag slides the audio under it without moving the marker;
//!   double-click on a marker deletes it; Alt bypasses the grid.
//! - **Keyboard:** each marker is a Tab stop, in time order. Left and Right
//!   move it by the grid (Alt finely), Delete takes it away. Add warp marker
//!   puts one at the edit cursor (the playhead when there is none).
//! - A drag shows the speed of the two spans beside the marker; at the limit
//!   (25% and 400%) the marker stops and the chip turns to the warning colour.
//!
//! One drag is one undo step: each frame redoes the edit from the project the
//! drag began on (`Document::edit_warp_marker`). The edits themselves are
//! `lumen_project::edit::warp_markers`.
//!
//! The parts are `gestures` (the pointer), `keys` (the keyboard and the Tab
//! stops), `snap` (the ghost and the grid), `draw` (the strip's painting) and
//! `markers` (the marker edits, their commands and the span chip's words).

mod draw;
mod gestures;
mod keys;
mod markers;
mod snap;

pub(crate) use keys::{focused, take_keys};
pub(crate) use markers::span_words;

use eframe::egui;

/// Said when a marker cannot go further.
pub(super) const AT_LIMIT: &str = "limit: a span plays between 25% and 400% of the file's speed";
pub(super) const OUTSIDE_FILE: &str = "the clip would start outside its file";

/// The stable id of marker `index`'s Tab stop.
pub(super) fn stop_id(index: usize) -> egui::Id {
    egui::Id::new(("audio editor warp marker", index))
}
