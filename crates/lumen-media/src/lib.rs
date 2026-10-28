//! Audio files for playback (M2 slice 6): decode with symphonia (MPL-2.0,
//! owner decision, used unmodified) and convert to the device's sample rate
//! with rubato (MIT/Apache-2.0). The result lives in memory, or, for large
//! sessions, is read from disk as it plays ([`stream`], M9 slice R6).
//!
//! Formats: WAV, AIFF, FLAC, MP3, Ogg Vorbis. AAC/M4A are deferred until
//! their licensing is checked.
//!
//! Everything here is slow work for a background thread: it allocates, reads
//! the disk and can take seconds. It takes a cancel flag and reports
//! progress.

pub mod export;
mod peaks;
pub mod stream;
pub mod take;
pub mod wav;

pub use peaks::{
    BASE as PEAK_BASE, Bucket, FACTOR as PEAK_FACTOR, Growing as GrowingPeaks,
    LEVELS as PEAK_LEVELS, Peaks,
};

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Decoded audio, channel after channel (planar), ready to play.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Builds audio from per-channel samples. All channels must be the same
    /// length; at least one is required.
    pub fn new(sample_rate: u32, channels: Vec<Vec<f32>>) -> Result<Self, MediaError> {
        let frames = channels.first().map(Vec::len).ok_or(MediaError::Empty)?;
        if channels.iter().any(|c| c.len() != frames) {
            return Err(MediaError::Decode("channels of different lengths".into()));
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    pub fn frames(&self) -> usize {
        self.channels[0].len()
    }

    pub fn channel(&self, index: usize) -> &[f32] {
        &self.channels[index]
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate.max(1))
    }

    /// The samples, channel after channel, without copying.
    /// The waveform peaks of this audio; `None` if cancelled.
    pub fn peaks(&self, cancel: &AtomicBool) -> Option<Peaks> {
        Peaks::build(&self.channels, cancel)
    }

    pub fn into_channels(self) -> Vec<Vec<f32>> {
        self.channels
    }

    /// Bytes of sample memory held.
    pub fn memory(&self) -> usize {
        self.channels() * self.frames() * std::mem::size_of::<f32>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// The file could not be opened or read.
    Io(String),
    /// Not an audio format Lumen reads (or AAC/M4A, deferred).
    Unsupported(String),
    /// The file is damaged.
    Decode(String),
    /// No audio in the file.
    Empty,
    Cancelled,
    Resample(String),
    Encode(String),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaError::Io(m) => write!(f, "Could not read the file: {m}"),
            MediaError::Unsupported(m) => write!(
                f,
                "Not a supported audio format ({m}). Use WAV, AIFF, FLAC, MP3 or Ogg Vorbis"
            ),
            MediaError::Decode(m) => write!(f, "The file is damaged: {m}"),
            MediaError::Empty => write!(f, "The file contains no audio"),
            MediaError::Cancelled => write!(f, "Canceled"),
            MediaError::Resample(m) => write!(f, "Sample-rate conversion failed: {m}"),
            MediaError::Encode(m) => write!(f, "Audio encoding failed: {m}"),
        }
    }
}

impl std::error::Error for MediaError {}

/// Writes a small file whole: under a temporary name, then renamed into
/// place, so a reader never sees half of it.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), MediaError> {
    let failed = |e: std::io::Error| MediaError::Io(e.to_string());
    let temp = path.with_extension(format!("{}.writing", std::process::id()));
    std::fs::write(&temp, bytes).map_err(failed)?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        failed(e)
    })
}

fn cancelled(cancel: &AtomicBool) -> Result<(), MediaError> {
    if cancel.load(Ordering::Relaxed) {
        Err(MediaError::Cancelled)
    } else {
        Ok(())
    }
}

