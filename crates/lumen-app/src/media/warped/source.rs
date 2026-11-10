//! Reading a warped stream: the file it plays, opened, and the renderer that
//! turns it into the stream's frames. For workers, never the UI or audio
//! thread.

use std::sync::Arc;

use lumen_engine::ClipAudio;
use lumen_media::MediaError;
use lumen_media::repitch::{self, Source};
use lumen_media::slicer::Slicer;
use lumen_media::stream::Reader;
use lumen_media::stretch::{Chunks, Stretcher};

use super::{Engine, Warped};

/// A warped stream opened for reading.
pub(in crate::media) struct Rendering {
    warped: Arc<Warped>,
    /// Never waits for background work (the disk thread's rendering).
    polling: bool,
    renderer: Renderer,
    input: Input,
}

/// What turns the file into the stream's frames.
enum Renderer {
    Repitch(repitch::Renderer),
    Stretch(Box<Chunks>),
    Slices(Slicer),
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
    pub(in crate::media) fn new(
        warped: Arc<Warped>,
        file: Option<Reader>,
        polling: bool,
    ) -> Option<Self> {
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
        let renderer = match warped.engine {
            Engine::Stretch(preset) => Stretcher::new(warped.channels, warped.rate, preset)
                .map(|s| Renderer::Stretch(Box::new(Chunks::new(s))))
                // A stretcher that cannot be made plays the stream as
                // Repitch: something to hear rather than a hole.
                .unwrap_or_else(|_| Renderer::Repitch(repitch::Renderer::new())),
            Engine::Repitch => Renderer::Repitch(repitch::Renderer::new()),
            Engine::Slices => Renderer::Slices(Slicer::new()),
        };
        Some(Self {
            warped,
            polling,
            renderer,
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
        let position = |k| warped.position(k);
        match &mut self.renderer {
            Renderer::Repitch(r) => {
                r.render(&mut self.input, &position, from as i64, left, right)?
            }
            Renderer::Stretch(c) => {
                c.render(&mut self.input, &position, from as i64, left, right)?
            }
            Renderer::Slices(s) => {
                let starts = if self.polling {
                    Warped::slice_starts_polled(&warped)?
                } else {
                    warped.slice_starts(&mut self.input)?
                };
                s.render(
                    &mut self.input,
                    (&position, starts),
                    from as i64,
                    left,
                    right,
                )?
            }
        }
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

/// The frames in the file a stream reads.
pub(super) fn frames_of(origin: &super::Origin) -> Option<u64> {
    match origin {
        super::Origin::Memory(audio) => Some(audio.frames() as u64),
        super::Origin::File(id) => {
            let path = crate::media::streams::path(*id)?;
            Reader::open(&path).ok().map(|r| r.layout().frames)
        }
    }
}

/// The transients of the file a stream reads, read through a reader of its own
/// (for a worker).
pub(super) fn scan_file(
    origin: &super::Origin,
    channels: usize,
    rate: u32,
) -> Result<Vec<f64>, MediaError> {
    let mut input = match origin {
        super::Origin::Memory(audio) => {
            audio.memory().ok_or(MediaError::Empty)?;
            Input::Memory(Arc::clone(audio))
        }
        super::Origin::File(id) => {
            let path = crate::media::streams::path(*id)
                .ok_or_else(|| MediaError::Io(format!("no stream file for media {id}")))?;
            let reader = Reader::open(&path)?;
            let channels = channels.min(usize::from(reader.layout().channels));
            Input::File {
                reader: Box::new(reader),
                channels,
            }
        }
    };
    lumen_media::slicer::scan(&mut input, rate)
}
