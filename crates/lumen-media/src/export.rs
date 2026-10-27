//! Standalone export encoding from a rendered stereo float WAV.
//!
//! All work is intended for a worker thread. WAV output is written in bounded
//! blocks to a sibling temporary file and installed only after it is complete.
//! FLAC frames are encoded individually to a temporary file, with cancellation
//! between frames. Decoding and rate conversion currently hold audio in RAM.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use flacenc::component::{BitRepr, Stream};
use flacenc::error::Verify;
use flacenc::source::{Context, Fill, FrameBuf};
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::{Audio, MediaError, cancelled};

const BLOCK_FRAMES: usize = 16_384;

/// The destination encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    WavI16,
    WavI24,
    WavF32,
    FlacI16,
    FlacI24,
}

impl Format {
    fn bits(self) -> u32 {
        match self {
            Self::WavI16 | Self::FlacI16 => 16,
            Self::WavI24 | Self::FlacI24 => 24,
            Self::WavF32 => 32,
        }
    }

    fn is_float(self) -> bool {
        matches!(self, Self::WavF32)
    }

    fn is_flac(self) -> bool {
        matches!(self, Self::FlacI16 | Self::FlacI24)
    }
}

/// Output options. Dither is applied only to integer formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub sample_rate: u32,
    pub format: Format,
    pub normalize_peak: bool,
    pub tpdf_dither: bool,
}

/// Summary of the audio that was installed at the destination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResultInfo {
    pub frames: u64,
    /// Maximum absolute sample after normalization and output-rate conversion.
    pub sample_peak: f32,
    /// Linear gain applied by peak normalization, or 1.0 when it was not used.
    pub normalization_gain: f64,
}

/// Encodes a stereo 32-bit float WAV render to `destination`.
///
/// The destination is untouched until encoding succeeds. Cancellation is
/// checked during decode, rate-conversion blocks, WAV writes and between FLAC
/// frames. Progress runs from 0.0 to 1.0.
pub fn encode(
    source: &Path,
    destination: &Path,
    spec: Spec,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<ResultInfo, MediaError> {
    if spec.sample_rate == 0 {
        return Err(MediaError::Unsupported("a sample rate of 0".into()));
    }
    validate_float_stereo_wav(source)?;
    cancelled(cancel)?;
    let mut decode_progress = |p: f32| progress(p * 0.25);
    let decoded = crate::decode(source, cancel, &mut decode_progress)?;
    if decoded.channels() != 2 {
        return Err(MediaError::Unsupported(format!(
            "{} channels (export requires stereo)",
            decoded.channels()
        )));
    }
    let audio = if decoded.sample_rate == spec.sample_rate {
        decoded
    } else {
        resample_stereo(&decoded, spec.sample_rate, cancel, progress)?
    };
    cancelled(cancel)?;

    let mut peak = 0.0f32;
    for channel in &audio.channels {
        for block in channel.chunks(BLOCK_FRAMES) {
            cancelled(cancel)?;
            for &v in block {
                if !v.is_finite() {
                    return Err(MediaError::Encode(
                        "the render contains non-finite samples".into(),
                    ));
                }
                peak = peak.max(v.abs());
            }
        }
    }
    let normalization_gain = if spec.normalize_peak && peak > 0.0 {
        10.0_f64.powf(-1.0 / 20.0) / f64::from(peak)
    } else {
        1.0
    };
    let mut output_peak = (f64::from(peak) * normalization_gain) as f32;
    if !spec.format.is_float() {
        output_peak = output_peak.min(1.0);
    }
    let result = ResultInfo {
        frames: audio.frames() as u64,
        sample_peak: output_peak,
        normalization_gain,
    };

    if spec.format.is_flac() {
        cancelled(cancel)?;
        let (header, frames) = encode_flac(
            &audio,
            spec,
            normalization_gain,
            destination,
            cancel,
            progress,
        )?;
        commit_parts(destination, &header, &frames.0, cancel, progress)?;
        progress(1.0);
    } else {
        encode_wav(
            destination,
            &audio,
            spec,
            normalization_gain,
            cancel,
            progress,
        )?;
    }
    Ok(result)
}

fn validate_float_stereo_wav(path: &Path) -> Result<(), MediaError> {
    let mut file = File::open(path).map_err(crate::wav::io)?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header).map_err(crate::wav::io)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(MediaError::Unsupported("source is not a RIFF WAV".into()));
    }
    loop {
        let mut chunk = [0_u8; 8];
        file.read_exact(&mut chunk).map_err(crate::wav::io)?;
        let size = u32::from_le_bytes(chunk[4..8].try_into().expect("four bytes")) as usize;
        if &chunk[..4] == b"fmt " {
            if size < 16 {
                return Err(MediaError::Decode("invalid WAV format chunk".into()));
            }
            let mut fmt = vec![0; size.min(64 * 1024)];
            file.read_exact(&mut fmt).map_err(crate::wav::io)?;
            let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
            let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            if tag != 3 || channels != 2 || bits != 32 {
                return Err(MediaError::Unsupported(
                    "source must be stereo 32-bit float WAV".into(),
                ));
            }
            return Ok(());
        }
        let skip = size + (size & 1);
        file.seek_relative(skip as i64).map_err(crate::wav::io)?;
    }
}