/// Decodes a whole file. `progress` receives 0.0–1.0 when the length is
/// known up front.
pub fn decode(
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Audio, MediaError> {
    let mut rate = None;
    let mut channels: Vec<Vec<f32>> = Vec::new();
    decode_blocks(path, cancel, progress, &mut |from, planes, frames| {
        rate = Some(from);
        if channels.is_empty() {
            channels = vec![Vec::new(); planes.len()];
        }
        for (all, part) in channels.iter_mut().zip(planes) {
            all.extend_from_slice(&part[..frames]);
        }
        Ok(())
    })?;
    let rate = rate.ok_or(MediaError::Empty)?;
    Audio::new(rate, channels)
}

/// What [`read_blocks`] hands each block to: the file's rate and one slice
/// per channel, all of one length.
pub type Blocks<'s> = dyn FnMut(u32, &[&[f32]]) -> Result<(), MediaError> + 's;

/// Reads `path` block by block without holding it whole (M9 slice R6), its
/// first two channels: an uncompressed WAV, RF64 or AIFF file directly
/// (RF64 is beyond the decoder), any other file decoded packet by packet.
/// Fails with [`MediaError::Empty`] when the file holds no audio, and with
/// [`MediaError::Cancelled`] when `cancel` is set.
pub fn read_blocks(
    path: &Path,
    cancel: &AtomicBool,
    sink: &mut Blocks<'_>,
) -> Result<(), MediaError> {
    const BLOCK: usize = 65_536;
    if crate::stream::layout(path).ok().flatten().is_none() {
        return decode_blocks(path, cancel, &mut |_| {}, &mut |rate, planes, frames| {
            let parts: Vec<&[f32]> = planes.iter().take(2).map(|p| &p[..frames]).collect();
            sink(rate, &parts)
        });
    }
    let mut reader = crate::stream::Reader::open(path)?;
    let layout = *reader.layout();
    if layout.frames == 0 {
        return Err(MediaError::Empty);
    }
    let mut planes = vec![vec![0.0; BLOCK]; layout.played()];
    let mut at = 0;
    while at < layout.frames {
        cancelled(cancel)?;
        let n = ((layout.frames - at) as usize).min(BLOCK);
        let mut parts: Vec<&mut [f32]> = planes.iter_mut().map(|p| &mut p[..n]).collect();
        reader.read(at, &mut parts)?;
        let parts: Vec<&[f32]> = planes.iter().map(|p| &p[..n]).collect();
        sink(layout.sample_rate, &parts)?;
        at += n as u64;
    }
    Ok(())
}

/// What [`decode_blocks`] hands each packet to: the file's rate, the planes
/// and the frames in them.
pub(crate) type Packets<'s> = dyn FnMut(u32, &[Vec<f32>], usize) -> Result<(), MediaError> + 's;

/// Decodes `path` packet by packet: `sink` receives the file's rate and each
/// packet's planes (one per channel, the first `frames` samples valid). Fails
/// with [`MediaError::Empty`] when no audio was decoded.
pub(crate) fn decode_blocks(
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
    sink: &mut Packets<'_>,
) -> Result<(), MediaError> {
    let file = File::open(path).map_err(|e| MediaError::Io(e.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(MediaError::Empty)?;
    let track_id = track.id;
    let total = track.num_frames;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| MediaError::Unsupported("no audio codec parameters".into()))?;
    let mut sample_rate = params.sample_rate;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;

    let mut count = None;
    let mut decoded = 0_u64;
    let mut packet_planes: Vec<Vec<f32>> = Vec::new();
    loop {
        cancelled(cancel)?;
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(MediaError::Decode(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            // A damaged packet: skip it, as players do.
            Err(DecodeError::DecodeError(_)) => continue,
            Err(e) => return Err(MediaError::Decode(e.to_string())),
        };
        sample_rate = sample_rate.or(Some(buffer.spec().rate()));
        let rate =
            sample_rate.ok_or_else(|| MediaError::Unsupported("unknown sample rate".into()))?;
        let frames = buffer.frames();
        buffer.copy_to_vecs_planar::<f32>(&mut packet_planes);
        if *count.get_or_insert(packet_planes.len()) != packet_planes.len() {
            return Err(MediaError::Decode("the channel count changed".into()));
        }
        if frames > 0 {
            sink(rate, &packet_planes, frames)?;
            decoded += frames as u64;
        }
        if let Some(total) = total.filter(|&t| t > 0) {
            progress((decoded as f64 / total as f64).min(1.0) as f32);
        }
    }
    if decoded == 0 || count == Some(0) {
        return Err(MediaError::Empty);
    }
    progress(1.0);
    Ok(())
}

/// What a file's header says, found without decoding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
    /// The length in frames, when the format records it.
    pub frames: Option<u64>,
}

/// Reads the header of `path`: quick, for checking a file against what a
/// project knows of it (relinking).
pub fn probe(path: &Path) -> Result<Header, MediaError> {
    let file = File::open(path).map_err(|e| MediaError::Io(e.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(MediaError::Empty)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| MediaError::Unsupported("no audio codec parameters".into()))?;
    Ok(Header {
        sample_rate: params.sample_rate,
        channels: params.channels.as_ref().map(|c| c.count()),
        frames: track.num_frames,
    })
}

/// Converts `audio` to `rate` (a copy when already at that rate).
pub fn resample(audio: &Audio, rate: u32, cancel: &AtomicBool) -> Result<Audio, MediaError> {
    if audio.sample_rate == rate {
        return Ok(audio.clone());
    }
    cancelled(cancel)?;
    let channels = audio.channels();
    let mut resampler = Fft::<f32>::new(
        audio.sample_rate as usize,
        rate as usize,
        4096,
        channels,
        FixedSync::Input,
    )
    .map_err(|e| MediaError::Resample(e.to_string()))?;
    let input = SequentialSliceOfVecs::new(&audio.channels, channels, audio.frames())
        .map_err(|e| MediaError::Resample(e.to_string()))?;
    let output = resampler
        .process_all(&input, audio.frames(), None)
        .map_err(|e| MediaError::Resample(e.to_string()))?;
    cancelled(cancel)?;
    let interleaved = output.take_data();
    let planar = (0..channels)
        .map(|c| {
            interleaved
                .iter()
                .skip(c)
                .step_by(channels)
                .copied()
                .collect()
        })
        .collect();
    Audio::new(rate, planar)
}

/// Decodes `path` and converts it to `rate`. Progress covers both steps.
pub fn load(
    path: &Path,
    rate: u32,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Audio, MediaError> {
    let decoded = decode(path, cancel, &mut |p| progress(p * 0.8))?;
    let converted = resample(&decoded, rate, cancel)?;
    progress(1.0);
    Ok(converted)
}
