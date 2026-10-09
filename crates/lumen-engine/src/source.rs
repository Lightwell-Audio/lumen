//! What the engine plays: a [`Source`] renders timeline audio for a range of
//! samples. Slice 6 implements it for audio clips; tests use probes.

use crate::timeline::Timeline;

/// Where a source writes: part of a channel-major block.
pub struct Output<'a> {
    data: &'a mut [f32],
    /// Frames per channel in the whole block.
    stride: usize,
    /// First frame of this part within the block.
    offset: usize,
    frames: usize,
    channels: usize,
}

impl<'a> Output<'a> {
    pub(crate) fn new(
        data: &'a mut [f32],
        stride: usize,
        offset: usize,
        frames: usize,
        channels: usize,
    ) -> Self {
        Self {
            data,
            stride,
            offset,
            frames,
            channels,
        }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// `frames` frames of this part from its frame `from`.
    pub(crate) fn part(&mut self, from: usize, frames: usize) -> Output<'_> {
        debug_assert!(from + frames <= self.frames);
        Output::new(
            self.data,
            self.stride,
            self.offset + from,
            frames,
            self.channels,
        )
    }

    /// The frames of `channel` this part covers. Already zeroed; sources add
    /// to them.
    pub fn channel(&mut self, channel: usize) -> &mut [f32] {
        let start = channel * self.stride + self.offset;
        &mut self.data[start..start + self.frames]
    }
}

/// What a source needs to know about the part it renders.
#[derive(Debug, Clone, Copy)]
pub struct RenderContext<'a> {
    pub timeline: &'a Timeline,
    pub sample_rate: u32,
    /// Timeline sample of the first frame. The part never crosses a loop
    /// wrap or a locate: those split the block.
    pub start: i64,
    /// This is the 2 ms rendered ahead of a jump, to crossfade from (the
    /// engine fades it out). Meters and recorders should ignore it.
    pub tail: bool,
}

/// Why playback jumped, so sources can choose whether to fade in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    /// Playback started.
    Play,
    /// The user moved the playhead while playing.
    Locate,
    /// Playback wrapped from the loop end to the loop start. Loops must
    /// stay seamless, so sources should not fade here.
    LoopWrap,
    /// The tempo map or the sample rate changed, so the same musical
    /// position now falls on a different sample.
    Reposition,
}

/// Timeline audio, rendered on the audio thread. Every method must be
/// real-time safe: no allocation, locks or blocking (ADR-0010).
pub trait Source: Send {
    /// Adds `out.frames()` frames of audio starting at timeline sample
    /// `cx.start` to `out`. Called while playing, and while scrubbing at
    /// any position (M3 slice 5e).
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>);

    /// Playback starts or jumps to `sample`, for the reason given. Sources
    /// reposition here. The engine already crossfades every jump.
    fn locate(&mut self, _timeline: &Timeline, _sample_rate: u32, _sample: i64, _jump: Jump) {}

    /// A block of `frames` passed without rendering (stopped, not
    /// scrubbing): what measures levels lets them fall (M4 slice 5).
    fn idle(&mut self, _sample_rate: u32, _frames: usize) {}
}

/// Silence.
impl Source for () {
    fn render(&mut self, _: &RenderContext<'_>, _: &mut Output<'_>) {}
}