fn resample_stereo(
    audio: &Audio,
    rate: u32,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Audio, MediaError> {
    cancelled(cancel)?;
    let mut resampler = Fft::<f32>::new(
        audio.sample_rate as usize,
        rate as usize,
        4096,
        2,
        FixedSync::Input,
    )
    .map_err(|e| MediaError::Resample(e.to_string()))?;
    let input = SequentialSliceOfVecs::new(&audio.channels, 2, audio.frames())
        .map_err(|e| MediaError::Resample(e.to_string()))?;
    let expected =
        (audio.frames() as f64 * f64::from(rate) / f64::from(audio.sample_rate)).ceil() as usize;
    let mut channels: Vec<_> = (0..2).map(|_| Vec::with_capacity(expected)).collect();
    let mut consumed = 0usize;
    let mut delay = resampler.output_delay();
    let size = resampler.output_frames_max();
    let mut buffers = vec![vec![0.0; size]; 2];
    while channels[0].len() < expected {
        cancelled(cancel)?;
        let left = audio.frames().saturating_sub(consumed);
        let indexing = Indexing {
            input_offset: consumed,
            output_offset: 0,
            partial_len: (left < resampler.input_frames_next()).then_some(left),
            active_channels_mask: None,
        };
        let mut output = SequentialSliceOfVecs::new_mut(&mut buffers, 2, size)
            .map_err(|e| MediaError::Resample(e.to_string()))?;
        let (read, written) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| MediaError::Resample(e.to_string()))?;
        consumed = consumed.saturating_add(read.min(left));
        let skip = delay.min(written);
        delay -= skip;
        let take = (written - skip).min(expected - channels[0].len());
        for c in 0..2 {
            channels[c].extend_from_slice(&buffers[c][skip..skip + take]);
        }
        progress(0.3 + 0.2 * consumed as f32 / audio.frames().max(1) as f32);
    }
    progress(0.5);
    Audio::new(rate, channels)
}

