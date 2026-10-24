//! A WAV file writer (M7b slice E3). Rendered audio (a consolidated clip, a
//! bounce) is written here: 16 or 24-bit integer or 32-bit float, mono or
//! stereo, streamed block by block so a long render never holds its file
//! twice in memory.
//!
//! The file is written beside its destination under a temporary name, flushed
//! and renamed over it by [`Writer::finish`], so a crash or a cancel leaves
//! the old file or none, never half of a new one. Integer formats round and
//! clip without dither; the float format stores every sample exactly.

use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::MediaError;

/// RIFF sizes are 32 bits: a file may not pass this many bytes of audio.
const MOST_DATA: u64 = u32::MAX as u64 - 64;

/// How a sample is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    I16,
    I24,
    F32,
}

impl Sample {
    fn bytes(self) -> u16 {
        match self {
            Sample::I16 => 2,
            Sample::I24 => 3,
            Sample::F32 => 4,
        }
    }

    /// The format tag: 1 for integer PCM, 3 for IEEE float.
    fn tag(self) -> u16 {
        match self {
            Sample::I16 | Sample::I24 => 1,
            Sample::F32 => 3,
        }
    }
}

/// The shape of a file to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub sample_rate: u32,
    /// One or two.
    pub channels: u16,
    pub sample: Sample,
}

/// The size of the header in front of the audio: `RIFF`, `fmt ` and the start
/// of `data`.
const HEADER: u64 = 44;

