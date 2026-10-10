//! What the engine asks of an instrument.

use lumen_core::midi::Event;

/// A device that turns MIDI into stereo audio. Every method except the
/// constructor is real-time safe.
pub trait Instrument: Send {
    /// Renders `left.len()` frames (the same for `right`) of the sound
    /// after the `events` (sorted by offset, offsets within the block),
    /// adding to the slices, at `rate` Hz. Events apply at their exact
    /// frame; ones at or past the end apply after the last frame, so a
    /// call with empty slices only delivers messages (and then `rate` may
    /// be 0, meaning unchanged).
    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]);

    /// Whether anything still sounds (a held note or a release tail). The
    /// engine keeps rendering a stopped project while this is true.
    fn active(&self) -> bool;

    /// Chooses another sound (a preset). Notes already sounding finish
    /// with the sound they began with.
    fn configure(&mut self, _preset: u32) {}

    /// Stops every voice at once, without a release tail.
    fn silence(&mut self);
}