fn encode_wav(
    destination: &Path,
    audio: &Audio,
    spec: Spec,
    gain: f64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<(), MediaError> {
    use crate::wav::{Sample, Spec as WavSpec, Writer};
    let sample = match spec.format {
        Format::WavI16 => Sample::I16,
        Format::WavI24 => Sample::I24,
        Format::WavF32 => Sample::F32,
        _ => return Err(MediaError::Unsupported("invalid WAV format".into())),
    };
    let temp = temporary_path(destination)?;
    let _cleanup = TempCleanup(temp.clone());
    let mut writer = Writer::create(
        &temp,
        WavSpec {
            sample_rate: spec.sample_rate,
            channels: 2,
            sample,
        },
    )?;
    let mut rng = Dither::new();
    for start in (0..audio.frames()).step_by(BLOCK_FRAMES) {
        cancelled(cancel)?;
        let end = (start + BLOCK_FRAMES).min(audio.frames());
        let mut left = Vec::with_capacity(end - start);
        let mut right = Vec::with_capacity(end - start);
        for i in start..end {
            let mut l = f64::from(audio.channels[0][i]) * gain;
            let mut r = f64::from(audio.channels[1][i]) * gain;
            if spec.tpdf_dither && !spec.format.is_float() {
                let scale = 2_f64.powi(spec.format.bits() as i32 - 1);
                l += f64::from(rng.tpdf()) / scale;
                r += f64::from(rng.tpdf()) / scale;
            }
            // Quantize in f64 before conversion to the exactly representable
            // PCM step, so f32 addition cannot bias 24-bit dither.
            let pcm = |v: f64| {
                if spec.format.is_float() {
                    v as f32
                } else {
                    let scale = 2_f64.powi(spec.format.bits() as i32 - 1);
                    ((v.clamp(-1.0, 1.0) * scale)
                        .round()
                        .clamp(-scale, scale - 1.0)
                        / scale) as f32
                }
            };
            left.push(pcm(l));
            right.push(pcm(r));
        }
        writer.write(&[&left, &right])?;
        progress(0.5 + 0.49 * end as f32 / audio.frames().max(1) as f32);
    }
    cancelled(cancel)?;
    writer.finish()?;
    install_temp(&temp, destination)?;
    progress(1.0);
    Ok(())
}

fn encode_flac(
    audio: &Audio,
    spec: Spec,
    gain: f64,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<(Vec<u8>, TempCleanup), MediaError> {
    let bits = spec.format.bits() as usize;
    let scale = (1_i64 << (bits - 1)) as f64;
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| MediaError::Encode(format!("invalid encoder configuration: {e:?}")))?;
    let block_size = config.block_size;
    let frame_path = temporary_path(destination)?;
    let cleanup = TempCleanup(frame_path.clone());
    let mut frame_file = BufWriter::new(File::create(&frame_path).map_err(crate::wav::io)?);
    let mut framebuf =
        FrameBuf::with_size(2, block_size).map_err(|e| MediaError::Encode(e.to_string()))?;
    let mut context = Context::new(bits, 2);
    let mut stream = Stream::new(spec.sample_rate as usize, 2, bits)
        .map_err(|e| MediaError::Encode(e.to_string()))?;
    let mut rng = Dither::new();
    for (frame_number, start) in (0..audio.frames()).step_by(block_size).enumerate() {
        cancelled(cancel)?;
        let end = (start + block_size).min(audio.frames());
        let mut samples = Vec::with_capacity((end - start) * 2);
        for i in start..end {
            for channel in 0..2 {
                let mut value = f64::from(audio.channels[channel][i]) * gain;
                if spec.tpdf_dither {
                    value += f64::from(rng.tpdf()) / scale;
                }
                samples.push(
                    (value.clamp(-1.0, 1.0) * scale)
                        .round()
                        .clamp(-scale, scale - 1.0) as i32,
                );
            }
        }
        framebuf
            .fill_interleaved(&samples)
            .map_err(|e| MediaError::Encode(e.to_string()))?;
        context
            .fill_interleaved(&samples)
            .map_err(|e| MediaError::Encode(e.to_string()))?;
        let frame = flacenc::encode_fixed_size_frame(
            &config,
            &framebuf,
            frame_number,
            stream.stream_info(),
        )
        .map_err(|e| MediaError::Encode(e.to_string()))?;
        stream.stream_info_mut().update_frame_info(&frame);
        let mut sink = flacenc::bitsink::ByteSink::new();
        frame
            .write(&mut sink)
            .map_err(|e| MediaError::Encode(e.to_string()))?;
        frame_file
            .write_all(sink.as_slice())
            .map_err(crate::wav::io)?;
        progress(0.5 + 0.24 * end as f32 / audio.frames().max(1) as f32);
    }
    frame_file.flush().map_err(crate::wav::io)?;
    frame_file.get_ref().sync_all().map_err(crate::wav::io)?;
    drop(frame_file);
    stream.stream_info_mut().set_total_samples(audio.frames());
    // Fixed blocking uses this nominal size even when the last frame is
    // shorter. Readers derive frame positions from it.
    stream
        .stream_info_mut()
        .set_block_sizes(block_size, block_size)
        .map_err(|e| MediaError::Encode(e.to_string()))?;
    stream
        .stream_info_mut()
        .set_md5_digest(&context.md5_digest());
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| MediaError::Encode(e.to_string()))?;
    Ok((sink.as_slice().to_vec(), cleanup))
}

