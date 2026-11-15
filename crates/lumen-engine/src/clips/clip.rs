use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::envelope::ClipEnvelope;
use crate::stream::StreamAudio;

use super::fades::ClipShape;

#[derive(Debug, Clone, PartialEq)]
pub enum ClipAudio {
    Memory {
        sample_rate: u32,
        channels: Vec<Vec<f32>>,
    },
    Stream(StreamAudio),
}

impl ClipAudio {
    pub fn sample_rate(&self) -> u32 {
        match self {
            ClipAudio::Memory { sample_rate, .. } => *sample_rate,
            ClipAudio::Stream(stream) => stream.sample_rate(),
        }
    }

    pub fn frames(&self) -> usize {
        match self {
            ClipAudio::Memory { channels, .. } => channels.first().map_or(0, Vec::len),
            ClipAudio::Stream(stream) => stream.frames() as usize,
        }
    }

    pub fn channels(&self) -> usize {
        match self {
            ClipAudio::Memory { channels, .. } => channels.len(),
            ClipAudio::Stream(stream) => stream.channels(),
        }
    }

    pub fn memory(&self) -> Option<&[Vec<f32>]> {
        match self {
            ClipAudio::Memory { channels, .. } => Some(channels),
            ClipAudio::Stream(_) => None,
        }
    }

    pub fn stream(&self) -> Option<&StreamAudio> {
        match self {
            ClipAudio::Memory { .. } => None,
            ClipAudio::Stream(stream) => Some(stream),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub id: u64,
    pub track: u64,
    pub start: Ticks,
    pub offset: i64,
    pub length: i64,
    pub gain: f32,
    pub audio: Arc<ClipAudio>,
    pub shape: ClipShape,
    pub envelope: ClipEnvelope,
}
