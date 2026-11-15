pub mod detect;
pub mod export;
mod peaks;
pub mod repitch;
pub mod slicer;
pub mod stream;
pub mod stretch;
pub mod take;
pub mod tempo;
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

#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

impl Audio {
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

    pub fn peaks(&self, cancel: &AtomicBool) -> Option<Peaks> {
        Peaks::build(&self.channels, cancel)
    }

    pub fn into_channels(self) -> Vec<Vec<f32>> {
        self.channels
    }

    pub fn memory(&self) -> usize {
        self.channels() * self.frames() * std::mem::size_of::<f32>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    Io(String),
    Unsupported(String),
    Decode(String),
    Empty,
    Cancelled,
    NotReady,
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
            MediaError::NotReady => write!(f, "Not ready yet"),
            MediaError::Resample(m) => write!(f, "Sample-rate conversion failed: {m}"),
            MediaError::Encode(m) => write!(f, "Audio encoding failed: {m}"),
        }
    }
}

impl std::error::Error for MediaError {}

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

pub type Blocks<'s> = dyn FnMut(u32, &[&[f32]]) -> Result<(), MediaError> + 's;

pub fn read_blocks(
    path: &Path,
    cancel: &AtomicBool,
    sink: &mut Blocks<'_>,
) -> Result<(), MediaError> {
    read_blocks_from(path, 0, BLOCK_FRAMES, cancel, sink)
}

const BLOCK_FRAMES: usize = 65_536;

pub fn read_blocks_from(
    path: &Path,
    start: u64,
    block: usize,
    cancel: &AtomicBool,
    sink: &mut Blocks<'_>,
) -> Result<(), MediaError> {
    let block = block.max(1);
    if crate::stream::layout(path).ok().flatten().is_none() {
        let mut skip = start;
        return decode_blocks(path, cancel, &mut |_| {}, &mut |rate, planes, frames| {
            let from = usize::try_from(skip).unwrap_or(usize::MAX).min(frames);
            skip -= from as u64;
            if from == frames {
                return Ok(());
            }
            let parts: Vec<&[f32]> = planes.iter().take(2).map(|p| &p[from..frames]).collect();
            sink(rate, &parts)
        });
    }
    let mut reader = crate::stream::Reader::open(path)?;
    let layout = *reader.layout();
    if layout.frames == 0 {
        return Err(MediaError::Empty);
    }
    let mut planes = vec![vec![0.0; block]; layout.played()];
    let mut at = start.min(layout.frames);
    while at < layout.frames {
        cancelled(cancel)?;
        let n = ((layout.frames - at) as usize).min(block);
        let mut parts: Vec<&mut [f32]> = planes.iter_mut().map(|p| &mut p[..n]).collect();
        reader.read(at, &mut parts)?;
        let parts: Vec<&[f32]> = planes.iter().map(|p| &p[..n]).collect();
        sink(layout.sample_rate, &parts)?;
        at += n as u64;
    }
    Ok(())
}

pub(crate) type Packets<'s> = dyn FnMut(u32, &[Vec<f32>], usize) -> Result<(), MediaError> + 's;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
    pub frames: Option<u64>,
}

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
