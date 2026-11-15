pub mod cache;
pub mod convert;
mod parse;

use std::fs::File;
use std::path::Path;

use crate::MediaError;

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
    pub fn bytes(self) -> usize {
        match self {
            Encoding::U8 | Encoding::I8 => 1,
            Encoding::I16Le | Encoding::I16Be => 2,
            Encoding::I24Le | Encoding::I24Be => 3,
            Encoding::I32Le | Encoding::I32Be | Encoding::F32Le | Encoding::F32Be => 4,
            Encoding::F64Le | Encoding::F64Be => 8,
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub sample_rate: u32,
    pub channels: u16,
    pub encoding: Encoding,
    pub data: u64,
    pub frames: u64,
}

impl Layout {
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

pub fn layout(path: &Path) -> Result<Option<Layout>, MediaError> {
    let mut file = File::open(path).map_err(io)?;
    let total = file.metadata().map_err(io)?.len();
    parse::layout(&mut file, total)
}

pub struct Reader {
    file: File,
    layout: Layout,
    bytes: Vec<u8>,
}

impl Reader {
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

const SCAN: usize = 65_536;

pub fn peaks(
    path: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<crate::Peaks>, MediaError> {
    let mut reader = Reader::open(path)?;
    let layout = *reader.layout();
    let mut growing = crate::GrowingPeaks::default();
    let mut blocks = vec![vec![0.0; SCAN]; layout.played()];
    let mut at = 0;
    while at < layout.frames {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(None);
        }
        let mut parts: Vec<&mut [f32]> = blocks.iter_mut().map(Vec::as_mut_slice).collect();
        let read = reader.read(at, &mut parts)?;
        if read == 0 {
            break;
        }
        growing.add(&blocks[0][..read], blocks.get(1).map(|r| &r[..read]));
        at += read as u64;
        progress(at as f32 / layout.frames as f32);
    }
    if layout.played() == 1 {
        growing.channels = 1;
    }
    Ok(Some(growing.finish()))
}

pub fn analysis(
    path: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<crate::detect::Analysis>, MediaError> {
    let mut reader = Reader::open(path)?;
    let layout = *reader.layout();
    crate::detect::analyse(
        |from, out| reader.read(from, out),
        layout.sample_rate,
        layout.frames,
        layout.played(),
        cancel,
        progress,
    )
}

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
