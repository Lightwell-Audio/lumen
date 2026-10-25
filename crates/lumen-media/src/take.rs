//! Files for recorded takes (M9 slice R2, ADR-0102): a 32-bit float WAV that
//! is valid at every moment.
//!
//! [`wav::Writer`] hides its file until it is finished, which is right for a
//! render and wrong for a recording: a crash would lose the whole take. A
//! [`TakeWriter`] writes the real file from the start. Its header always names
//! a length that the file holds: the header is refreshed by
//! [`TakeWriter::refresh`] (the recording worker calls it about once a
//! second) and by [`TakeWriter::finish`]. Audio written since the last refresh
//! sits after the end the header names, where readers ignore it.
//! [`repair`] puts a file that was cut short right: it counts the whole frames
//! the file holds, drops a half frame, and writes the header again.
//!
//! # Long takes
//!
//! A RIFF length has 32 bits (4 GiB: about three hours of stereo at 48 kHz).
//! Past that the header carries the *streaming* length, `u32::MAX`, which
//! readers take to mean "to the end of the file". The decoder Lumen plays
//! with reads such a file in full; RF64 it does not read at all, so it is not
//! used. A file that is shorter when finished gets its exact lengths.
//!
//! Dropping a writer leaves the file where it is, as a crash would: the
//! recovery pass owns what to do with an unfinished take.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::MediaError;
use crate::wav::{HEADER, Sample, Spec, header, io};

/// The audio a 32-bit RIFF length can name.
const MOST_DATA: u64 = u32::MAX as u64 - 64;
/// The length readers take to mean "until the end of the file".
const STREAMING: u32 = u32::MAX;

/// The length a header should name for `bytes` of audio.
fn named(bytes: u64, most: u64) -> u32 {
    if bytes > most {
        STREAMING
    } else {
        bytes as u32
    }
}

/// A recording's file.
pub struct TakeWriter {
    file: BufWriter<File>,
    path: PathBuf,
    spec: Spec,
    frames: u64,
    /// Interleaved bytes of one block, kept between calls.
    scratch: Vec<u8>,
    /// Audio past this many bytes is named by the streaming length.
    most: u64,
}

impl TakeWriter {
    /// Starts the file at `path` (replacing a file already there): a valid,
    /// empty WAV of 32-bit float, mono or stereo.
    pub fn create(path: &Path, rate: u32, channels: u16) -> Result<Self, MediaError> {
        let spec = Spec {
            sample_rate: rate,
            channels,
            sample: Sample::F32,
        };
        if !(1..=2).contains(&channels) {
            return Err(MediaError::Unsupported(format!(
                "{channels} channels (a take is written as mono or stereo)"
            )));
        }
        if rate == 0 {
            return Err(MediaError::Unsupported("a sample rate of 0".to_owned()));
        }
        let mut file = BufWriter::new(File::create(path).map_err(io)?);
        file.write_all(&header(spec, 0)).map_err(io)?;
        file.flush().map_err(io)?;
        Ok(Self {
            file,
            path: path.to_owned(),
            spec,
            frames: 0,
            scratch: Vec::new(),
            most: MOST_DATA,
        })
    }

    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Frames written so far (some perhaps not yet named by the header).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    fn block_bytes(&self) -> u64 {
        u64::from(self.spec.channels) * 4
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
        self.scratch.clear();
        for i in 0..frames {
            for channel in block {
                self.scratch.extend(channel[i].to_le_bytes());
            }
        }
        self.file.write_all(&self.scratch).map_err(io)?;
        self.frames += frames as u64;
        Ok(())
    }

    /// Appends `frames` frames of silence (a gap the recorder could not fill).
    pub fn write_silence(&mut self, frames: usize) -> Result<(), MediaError> {
        const CHUNK: usize = 4_096;
        let zeros = [0.0_f32; CHUNK];
        let mut left = frames;
        while left > 0 {
            let n = left.min(CHUNK);
            let slices = [&zeros[..n], &zeros[..n]];
            self.write(&slices[..usize::from(self.spec.channels)])?;
            left -= n;
        }
        Ok(())
    }

    /// Flushes what has been written and makes the header name all of it, so
    /// a crash from here on loses nothing up to now.
    pub fn refresh(&mut self) -> Result<(), MediaError> {
        self.file.flush().map_err(io)?;
        let data = named(self.frames * self.block_bytes(), self.most);
        let file = self.file.get_mut();
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        file.write_all(&header(self.spec, data)).map_err(io)?;
        file.seek(SeekFrom::End(0)).map_err(io)?;
        Ok(())
    }

    /// Ends the take: the header names every frame and the file is on disk.
    /// Returns the frames written.
    pub fn finish(mut self) -> Result<u64, MediaError> {
        self.refresh()?;
        self.file.get_ref().sync_all().map_err(io)?;
        Ok(self.frames)
    }
}

