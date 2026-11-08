//! The tempo a file says it was made at (M9.5 S4d, W A): the loop tag some
//! WAV files carry, the BPM tag of other formats, and a BPM in the file's
//! name. Detection from the audio is S5a's.
//!
//! Quick work (a header or a tag, never the audio), but it opens the file:
//! for a background thread.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, RawValue, StandardTag};

/// The tempos a file may claim: Lumen's own limits.
const SLOWEST: f64 = 20.0;
const FASTEST: f64 = 999.0;

/// A tempo a file states.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileTempo {
    pub bpm: f64,
    /// The file says it is a one-shot, not a loop (the loop tag of a WAV
    /// file has a flag for it).
    pub one_shot: bool,
}

/// The tempo `path` states in itself: the loop tag of a WAV file, or a BPM tag.
pub fn in_file(path: &Path) -> Option<FileTempo> {
    find(path, false)
}

/// The tempo `path` states: [`in_file`], then a BPM in the file's name. A WAV
/// file's loop tag that calls it a one-shot keeps saying so whichever of them
/// the tempo comes from.
pub fn stated(path: &Path) -> Option<FileTempo> {
    find(path, true)
}

fn find(path: &Path, by_name: bool) -> Option<FileTempo> {
    let wav = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("wave"));
    let acid = if wav { loop_tag(path) } else { None };
    let one_shot = acid.is_some_and(|a| a.one_shot);
    let bpm = acid.and_then(|a| a.bpm).or_else(|| tag(path)).or_else(|| {
        by_name
            .then(|| path.file_stem()?.to_str().and_then(in_name))
            .flatten()
    });
    bpm.map(|bpm| FileTempo { bpm, one_shot })
}

/// A tempo written in a file's name: "120 BPM", "bpm_92.5", "Funk-98bpm".
/// Only a number beside the letters `bpm` counts, so "808", "Kick 01" and
/// "Take 120" state nothing.
pub fn in_name(name: &str) -> Option<f64> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(found) = lower[from..].find("bpm") {
        let at = from + found;
        from = at + 3;
        // The number before: "120bpm", "120 bpm", "120_bpm".
        let mut end = at;
        while end > 0 && matches!(bytes[end - 1], b' ' | b'_' | b'-') {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
            start -= 1;
        }
        if let Some(bpm) = number(&lower[start..end]) {
            return Some(bpm);
        }
        // The number after: "bpm120", "bpm_120", "bpm 92.5".
        let mut start = at + 3;
        while start < bytes.len() && matches!(bytes[start], b' ' | b'_' | b'-') {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        if let Some(bpm) = number(&lower[start..end]) {
            return Some(bpm);
        }
    }
    None
}

fn number(text: &str) -> Option<f64> {
    let bpm: f64 = text.trim_matches('.').parse().ok()?;
    (SLOWEST..=FASTEST).contains(&bpm).then_some(bpm)
}

/// What the loop chunk of a WAV file says.
#[derive(Clone, Copy)]
struct LoopTag {
    /// The tempo, when it is one Lumen can use.
    bpm: Option<f64>,
    one_shot: bool,
}

/// The tempo in the loop chunk ("acid") of a WAV file: a flag word (bit 0:
/// one-shot), the root note, two reserved words, the beat count, the meter,
/// and the tempo as a 32-bit float.
fn loop_tag(path: &Path) -> Option<LoopTag> {
    let mut file = File::open(path).ok()?;
    let mut head = [0_u8; 12];
    file.read_exact(&mut head).ok()?;
    if &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
        return None;
    }
    // Chunks are a tag, a length and the data, padded to an even length. The
    // sound is skipped over, so a long file costs a seek.
    for _ in 0..64 {
        let mut chunk = [0_u8; 8];
        file.read_exact(&mut chunk).ok()?;
        let length = u64::from(u32::from_le_bytes(chunk[4..].try_into().ok()?));
        if &chunk[..4] == b"acid" && length >= 24 {
            let mut data = [0_u8; 24];
            file.read_exact(&mut data).ok()?;
            let flags = u32::from_le_bytes(data[..4].try_into().ok()?);
            let bpm = f64::from(f32::from_le_bytes(data[20..24].try_into().ok()?));
            // A tempo outside the limits (or not a number) is no tempo, but
            // the flag is still the file's.
            return Some(LoopTag {
                bpm: (SLOWEST..=FASTEST).contains(&bpm).then_some(bpm),
                one_shot: flags & 1 != 0,
            });
        }
        file.seek(SeekFrom::Current((length + (length & 1)) as i64))
            .ok()?;
    }
    None
}

/// The BPM tag of a file: ID3's, Vorbis comments', and the like.
fn tag(path: &Path) -> Option<f64> {
    let file = File::open(path).ok()?;
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
        .ok()?;
    let log = format.metadata();
    let revision = log.current()?;
    revision.media.tags.iter().find_map(|tag| {
        if let Some(StandardTag::Bpm(bpm)) = &tag.std {
            return number(&bpm.to_string());
        }
        let key = tag.raw.key.to_ascii_lowercase();
        if key != "bpm" && key != "tbpm" && key != "tempo" {
            return None;
        }
        match &tag.raw.value {
            RawValue::Float(v) => (SLOWEST..=FASTEST).contains(v).then_some(*v),
            RawValue::SignedInt(v) => number(&v.to_string()),
            RawValue::String(text) => number(text.trim()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests;
