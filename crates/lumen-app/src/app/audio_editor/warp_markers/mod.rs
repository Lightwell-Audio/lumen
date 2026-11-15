mod draw;
mod gestures;
mod keys;
mod markers;
mod snap;

pub(crate) use keys::{focused, take_keys};
pub(crate) use markers::span_words;

use eframe::egui;

pub(super) const AT_LIMIT: &str = "limit: a span plays between 25% and 400% of the file's speed";
pub(super) const OUTSIDE_FILE: &str = "the clip would start outside its file";

pub(super) fn stop_id(index: usize) -> egui::Id {
    egui::Id::new(("audio editor warp marker", index))
}
