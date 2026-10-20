//! What the engine asks of an instrument.

use std::time::Instant;

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

    /// As [`Instrument::render`], for a callback that has one deadline for
    /// every host (`deadline`, when it has): a hosted instrument that is
    /// waited for here waits no longer than that, rather than for a share of
    /// the block of its own. The default ignores it.
    fn render_by(
        &mut self,
        rate: u32,
        events: &[Event],
        left: &mut [f32],
        right: &mut [f32],
        _deadline: Option<Instant>,
    ) {
        self.render(rate, events, left, right);
    }

    /// Whether anything still sounds (a held note or a release tail). The
    /// engine keeps rendering a stopped project while this is true.
    fn active(&self) -> bool;

    /// Adds the next `left.len()` frames of what sounds now, as if no event
    /// came, **without** moving on: the next `render` starts where the last
    /// one ended. The engine fades this out across a jump while the new
    /// position fades in, so what was sounding does not stop with a click.
    /// `rate` is the device's; an instrument that runs at another rate adds
    /// nothing. The default adds nothing.
    fn preview(&mut self, _rate: u32, _left: &mut [f32], _right: &mut [f32]) {}

    /// Chooses another sound (a preset). Notes already sounding glide to
    /// it without starting again.
    fn configure(&mut self, _preset: u32) {}

    /// Stops every voice at once, without a release tail.
    fn silence(&mut self);

    /// Whether this instrument runs in a plugin host (M6 slice 2b). Only a
    /// hosted instrument overrides [`Instrument::begin`] and
    /// [`Instrument::finish`].
    fn hosted(&self) -> bool {
        false
    }

    /// The parameters that change in the next block (automation lanes of a
    /// plugin instrument), at their sample, to ride with it. Called before
    /// [`Instrument::begin`] or `render` for the block; a block whose host is
    /// left alone drops them. Only a hosted instrument takes them: the
    /// default does nothing.
    fn automate(&mut self, _params: &crate::ParamBlock) {}

    /// Starts the next block early, before the engine's own work for it:
    /// a hosted instrument hands its host the block and the host works while
    /// the engine does. Returns whether the block was started, in which case
    /// [`Instrument::finish`] must follow instead of `render`. The default
    /// starts nothing, and `render` does everything.
    fn begin(&mut self, _rate: u32, _frames: usize, _events: &[Event]) -> bool {
        false
    }

    /// The sound of auxiliary output pair `pair` (1 and up: the instrument's
    /// other outputs that are on) for the block last finished, into `left` and
    /// `right`. False when there is no such output or the block was not the
    /// host's. The default has none.
    fn output(&mut self, _pair: usize, _left: &mut [f32], _right: &mut [f32]) -> bool {
        false
    }

    /// Completes a block [`Instrument::begin`] started: overwrites `left` and
    /// `right` with its output, waiting for the host until `deadline` at
    /// most. A host that does not answer leaves them untouched (silence).
    fn finish(&mut self, _deadline: Instant, _left: &mut [f32], _right: &mut [f32]) {}
}
