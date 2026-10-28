//! Stream files (M9 slice R6): audio a disk thread reads at any frame without
//! decoding it.
//!
//! An uncompressed WAV, RF64 or AIFF file at the device rate is its own
//! stream file. Anything else (another rate, FLAC, MP3, Ogg) is converted
//! once into a 32-bit float WAV in the cache ([`cache`]), and that copy is
//! the stream file. Lumen plays the first two channels of any file, so a
//! converted copy keeps those two.
//!
//! A [`Reader`] reads with positioned reads, never a memory map: a mapped
//! page that is not resident is a blocking read wherever it is touched, and
//! the audio thread must never be that place (ADR-0108, S1). Samples become
//! `f32` by the same arithmetic the decoder uses, so a file streamed plays
//! exactly what the same file loaded into memory plays.
//!
//! Like the rest of this crate, everything here is work for a background
//! thread: it reads the disk and may allocate.

pub mod cache;
pub mod convert;
mod parse;

use std::fs::File;
use std::path::Path;

use crate::MediaError;

/// How samples are stored in a stream file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    U8,
    I8,
    I16Le,
    I16Be,
    I24Le,
    I24Be,
    I32Le,
    I32Be,
    F32Le,
    F32Be,
    F64Le,
    F64Be,
}

impl Encoding {
    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            Encoding::U8 | Encoding::I8 => 1,
            Encoding::I16Le | Encoding::I16Be => 2,
            Encoding::I24Le | Encoding::I24Be => 3,
            Encoding::I32Le | Encoding::I32Be | Encoding::F32Le | Encoding::F32Be => 4,
            Encoding::F64Le | Encoding::F64Be => 8,
        }
    }

    /// One sample from its bytes, scaled as the decoder scales it.
    #[inline]
    fn sample(self, b: &[u8]) -> f32 {
        match self {
            Encoding::U8 => f32::from(b[0]) / 128.0 - 1.0,
            Encoding::I8 => f32::from(b[0] as i8) / 128.0,
            Encoding::I16Le => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
            Encoding::I16Be => f32::from(i16::from_be_bytes([b[0], b[1]])) / 32_768.0,
            Encoding::I24Le => {
                (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
            }
            Encoding::I24Be => {
                (i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8) as f32 / 8_388_608.0
            }
            Encoding::I32Le => {
                (f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])) / 2_147_483_648.0) as f32
            }
            Encoding::I32Be => {
                (f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])) / 2_147_483_648.0) as f32
            }
            Encoding::F32Le => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Encoding::F32Be => f32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            Encoding::F64Le => f64::from_le_bytes(b[0..8].try_into().expect("eight bytes")) as f32,
            Encoding::F64Be => f64::from_be_bytes(b[0..8].try_into().expect("eight bytes")) as f32,
        }
    }
}

/// Where a stream file's audio is and how it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub sample_rate: u32,
    /// Channels in the file; Lumen plays the first two ([`Layout::played`]).
    pub channels: u16,
    pub encoding: Encoding,
    /// Byte offset of the first frame.
    pub data: u64,
    /// Whole frames the file holds.
    pub frames: u64,
}

impl Layout {
    /// Channels Lumen plays: one or two.
    pub fn played(&self) -> usize {
        usize::from(self.channels.min(2))
    }

    fn frame_bytes(&self) -> usize {
        usize::from(self.channels) * self.encoding.bytes()
    }
}

fn io(error: std::io::Error) -> MediaError {
    MediaError::Io(error.to_string())
}

/// The layout of `path`, or `None` when it is not a file a stream reads
/// directly (compressed, or not WAV or AIFF): such a file is converted.
pub fn layout(path: &Path) -> Result<Option<Layout>, MediaError> {
    let mut file = File::open(path).map_err(io)?;
    let total = file.metadata().map_err(io)?.len();
    parse::layout(&mut file, total)
}

/// Reads any range of frames of a stream file.
pub struct Reader {
    file: File,
    layout: Layout,
    /// Bytes read, kept between reads so a steady size allocates once.
    bytes: Vec<u8>,
}

impl Reader {
    /// Opens `path`; fails with [`MediaError::Unsupported`] when the file is
    /// not a stream file.
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let mut file = File::open(path).map_err(io)?;
        let total = file.metadata().map_err(io)?.len();
        let layout = parse::layout(&mut file, total)?.ok_or_else(|| {
            MediaError::Unsupported(
                "a compressed file, which is converted before it streams".into(),
            )
        })?;
        Ok(Self {
            file,
            layout,
            bytes: Vec::new(),
        })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Reads the frames from `from` into `out`, one slice per channel, all of
    /// one length: the file's first `out.len()` channels (at most
    /// [`Layout::channels`]). Frames past the end of the file are silence.
    /// Returns how many frames came from the file.
    pub fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<usize, MediaError> {
        let wanted = out.first().map_or(0, |c| c.len());
        if out.len() > usize::from(self.layout.channels) || out.iter().any(|c| c.len() != wanted) {
            return Err(MediaError::Io(
                "a read asks for one equal slice per channel the file has".to_owned(),
            ));
        }
        let available = self.layout.frames.saturating_sub(from).min(wanted as u64) as usize;
        let frame = self.layout.frame_bytes();
        let size = available * frame;
        if self.bytes.len() < size {
            self.bytes.resize(size, 0);
        }
        if size > 0 {
            let at = self.layout.data + from * frame as u64;
            read_at(&self.file, &mut self.bytes[..size], at)?;
        }
        let width = self.layout.encoding.bytes();
        let encoding = self.layout.encoding;
        for (c, channel) in out.iter_mut().enumerate() {
            let samples = self.bytes[..size].chunks_exact(frame);
            for (v, bytes) in channel.iter_mut().zip(samples) {
                *v = encoding.sample(&bytes[c * width..]);
            }
            channel[available..].fill(0.0);
        }
        Ok(available)
    }
}

/// Fills `buf` from `offset` without moving a shared cursor.
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> Result<(), MediaError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt as _;
        file.read_exact_at(buf, offset).map_err(io)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt as _;
        let mut done = 0;
        while done < buf.len() {
            match file.seek_read(&mut buf[done..], offset + done as u64) {
                Ok(0) => return Err(MediaError::Io("the file ended early".to_owned())),
                Ok(n) => done += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io(e)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
