//! Finding the audio in an uncompressed file: WAV (RIFF, and RF64 beyond
//! 4 GiB) and AIFF (AIFF, and AIFC without compression or with byte-swapped
//! or float samples). Pure: it reads headers from any `Read + Seek` and
//! touches no samples.

use std::io::{Read, Seek, SeekFrom};

use super::{Encoding, Layout};
use crate::MediaError;

/// The most channels a stream file may have.
const MOST_CHANNELS: u16 = 64;

fn io(error: std::io::Error) -> MediaError {
    MediaError::Io(error.to_string())
}

fn damaged(what: &str) -> MediaError {
    MediaError::Decode(what.to_owned())
}

/// The layout of the file `source` holds, `total` bytes long; `None` when it
/// is not a format a stream reads directly (compressed, or not WAV or AIFF).
pub(super) fn layout<R: Read + Seek>(
    source: &mut R,
    total: u64,
) -> Result<Option<Layout>, MediaError> {
    let mut head = [0_u8; 12];
    source.seek(SeekFrom::Start(0)).map_err(io)?;
    if source.read_exact(&mut head).is_err() {
        return Ok(None);
    }
    match (&head[0..4], &head[8..12]) {
        (b"RIFF" | b"RF64", b"WAVE") => wav(source, total, &head[0..4] == b"RF64"),
        (b"FORM", b"AIFF") => aiff(source, total, false),
        (b"FORM", b"AIFC") => aiff(source, total, true),
        _ => Ok(None),
    }
}

/// A chunk's id, the offset of its body and its declared size.
fn chunks<R: Read + Seek>(
    source: &mut R,
    total: u64,
    little: bool,
) -> Result<Vec<([u8; 4], u64, u64)>, MediaError> {
    let mut found = Vec::new();
    let mut at = 12_u64;
    while at + 8 <= total {
        let mut head = [0_u8; 8];
        source.seek(SeekFrom::Start(at)).map_err(io)?;
        source.read_exact(&mut head).map_err(io)?;
        let id: [u8; 4] = head[0..4].try_into().expect("four bytes");
        let raw: [u8; 4] = head[4..8].try_into().expect("four bytes");
        let size = u64::from(if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        });
        found.push((id, at + 8, size));
        // The audio is the last chunk that matters; a data chunk whose size
        // is unknown (a take still being written, or RF64) runs to the end.
        if (&id == b"data" || &id == b"SSND") && size == u64::from(u32::MAX) {
            break;
        }
        at = at + 8 + size + (size & 1);
    }
    Ok(found)
}

fn read_body<R: Read + Seek>(source: &mut R, at: u64, len: usize) -> Result<Vec<u8>, MediaError> {
    let mut body = vec![0_u8; len];
    source.seek(SeekFrom::Start(at)).map_err(io)?;
    source
        .read_exact(&mut body)
        .map_err(|_| damaged("a header chunk is cut short"))?;
    Ok(body)
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("four bytes"))
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("eight bytes"))
}

fn wav<R: Read + Seek>(
    source: &mut R,
    total: u64,
    rf64: bool,
) -> Result<Option<Layout>, MediaError> {
    let found = chunks(source, total, true)?;
    let mut big_data = None;
    if rf64 {
        let Some(&(_, at, size)) = found.iter().find(|c| &c.0 == b"ds64") else {
            return Err(damaged("an RF64 file without its ds64 chunk"));
        };
        if size < 24 {
            return Err(damaged("an RF64 ds64 chunk that is too short"));
        }
        big_data = Some(le64(&read_body(source, at, 24)?, 8));
    }
    let Some(&(_, fmt_at, fmt_size)) = found.iter().find(|c| &c.0 == b"fmt ") else {
        return Err(damaged("a WAV file without its format chunk"));
    };
    if fmt_size < 16 {
        return Err(damaged("a WAV format chunk that is too short"));
    }
    let fmt = read_body(source, fmt_at, fmt_size.min(40) as usize)?;
    let mut tag = le16(&fmt, 0);
    let channels = le16(&fmt, 2);
    let rate = le32(&fmt, 4);
    let block = le16(&fmt, 12);
    let bits = le16(&fmt, 14);
    if tag == 0xFFFE {
        if fmt.len() < 26 {
            return Err(damaged("an extensible WAV format chunk that is too short"));
        }
        // The sub-format's GUID starts with the format tag.
        tag = le16(&fmt, 24);
    }
    let container = block.checked_div(channels).unwrap_or(0);
    let encoding = match (tag, container, bits) {
        (1, 1, _) => Encoding::U8,
        (1, 2, _) => Encoding::I16Le,
        (1, 3, _) => Encoding::I24Le,
        (1, 4, _) => Encoding::I32Le,
        (3, 4, 32) => Encoding::F32Le,
        (3, 8, 64) => Encoding::F64Le,
        // Compressed (ADPCM, A-law and others): decoded and converted instead.
        _ => return Ok(None),
    };
    let Some(&(_, data, declared)) = found.iter().find(|c| &c.0 == b"data") else {
        return Err(damaged("a WAV file without its audio"));
    };
    let declared = match big_data {
        Some(size) if declared == u64::from(u32::MAX) => size,
        _ => declared,
    };
    finish(channels, rate, encoding, data, declared, total).map(Some)
}

