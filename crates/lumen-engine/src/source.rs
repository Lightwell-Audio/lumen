use std::time::Instant;

use crate::timeline::Timeline;

pub struct Output<'a> {
    data: &'a mut [f32],
    stride: usize,
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

    pub fn planar(data: &'a mut [f32], frames: usize, channels: usize) -> Option<Self> {
        (frames.checked_mul(channels) == Some(data.len()))
            .then(|| Self::new(data, frames, 0, frames, channels))
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

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

    pub fn channel(&mut self, channel: usize) -> &mut [f32] {
        let start = channel * self.stride + self.offset;
        &mut self.data[start..start + self.frames]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub device: u64,
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: usize,
    pub data: &'a [f32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Play,
    Scrub,
    Ring,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderContext<'a> {
    pub timeline: &'a Timeline,
    pub sample_rate: u32,
    pub start: i64,
    pub tail: bool,
    pub stopping: bool,
    pub mode: Mode,
    pub stream: u64,
    pub device: u64,
    pub recording: bool,
    pub looping: Option<(i64, i64)>,
    pub deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    Play,
    Locate,
    LoopWrap,
    Reposition,
}

pub trait Source: Send {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>);

    fn input(&mut self, _inputs: &Inputs<'_>) {}

    fn locate(&mut self, _timeline: &Timeline, _sample_rate: u32, _sample: i64, _jump: Jump) {}

    fn idle(&mut self, _sample_rate: u32, _frames: usize) {}

    fn latency(&self) -> u32 {
        0
    }

    fn stopped(&mut self) {}

    fn panic(&mut self) {}

    fn ringing(&self) -> bool {
        false
    }

    fn hosted(&self) -> bool {
        false
    }
}

impl Source for () {
    fn render(&mut self, _: &RenderContext<'_>, _: &mut Output<'_>) {}
}
