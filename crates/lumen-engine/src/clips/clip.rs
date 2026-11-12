//! The clip the project hands the player: where it plays, how loud, and the
//! audio it reads.

use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::envelope::ClipEnvelope;
use crate::stream::StreamAudio;

use super::fades::ClipShape;

/// Audio for clips.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipAudio {
    /// Held in memory, channel after channel, at `sample_rate`.
    Memory {
        sample_rate: u32,
        channels: Vec<Vec<f32>>,
    },
    /// Read from disk as it plays (M9 slice R6): only the pages a disk
    /// thread has put in memory can be heard.
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

    /// Channels played: all of a file in memory, one or two streamed.
    pub fn channels(&self) -> usize {
        match self {
            ClipAudio::Memory { channels, .. } => channels.len(),
            ClipAudio::Stream(stream) => stream.channels(),
        }
    }

    /// The samples in memory, channel after channel; `None` when streamed.
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
    /// Identity across clip lists (the project's clip id), so a clip's
    /// level glides from list to list.
    pub id: u64,
    /// The track it plays on: the id of its node in the routing.
    pub track: u64,
    /// Timeline position of the clip's first sample.
    pub start: Ticks,
    /// First frame of the audio played at the clip start.
    pub offset: i64,
    /// Frames played, repeats included.
    pub length: i64,
    /// Linear gain; 0 for a muted clip, which stays in the list so it
    /// fades out and back in.
    pub gain: f32,
    /// Plays only while the device runs at `audio.sample_rate`; otherwise
    /// the clip is silent until the app converts it again.
    pub audio: Arc<ClipAudio>,
    /// Fades and repeats.
    pub shape: ClipShape,
    /// Gain and pan envelopes, on the clip's own clock (M7 slice A7).
    pub envelope: ClipEnvelope,
}