/// An 80-bit extended-precision number (AIFF's sample rate) as a whole number.
fn extended(b: &[u8]) -> Option<u32> {
    let exponent = i32::from(u16::from_be_bytes([b[0], b[1]]) & 0x7FFF) - 16_383;
    let mantissa = u64::from_be_bytes(b[2..10].try_into().ok()?);
    if b[0] & 0x80 != 0 || !(0..=31).contains(&exponent) {
        return None;
    }
    let value = mantissa >> (63 - exponent);
    u32::try_from(value).ok()
}

fn aiff<R: Read + Seek>(
    source: &mut R,
    total: u64,
    compressed: bool,
) -> Result<Option<Layout>, MediaError> {
    let found = chunks(source, total, false)?;
    let Some(&(_, comm_at, comm_size)) = found.iter().find(|c| &c.0 == b"COMM") else {
        return Err(damaged("an AIFF file without its COMM chunk"));
    };
    let need = if compressed { 22 } else { 18 };
    if comm_size < need {
        return Err(damaged("an AIFF COMM chunk that is too short"));
    }
    let comm = read_body(source, comm_at, need as usize)?;
    let channels = u16::from_be_bytes([comm[0], comm[1]]);
    let frames = u64::from(u32::from_be_bytes(comm[2..6].try_into().expect("four")));
    let bits = u16::from_be_bytes([comm[6], comm[7]]);
    let rate = extended(&comm[8..18]).ok_or_else(|| damaged("an AIFF sample rate"))?;
    let kind: &[u8] = if compressed { &comm[18..22] } else { b"NONE" };
    let bytes = bits.div_ceil(8);
    let encoding = match (kind, bytes) {
        (b"NONE", 1) => Encoding::I8,
        (b"NONE", 2) => Encoding::I16Be,
        (b"NONE", 3) => Encoding::I24Be,
        (b"NONE", 4) => Encoding::I32Be,
        (b"sowt", 2) => Encoding::I16Le,
        (b"sowt", 3) => Encoding::I24Le,
        (b"sowt", 4) => Encoding::I32Le,
        (b"fl32" | b"FL32", _) => Encoding::F32Be,
        (b"fl64" | b"FL64", _) => Encoding::F64Be,
        _ => return Ok(None),
    };
    let Some(&(_, ssnd_at, ssnd_size)) = found.iter().find(|c| &c.0 == b"SSND") else {
        return Err(damaged("an AIFF file without its audio"));
    };
    let ssnd = read_body(source, ssnd_at, 8)?;
    let offset = u64::from(u32::from_be_bytes(ssnd[0..4].try_into().expect("four")));
    let data = ssnd_at + 8 + offset;
    let declared = (frames * u64::from(channels) * encoding.bytes() as u64)
        .min(ssnd_size.saturating_sub(8 + offset));
    finish(channels, rate, encoding, data, declared, total).map(Some)
}

fn finish(
    channels: u16,
    rate: u32,
    encoding: Encoding,
    data: u64,
    declared: u64,
    total: u64,
) -> Result<Layout, MediaError> {
    if channels == 0 || channels > MOST_CHANNELS {
        return Err(damaged(&format!("{channels} channels")));
    }
    if rate == 0 {
        return Err(damaged("a sample rate of 0"));
    }
    let available = total.saturating_sub(data);
    // A take still being written names `u32::MAX`: everything to the end.
    let bytes = if declared == u64::from(u32::MAX) {
        available
    } else {
        declared.min(available)
    };
    let frame = u64::from(channels) * encoding.bytes() as u64;
    Ok(Layout {
        sample_rate: rate,
        channels,
        encoding,
        data,
        frames: bytes / frame,
    })
}