fn header(spec: Spec, data: u32) -> [u8; HEADER as usize] {
    let block = spec.channels * spec.sample.bytes();
    let mut out = Vec::with_capacity(HEADER as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(spec.sample.tag().to_le_bytes());
    out.extend(spec.channels.to_le_bytes());
    out.extend(spec.sample_rate.to_le_bytes());
    out.extend((spec.sample_rate * u32::from(block)).to_le_bytes());
    out.extend(block.to_le_bytes());
    out.extend((spec.sample.bytes() * 8).to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    out.try_into().expect("the header is 44 bytes")
}

/// A sample as a signed integer of `bits + 1` bits: scaled by 2^bits, as the
/// decoders read it back, and clipped to the range (+1.0 becomes the largest
/// value, one step below the scale).
fn whole(v: f32, bits: u32) -> i32 {
    let scale = f64::from(1_u32 << bits);
    (f64::from(v).clamp(-1.0, 1.0) * scale)
        .round()
        .clamp(-scale, scale - 1.0) as i32
}

fn io(error: std::io::Error) -> MediaError {
    MediaError::Io(error.to_string())
}

/// A file being written. Dropping it without calling [`Writer::finish`]
/// removes the partial file.
pub struct Writer {
    file: Option<BufWriter<File>>,
    temp: PathBuf,
    destination: PathBuf,
    spec: Spec,
    frames: u64,
}

impl Writer {
    /// Starts a file that [`finish`](Self::finish) will put at `destination`.
    pub fn create(destination: &Path, spec: Spec) -> Result<Self, MediaError> {
        if !(1..=2).contains(&spec.channels) {
            return Err(MediaError::Unsupported(format!(
                "{} channels (a WAV is written as mono or stereo)",
                spec.channels
            )));
        }
        if spec.sample_rate == 0 {
            return Err(MediaError::Unsupported("a sample rate of 0".to_owned()));
        }
        let name = destination
            .file_name()
            .ok_or_else(|| MediaError::Io("the path has no file name".to_owned()))?
            .to_string_lossy()
            .into_owned();
        let dir = destination
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temp = dir.join(format!(".{name}.{}.writing", std::process::id()));
        let mut file = BufWriter::new(File::create(&temp).map_err(io)?);
        let started = file.write_all(&header(spec, 0)).map_err(io);
        if let Err(error) = started {
            drop(file);
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        Ok(Self {
            file: Some(file),
            temp,
            destination: destination.to_owned(),
            spec,
            frames: 0,
        })
    }

    /// Appends a block: one slice of samples per channel, all the same length.
    pub fn write(&mut self, block: &[&[f32]]) -> Result<(), MediaError> {
        let frames = block.first().map_or(0, |c| c.len());
        if block.len() != usize::from(self.spec.channels) || block.iter().any(|c| c.len() != frames)
        {
            return Err(MediaError::Io(
                "a block must hold one equal slice per channel".to_owned(),
            ));
        }
        let bytes = u64::from(self.spec.channels) * u64::from(self.spec.sample.bytes());
        if (self.frames + frames as u64) * bytes > MOST_DATA {
            return Err(MediaError::Io(
                "the audio is too long for a WAV file".to_owned(),
            ));
        }
        let mut data = Vec::with_capacity(frames * bytes as usize);
        for i in 0..frames {
            for channel in block {
                let v = channel[i];
                match self.spec.sample {
                    Sample::I16 => data.extend(&whole(v, 15).to_le_bytes()[..2]),
                    Sample::I24 => data.extend(&whole(v, 23).to_le_bytes()[..3]),
                    Sample::F32 => data.extend(v.to_le_bytes()),
                }
            }
        }
        let file = self.file.as_mut().expect("the file is open until finish");
        file.write_all(&data).map_err(io)?;
        self.frames += frames as u64;
        Ok(())
    }

    /// Writes the sizes, flushes to disk and puts the file at its destination.
    /// Returns the number of frames written.
    pub fn finish(mut self) -> Result<u64, MediaError> {
        let data = (self.frames
            * u64::from(self.spec.channels)
            * u64::from(self.spec.sample.bytes())) as u32;
        let mut file = self
            .file
            .take()
            .expect("the file is open until finish")
            .into_inner()
            .map_err(|e| MediaError::Io(e.to_string()))?;
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        file.write_all(&header(self.spec, data)).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        fs::rename(&self.temp, &self.destination).map_err(io)?;
        sync_dir(&self.destination);
        Ok(self.frames)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.temp);
        }
    }
}

/// Makes the rename durable where a directory can be flushed; NTFS journals it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .and_then(|d| File::open(d).ok())
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-wav-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pattern(frames: usize) -> (Vec<f32>, Vec<f32>) {
        let left = (0..frames)
            .map(|i| ((i % 997) as f32 - 498.0) / 500.0)
            .collect();
        let right = (0..frames).map(|i| -((i % 31) as f32) / 40.0).collect();
        (left, right)
    }

    fn write(dir: &Path, name: &str, spec: Spec, blocks: &[usize]) -> (PathBuf, Vec<Vec<f32>>) {
        let total: usize = blocks.iter().sum();
        let (left, right) = pattern(total);
        let path = dir.join(name);
        let mut writer = Writer::create(&path, spec).unwrap();
        let mut at = 0;
        for &n in blocks {
            let l = &left[at..at + n];
            let r = &right[at..at + n];
            if spec.channels == 2 {
                writer.write(&[l, r]).unwrap();
            } else {
                writer.write(&[l]).unwrap();
            }
            at += n;
        }
        assert_eq!(writer.finish().unwrap(), total as u64);
        (path, vec![left, right])
    }

    #[test]
    fn float_files_read_back_exactly_in_any_block_split() {
        let dir = folder("float");
        let spec = Spec {
            sample_rate: 44_100,
            channels: 2,
            sample: Sample::F32,
        };
        let (path, wanted) = write(&dir, "a.wav", spec, &[1, 4_000, 255, 10_000]);
        let audio = crate::load(&path, 44_100, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.channel(0), &wanted[0][..]);
        assert_eq!(audio.channel(1), &wanted[1][..]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn integer_files_are_within_a_step_and_mono_has_one_channel() {
        let dir = folder("int");
        for (sample, step) in [
            (Sample::I16, 0.5 / 32_768.0 + 1e-7),
            (Sample::I24, 0.5 / 8_388_608.0 + 1e-7),
        ] {
            let spec = Spec {
                sample_rate: 48_000,
                channels: 1,
                sample,
            };
            let (path, wanted) = write(&dir, "m.wav", spec, &[3_000, 777]);
            let audio = crate::load(&path, 48_000, &AtomicBool::new(false), &mut |_| {}).unwrap();
            assert_eq!(audio.channels(), 1);
            assert_eq!(audio.frames(), 3_777);
            for (a, b) in audio.channel(0).iter().zip(&wanted[0]) {
                assert!((a - b).abs() <= step, "{sample:?}: {a} against {b}");
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn integer_files_clip_instead_of_wrapping() {
        let dir = folder("clip");
        let path = dir.join("c.wav");
        let spec = Spec {
            sample_rate: 48_000,
            channels: 1,
            sample: Sample::I16,
        };
        let mut writer = Writer::create(&path, spec).unwrap();
        writer
            .write(&[&[2.0, -2.0, f32::INFINITY, -f32::INFINITY]])
            .unwrap();
        writer.finish().unwrap();
        let audio = crate::load(&path, 48_000, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let top = 32_767.0 / 32_768.0;
        assert_eq!(audio.channel(0), &[top, -1.0, top, -1.0]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_is_not_there_until_it_is_finished_and_a_dropped_writer_leaves_nothing() {
        let dir = folder("atomic");
        let path = dir.join("x.wav");
        let spec = Spec {
            sample_rate: 48_000,
            channels: 1,
            sample: Sample::F32,
        };
        let mut writer = Writer::create(&path, spec).unwrap();
        writer.write(&[&[0.5; 100]]).unwrap();
        assert!(!path.exists());
        drop(writer);
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "no temporary file stays"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finishing_over_an_existing_file_replaces_it() {
        let dir = folder("replace");
        let path = dir.join("y.wav");
        fs::write(&path, b"old").unwrap();
        let spec = Spec {
            sample_rate: 48_000,
            channels: 1,
            sample: Sample::F32,
        };
        let mut writer = Writer::create(&path, spec).unwrap();
        writer.write(&[&[0.25; 10]]).unwrap();
        writer.finish().unwrap();
        let audio = crate::load(&path, 48_000, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(audio.frames(), 10);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_silent_empty_file_is_valid_and_bad_requests_are_refused() {
        let dir = folder("refuse");
        let spec = Spec {
            sample_rate: 48_000,
            channels: 1,
            sample: Sample::F32,
        };
        let path = dir.join("e.wav");
        Writer::create(&path, spec).unwrap().finish().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), HEADER);
        for bad in [
            Spec {
                channels: 0,
                ..spec
            },
            Spec {
                channels: 3,
                ..spec
            },
            Spec {
                sample_rate: 0,
                ..spec
            },
        ] {
            assert!(Writer::create(&dir.join("b.wav"), bad).is_err());
        }
        let mut writer = Writer::create(&dir.join("w.wav"), spec).unwrap();
        assert!(
            writer.write(&[&[0.0; 4], &[0.0; 4]]).is_err(),
            "too many channels"
        );
        let stereo = Spec {
            channels: 2,
            ..spec
        };
        let mut writer = Writer::create(&dir.join("s.wav"), stereo).unwrap();
        assert!(
            writer.write(&[&[0.0; 4], &[0.0; 3]]).is_err(),
            "unequal channels"
        );
        drop(writer);
        fs::remove_dir_all(dir).unwrap();
    }
}
