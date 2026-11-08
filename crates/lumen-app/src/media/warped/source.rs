//! Reading a warped stream: the file it plays, opened, and the renderer that
//! turns it into the stream's frames. For workers, never the UI or audio
//! thread.

use std::sync::Arc;

use lumen_engine::ClipAudio;
use lumen_media::MediaError;
use lumen_media::repitch::{Renderer, Source};
use lumen_media::stream::Reader;

use super::Warped;

/// A warped stream opened for reading.
pub(in crate::media) struct Rendering {
    warped: Arc<Warped>,
    renderer: Renderer,
    input: Input,
}

enum Input {
    Memory(Arc<ClipAudio>),
    File {
        reader: Box<Reader>,
        channels: usize,
    },
}

impl Rendering {
    /// `warped` over `file`, the reader of its stream file when it plays one.
    pub(in crate::media) fn new(warped: Arc<Warped>, file: Option<Reader>) -> Option<Self> {
        let input = match (&warped.origin, file) {
            (super::Origin::Memory(audio), _) => {
                audio.memory()?;
                Input::Memory(Arc::clone(audio))
            }
            (super::Origin::File(_), Some(reader)) => {
                let channels = warped.channels.min(usize::from(reader.layout().channels));
                Input::File {
                    reader: Box::new(reader),
                    channels,
                }
            }
            (super::Origin::File(_), None) => return None,
        };
        Some(Self {
            warped,
            renderer: Renderer::new(),
            input,
        })
    }

    /// Reads the stream's frames from `from` into `left` and, for two
    /// channels, `right`. Returns the frames the stream has there; the rest
    /// are left as silence by the caller.
    pub(in crate::media) fn read(
        &mut self,
        from: u64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let warped = Arc::clone(&self.warped);
        let n = left.len();
        let right = if warped.channels >= 2 {
            &mut right[..n]
        } else {
            &mut right[..0]
        };
        self.renderer.render(
            &mut self.input,
            &|k| warped.position(k),
            from as i64,
            left,
            right,
        )?;
        Ok(warped.frames.saturating_sub(from).min(n as u64) as usize)
    }
}

impl Source for Input {
    fn channels(&self) -> usize {
        match self {
            Input::Memory(audio) => audio.channels().min(2),
            Input::File { channels, .. } => *channels,
        }
    }

    fn frames(&self) -> u64 {
        match self {
            Input::Memory(audio) => audio.frames() as u64,
            Input::File { reader, .. } => reader.layout().frames,
        }
    }

    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError> {
        match self {
            Input::Memory(audio) => {
                let channels = audio.memory().unwrap_or_default();
                for (out, data) in out.iter_mut().zip(channels) {
                    let from = (from as usize).min(data.len());
                    let n = out.len().min(data.len() - from);
                    out[..n].copy_from_slice(&data[from..from + n]);
                    out[n..].fill(0.0);
                }
                Ok(())
            }
            Input::File { reader, .. } => reader.read(from, out).map(|_| ()),
        }
    }
}