/// What [`repair`] found in a take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repaired {
    pub sample_rate: u32,
    pub channels: u16,
    /// The whole frames the file holds.
    pub frames: u64,
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// Makes a take that was cut short (a crash, a power cut) a whole file: the
/// header is read, a half frame at the end is cut off, and the header is
/// written again to name every frame left. A finished file is left as it was.
///
/// Fails with [`MediaError::Unsupported`] when the file is not one of ours
/// (a 44-byte float header) and with [`MediaError::Io`] when it cannot be read
/// or written.
pub fn repair(path: &Path) -> Result<Repaired, MediaError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(io)?;
    let mut head = [0_u8; HEADER as usize];
    file.read_exact(&mut head)
        .map_err(|_| MediaError::Unsupported("the file is shorter than a WAV header".to_owned()))?;
    let ours = &head[0..4] == b"RIFF"
        && &head[8..16] == b"WAVEfmt "
        && le32(&head, 16) == 16
        && le16(&head, 20) == 3
        && le16(&head, 34) == 32
        && &head[36..40] == b"data";
    let channels = le16(&head, 22);
    let sample_rate = le32(&head, 24);
    if !ours || !(1..=2).contains(&channels) || sample_rate == 0 {
        return Err(MediaError::Unsupported(
            "the file is not a take Lumen wrote".to_owned(),
        ));
    }
    let length = file.metadata().map_err(io)?.len();
    let block = u64::from(channels) * 4;
    let frames = (length - HEADER) / block;
    let bytes = frames * block;
    if length - HEADER != bytes {
        file.set_len(HEADER + bytes).map_err(io)?;
    }
    let spec = Spec {
        sample_rate,
        channels,
        sample: Sample::F32,
    };
    let wanted = header(spec, named(bytes, MOST_DATA));
    if wanted != head {
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        file.write_all(&wanted).map_err(io)?;
    }
    file.sync_all().map_err(io)?;
    Ok(Repaired {
        sample_rate,
        channels,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-take-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ramp(from: usize, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let left = (from..from + frames).map(|i| i as f32 / 4_096.0).collect();
        let right = (from..from + frames)
            .map(|i| -(i as f32) / 8_192.0)
            .collect();
        (left, right)
    }

    fn try_read(path: &Path) -> Result<crate::Audio, MediaError> {
        crate::load(path, 48_000, &AtomicBool::new(false), &mut |_| {})
    }

    fn read(path: &Path) -> crate::Audio {
        try_read(path).unwrap()
    }

    /// A file with no audio decodes as `Empty`: valid, and nothing to play.
    fn is_empty(path: &Path) -> bool {
        matches!(try_read(path), Err(MediaError::Empty))
    }

    fn stereo(path: &Path) -> TakeWriter {
        TakeWriter::create(path, 48_000, 2).unwrap()
    }

    #[test]
    fn a_new_take_is_a_valid_empty_file_and_a_finished_one_reads_back_exactly() {
        let dir = folder("basic");
        let path = dir.join("t.wav");
        let mut writer = stereo(&path);
        assert!(is_empty(&path), "empty at the start");
        let (l, r) = ramp(0, 1_000);
        writer.write(&[&l[..300], &r[..300]]).unwrap();
        writer.write(&[&l[300..], &r[300..]]).unwrap();
        assert_eq!(writer.finish().unwrap(), 1_000);
        let audio = read(&path);
        assert_eq!(audio.frames(), 1_000);
        assert_eq!(audio.channel(0), &l[..]);
        assert_eq!(audio.channel(1), &r[..]);
        assert_eq!(fs::metadata(&path).unwrap().len(), HEADER + 8_000);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_file_is_valid_after_every_refresh_while_the_take_goes_on() {
        let dir = folder("refresh");
        let path = dir.join("t.wav");
        let mut writer = stereo(&path);
        let (l, r) = ramp(0, 900);
        for step in 0..3 {
            let part = step * 300..(step + 1) * 300;
            writer.write(&[&l[part.clone()], &r[part]]).unwrap();
            writer.refresh().unwrap();
            let audio = read(&path);
            assert_eq!(audio.frames(), (step + 1) * 300, "after refresh {step}");
            assert_eq!(audio.channel(0), &l[..(step + 1) * 300]);
        }
        // Written after the last refresh, the audio is past the header's end.
        writer.write(&[&l[..50], &r[..50]]).unwrap();
        drop(writer);
        assert_eq!(read(&path).frames(), 900, "the header still names 900");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_cut_anywhere_is_repaired_to_the_whole_frames_it_holds() {
        let dir = folder("cut");
        let path = dir.join("t.wav");
        let mut writer = stereo(&path);
        let (l, r) = ramp(0, 40);
        writer.write(&[&l, &r]).unwrap();
        writer.finish().unwrap();
        let whole = fs::read(&path).unwrap();
        for length in 0..=whole.len() {
            let cut = dir.join("cut.wav");
            fs::write(&cut, &whole[..length]).unwrap();
            let result = repair(&cut);
            if (length as u64) < HEADER {
                assert!(result.is_err(), "{length} bytes is no file");
                continue;
            }
            let frames = (length as u64 - HEADER) / 8;
            let found = result.unwrap();
            assert_eq!(found.frames, frames, "cut at {length}");
            assert_eq!(found.channels, 2);
            if frames == 0 {
                assert!(is_empty(&cut), "cut at {length}");
            } else {
                let audio = read(&cut);
                assert_eq!(audio.frames() as u64, frames, "cut at {length}");
                assert_eq!(audio.channel(0), &l[..frames as usize]);
                assert_eq!(audio.channel(1), &r[..frames as usize]);
            }
            assert_eq!(
                fs::metadata(&cut).unwrap().len(),
                HEADER + frames * 8,
                "a half frame is cut off at {length}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_crash_before_any_refresh_loses_nothing_that_reached_the_disk() {
        let dir = folder("crash");
        let path = dir.join("t.wav");
        let mut writer = stereo(&path);
        let (l, r) = ramp(0, 5_000);
        writer.write(&[&l, &r]).unwrap();
        // The process dies: no refresh, no finish. The buffer's contents are
        // lost with it, so flush it as the operating system would have by now.
        writer.file.flush().unwrap();
        std::mem::forget(writer);
        assert!(is_empty(&path), "the header says 0");
        let found = repair(&path).unwrap();
        assert_eq!(found.frames, 5_000);
        assert_eq!(read(&path).channel(1), &r[..]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repairing_a_finished_file_changes_nothing() {
        let dir = folder("whole");
        let path = dir.join("t.wav");
        let mut writer = TakeWriter::create(&path, 44_100, 1).unwrap();
        writer.write(&[&[0.5; 123]]).unwrap();
        writer.finish().unwrap();
        let before = fs::read(&path).unwrap();
        let found = repair(&path).unwrap();
        assert_eq!(
            found,
            Repaired {
                sample_rate: 44_100,
                channels: 1,
                frames: 123
            }
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn past_the_riff_limit_the_header_names_the_streaming_length_and_the_file_reads_in_full() {
        let dir = folder("long");
        let path = dir.join("t.wav");
        let mut writer = stereo(&path);
        // A small limit stands in for 4 GiB.
        writer.most = 1_000;
        let (l, r) = ramp(0, 400);
        writer.write(&[&l, &r]).unwrap();
        writer.refresh().unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(le32(&bytes, 40), STREAMING, "data length is the marker");
        assert_eq!(le32(&bytes, 4), STREAMING, "so is the RIFF length");
        let audio = read(&path);
        assert_eq!(audio.frames(), 400);
        assert_eq!(audio.channel(0), &l[..]);
        writer.finish().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn silence_fills_a_gap_in_either_channel_count() {
        let dir = folder("gap");
        for channels in [1_u16, 2] {
            let path = dir.join(format!("g{channels}.wav"));
            let mut writer = TakeWriter::create(&path, 48_000, channels).unwrap();
            let one = [1.0_f32; 3];
            if channels == 2 {
                writer.write(&[&one, &one]).unwrap();
            } else {
                writer.write(&[&one]).unwrap();
            }
            writer.write_silence(10_000).unwrap();
            assert_eq!(writer.finish().unwrap(), 10_003);
            let audio = read(&path);
            assert_eq!(audio.frames(), 10_003);
            assert_eq!(audio.channel(0)[2], 1.0);
            assert!(audio.channel(0)[3..].iter().all(|&s| s == 0.0));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_requests_and_foreign_files_are_refused() {
        let dir = folder("refuse");
        for (rate, channels) in [(48_000, 0), (48_000, 3), (0, 1)] {
            assert!(TakeWriter::create(&dir.join("b.wav"), rate, channels).is_err());
        }
        let mut writer = stereo(&dir.join("s.wav"));
        assert!(writer.write(&[&[0.0; 4]]).is_err(), "one channel of two");
        assert!(
            writer.write(&[&[0.0; 4], &[0.0; 3]]).is_err(),
            "unequal channels"
        );
        drop(writer);
        // A 16-bit file written by the render writer is not a take.
        let other = dir.join("o.wav");
        let mut render = crate::wav::Writer::create(
            &other,
            Spec {
                sample_rate: 48_000,
                channels: 1,
                sample: Sample::I16,
            },
        )
        .unwrap();
        render.write(&[&[0.1; 10]]).unwrap();
        render.finish().unwrap();
        assert!(matches!(repair(&other), Err(MediaError::Unsupported(_))));
        fs::write(dir.join("junk.wav"), vec![7_u8; 500]).unwrap();
        assert!(matches!(
            repair(&dir.join("junk.wav")),
            Err(MediaError::Unsupported(_))
        ));
        fs::remove_dir_all(dir).unwrap();
    }
}