struct Dither(u64);

struct TempCleanup(PathBuf);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Dither {
    fn new() -> Self {
        Self(0x9e37_79b9_7f4a_7c15)
    }

    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / ((1_u32 << 24) as f32)
    }

    fn tpdf(&mut self) -> f32 {
        self.unit() - self.unit()
    }
}

fn temporary_path(destination: &Path) -> Result<PathBuf, MediaError> {
    let name = destination
        .file_name()
        .ok_or_else(|| MediaError::Io("the path has no file name".into()))?;
    let dir = destination
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| MediaError::Io(e.to_string()))?
        .as_nanos();
    Ok(dir.join(format!(
        ".{}.{}.{}.exporting",
        name.to_string_lossy(),
        std::process::id(),
        nonce
    )))
}

fn commit_parts(
    destination: &Path,
    header: &[u8],
    frames: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<(), MediaError> {
    let temp = temporary_path(destination)?;
    let _cleanup = TempCleanup(temp.clone());
    (|| {
        let mut file = BufWriter::new(File::create(&temp).map_err(crate::wav::io)?);
        file.write_all(header).map_err(crate::wav::io)?;
        let mut frames = BufReader::new(File::open(frames).map_err(crate::wav::io)?);
        let total = frames.get_ref().metadata().map_err(crate::wav::io)?.len();
        let mut copied = 0_u64;
        let mut block = [0_u8; 64 * 1024];
        loop {
            cancelled(cancel)?;
            let read = frames.read(&mut block).map_err(crate::wav::io)?;
            if read == 0 {
                break;
            }
            file.write_all(&block[..read]).map_err(crate::wav::io)?;
            copied += read as u64;
            progress(0.75 + 0.24 * copied as f32 / total.max(1) as f32);
        }
        file.flush().map_err(crate::wav::io)?;
        file.get_ref().sync_all().map_err(crate::wav::io)?;
        drop(file);
        cancelled(cancel)?;
        install_temp(&temp, destination)
    })()
}

fn install_temp(temp: &Path, destination: &Path) -> Result<(), MediaError> {
    if !destination.exists() {
        return fs::rename(temp, destination).map_err(crate::wav::io);
    }
    let backup = temporary_path(destination)?.with_extension("previous");
    fs::rename(destination, &backup).map_err(crate::wav::io)?;
    match fs::rename(temp, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup, destination);
            Err(crate::wav::io(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::{Sample, Spec as WavSpec, Writer};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn folder(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lumen-export-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn source(path: &Path, frames: usize, amplitude: f32) {
        let left: Vec<_> = (0..frames)
            .map(|i| (i as f32 * 0.071).sin() * amplitude)
            .collect();
        let right: Vec<_> = (0..frames)
            .map(|i| (i as f32 * 0.037).cos() * amplitude * 0.5)
            .collect();
        let mut writer = Writer::create(
            path,
            WavSpec {
                sample_rate: 48_000,
                channels: 2,
                sample: Sample::F32,
            },
        )
        .unwrap();
        writer.write(&[&left, &right]).unwrap();
        writer.finish().unwrap();
    }

    fn spec(format: Format) -> Spec {
        Spec {
            sample_rate: 48_000,
            format,
            normalize_peak: false,
            tpdf_dither: false,
        }
    }

    #[test]
    fn exports_wav_and_flac_in_requested_depth_and_decode() {
        let dir = folder("formats");
        let input = dir.join("render.wav");
        source(&input, 10_000, 0.6);
        for (format, depth, flac) in [
            (Format::WavI16, 16, false),
            (Format::WavI24, 24, false),
            (Format::WavF32, 32, false),
            (Format::FlacI16, 16, true),
            (Format::FlacI24, 24, true),
        ] {
            let output = dir.join(format!("{format:?}.{}", if flac { "flac" } else { "wav" }));
            let result = encode(
                &input,
                &output,
                spec(format),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap();
            assert_eq!(result.frames, 10_000);
            let decoded = crate::decode(&output, &AtomicBool::new(false), &mut |_| {}).unwrap();
            assert_eq!(decoded.sample_rate, 48_000);
            assert_eq!(decoded.frames(), 10_000);
            let bytes = fs::read(&output).unwrap();
            let found_depth = if flac {
                let streaminfo = u64::from_be_bytes(bytes[18..26].try_into().unwrap());
                ((streaminfo >> 36) & 0x1f) as u16 + 1
            } else {
                u16::from_le_bytes([bytes[34], bytes[35]])
            };
            assert_eq!(found_depth, depth, "{format:?}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rate_conversion_sets_expected_duration_and_normalization_peak() {
        let dir = folder("rate");
        let input = dir.join("render.wav");
        let output = dir.join("normalized.wav");
        source(&input, 4800, 0.2);
        let options = Spec {
            sample_rate: 44_100,
            normalize_peak: true,
            ..spec(Format::WavF32)
        };
        let result = encode(
            &input,
            &output,
            options,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(result.frames, 4410);
        assert!(result.normalization_gain > 4.4 && result.normalization_gain < 4.5);
        assert!((result.sample_peak - 10.0_f32.powf(-1.0 / 20.0)).abs() < 0.0001);
        let decoded = crate::decode(&output, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(decoded.frames(), 4410);
        assert_eq!(decoded.sample_rate, 44_100);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dither_is_repeatable_and_cancellation_preserves_existing_destination() {
        let dir = folder("safety");
        let input = dir.join("render.wav");
        let output = dir.join("existing.wav");
        source(&input, 20_000, 0.000_001);
        let mut options = Spec {
            tpdf_dither: true,
            ..spec(Format::WavI16)
        };
        encode(
            &input,
            &output,
            options,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let first = fs::read(&output).unwrap();
        encode(
            &input,
            &output,
            options,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(first, fs::read(&output).unwrap());
        assert!(first[44..].iter().any(|&v| v != 0));

        fs::write(&output, b"keep this file").unwrap();
        let cancel = AtomicBool::new(false);
        options.tpdf_dither = false;
        let result = encode(&input, &output, options, &cancel, &mut |p| {
            if p >= 0.25 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(result.unwrap_err(), MediaError::Cancelled);
        assert_eq!(fs::read(&output).unwrap(), b"keep this file");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn integer_wav_saturates_out_of_range_samples() {
        let dir = folder("saturate");
        let input = dir.join("render.wav");
        let output = dir.join("saturated.wav");
        source(&input, 512, 1.7);
        encode(
            &input,
            &output,
            spec(Format::WavI16),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let decoded = crate::decode(&output, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(decoded.channel(0).iter().all(|sample| *sample <= 1.0));
        assert!(decoded.channel(0).iter().any(|sample| *sample > 0.99));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn flac_with_a_partial_last_frame_decodes_exactly_and_can_be_cancelled() {
        let dir = folder("flac-partial");
        let input = dir.join("render.wav");
        let output = dir.join("prime.flac");
        source(&input, 10_007, 0.2);
        encode(
            &input,
            &output,
            spec(Format::FlacI24),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let decoded = crate::decode(&output, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(decoded.frames(), 10_007);
        let original = crate::decode(&input, &AtomicBool::new(false), &mut |_| {}).unwrap();
        for c in 0..2 {
            for (&a, &b) in decoded.channel(c).iter().zip(original.channel(c)) {
                assert!((a - b).abs() <= 1.0 / 8_388_608.0);
            }
        }
        let before = fs::read(&output).unwrap();
        let cancel = AtomicBool::new(false);
        let result = encode(&input, &output, spec(Format::FlacI24), &cancel, &mut |p| {
            if p > 0.5 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(result.unwrap_err(), MediaError::Cancelled);
        assert_eq!(fs::read(&output).unwrap(), before);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
}
