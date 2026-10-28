//! Converting a file into a stream file without holding it in memory: decode
//! packet by packet, convert the rate block by block, write as it goes.
//!
//! [`Converter`] gives the same samples as [`crate::resample`] does on the
//! whole file at once. It hands rubato the same chunks in the same order as
//! `process_all`: full chunks while more than a chunk is left, then the rest
//! as a partial chunk, then empty chunks until the output is long enough. It
//! also trims the same start-up delay and stops at the same length. So a clip
//! streamed from its converted copy sounds exactly as it does in memory.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::wav::{Sample, Spec, Writer};
use crate::{MediaError, cancelled};

/// Input frames per conversion step, as [`crate::resample`] uses.
const CHUNK: usize = 4096;

/// What converted blocks are handed to: one slice per channel.
pub type Sink<'s> = dyn FnMut(&[&[f32]]) -> Result<(), MediaError> + 's;

fn failed(error: impl std::fmt::Display) -> MediaError {
    MediaError::Resample(error.to_string())
}

/// Sample-rate conversion in blocks.
pub struct Converter {
    /// `None` when the rates are equal: blocks pass through.
    resampler: Option<Fft<f32>>,
    ratio: f64,
    channels: usize,
    /// Input not converted yet: at most one chunk plus a block.
    pending: Vec<Vec<f32>>,
    /// Input frames taken so far.
    taken: u64,
    /// Output frames given so far.
    given: u64,
    /// Start-up delay still to drop.
    trim: usize,
    out: Vec<Vec<f32>>,
}

impl Converter {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, MediaError> {
        if from == 0 || to == 0 || channels == 0 {
            return Err(failed("a rate or channel count of 0"));
        }
        let resampler = (from != to)
            .then(|| {
                Fft::<f32>::new(
                    from as usize,
                    to as usize,
                    CHUNK,
                    channels,
                    FixedSync::Input,
                )
            })
            .transpose()
            .map_err(failed)?;
        let (ratio, trim, size) = match &resampler {
            Some(r) => (r.resample_ratio(), r.output_delay(), r.output_frames_max()),
            None => (1.0, 0, 0),
        };
        Ok(Self {
            resampler,
            ratio,
            channels,
            pending: vec![Vec::with_capacity(2 * CHUNK); channels],
            taken: 0,
            given: 0,
            trim,
            out: vec![vec![0.0; size]; channels],
        })
    }

    /// Takes a block (one slice per channel, all the same length) and hands
    /// `sink` whatever output is ready.
    pub fn push(&mut self, block: &[&[f32]], sink: &mut Sink<'_>) -> Result<(), MediaError> {
        let frames = block.first().map_or(0, |c| c.len());
        if block.len() != self.channels || block.iter().any(|c| c.len() != frames) {
            return Err(failed("a block must hold one equal slice per channel"));
        }
        self.taken += frames as u64;
        if self.resampler.is_none() {
            self.given += frames as u64;
            return if frames > 0 { sink(block) } else { Ok(()) };
        }
        for (pending, part) in self.pending.iter_mut().zip(block) {
            pending.extend_from_slice(part);
        }
        // A chunk is converted only while more than a chunk is left, as
        // `process_all` does: the last chunk may be partial.
        while self.pending[0].len() > CHUNK {
            self.step(None, None, sink)?;
            for pending in &mut self.pending {
                pending.drain(..CHUNK);
            }
        }
        Ok(())
    }

    /// Converts what is left and pads to the full length; returns the output
    /// frames given in all.
    pub fn finish(mut self, sink: &mut Sink<'_>) -> Result<u64, MediaError> {
        if self.resampler.is_none() {
            return Ok(self.given);
        }
        let expected = (self.ratio * self.taken as f64).ceil() as u64;
        let left = self.pending[0].len();
        if left > 0 {
            self.step(Some(left), Some(expected), sink)?;
        }
        while self.given < expected {
            self.step(Some(0), Some(expected), sink)?;
        }
        Ok(self.given)
    }

    /// One call to the resampler on the start of `pending`: a full chunk for
    /// `None`, else the `partial` frames left; output past `most` is dropped.
    fn step(
        &mut self,
        partial: Option<usize>,
        most: Option<u64>,
        sink: &mut Sink<'_>,
    ) -> Result<(), MediaError> {
        let resampler = self.resampler.as_mut().expect("a step converts");
        let len = self.pending[0].len();
        let input =
            SequentialSliceOfVecs::new(&self.pending, self.channels, len).map_err(failed)?;
        let size = self.out[0].len();
        let mut output =
            SequentialSliceOfVecs::new_mut(&mut self.out, self.channels, size).map_err(failed)?;
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: partial,
            active_channels_mask: None,
        };
        let (_, written) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(failed)?;
        let skip = self.trim.min(written);
        self.trim -= skip;
        let mut take = written - skip;
        if let Some(most) = most {
            take = take.min(most.saturating_sub(self.given) as usize);
        }
        if take > 0 {
            let parts: Vec<&[f32]> = self.out.iter().map(|c| &c[skip..skip + take]).collect();
            sink(&parts)?;
            self.given += take as u64;
        }
        Ok(())
    }
}

/// Decodes `source`, converts it to `rate` and writes its first two channels
/// as a 32-bit float WAV at `destination` (in place only when finished, as
/// [`Writer`] does). Returns the frames written.
pub fn convert(
    source: &Path,
    rate: u32,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<u64, MediaError> {
    let mut state: Option<(Converter, Writer)> = None;
    crate::decode_blocks(source, cancel, progress, &mut |from, planes, frames| {
        cancelled(cancel)?;
        if state.is_none() {
            let channels = planes.len().min(2);
            let spec = Spec {
                sample_rate: rate,
                channels: channels as u16,
                sample: Sample::F32,
            };
            state = Some((
                Converter::new(from, rate, channels)?,
                Writer::create(destination, spec)?,
            ));
        }
        let (converter, writer) = state.as_mut().expect("made above");
        let parts: Vec<&[f32]> = planes.iter().take(2).map(|p| &p[..frames]).collect();
        converter.push(&parts, &mut |block| writer.write(block))
    })?;
    let (converter, mut writer) = state.ok_or(MediaError::Empty)?;
    converter.finish(&mut |block| writer.write(block))?;
    cancelled(cancel)?;
    writer.finish()
}
