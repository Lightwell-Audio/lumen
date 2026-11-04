//! Row pictures for the browser's file tabs (the owner's R B, M9 R7c): a
//! small waveform of an audio file, the notes of a MIDI file, and what the
//! file's header says (length, rate, channels) for the row's accessible
//! name and the preview strip.
//!
//! A picture is a quick pass, not R6's peaks: an uncompressed file is read
//! at [`COLUMNS`] places, a short window each; a compressed one is decoded
//! whole only when it is at most [`DECODE_BYTES`] (a long song gets a flat
//! line). They are kept in the cache folder as [`Small::Thumb`] files, keyed
//! by the file's path, size and modified time.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::UNIX_EPOCH;

use lumen_media::stream::cache::{self, Cache, Small};

/// Columns in a waveform picture: a 46 pt row picture at 200%.
pub(crate) const COLUMNS: usize = 92;
/// Frames read at each column of an uncompressed file.
const WINDOW: usize = 512;
/// A compressed file is decoded for its picture only up to this size.
const DECODE_BYTES: u64 = 24_000_000;
/// Notes kept for a MIDI picture.
const MOST_NOTES: usize = 256;
/// The picture file's version.
const VERSION: u8 = 1;

/// A row's picture.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Picture {
    /// Each column's peak, 0 to 255.
    Wave(Vec<u8>),
    /// Notes as (start, length) across 0 to 1 and pitch across 0 to 1.
    Notes(Vec<(f32, f32, f32)>),
    /// No picture (a long compressed file): a flat line.
    Flat,
}

/// What a file's row knows of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Thumb {
    pub picture: Picture,
    pub seconds: Option<f64>,
    pub rate: Option<u32>,
    pub channels: Option<u16>,
}

impl Thumb {
    /// "0.9 s · 48 kHz mono", what is known.
    pub(crate) fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(seconds) = self.seconds {
            parts.push(length(seconds));
        }
        if let Some(rate) = self.rate {
            parts.push(format!("{} kHz", trim_rate(rate)));
        }
        if let Some(channels) = self.channels {
            parts.push(match channels {
                1 => "mono".to_owned(),
                2 => "stereo".to_owned(),
                n => format!("{n} channels"),
            });
        }
        parts.join(" \u{b7} ")
    }

    /// Said by a screen reader: "0.9 seconds, 48 kHz mono".
    pub(crate) fn said(&self) -> String {
        let mut words = Vec::new();
        if let Some(seconds) = self.seconds {
            words.push(format!("{} seconds", round_seconds(seconds)));
        }
        let mut format = Vec::new();
        if let Some(rate) = self.rate {
            format.push(format!("{} kHz", trim_rate(rate)));
        }
        match self.channels {
            Some(1) => format.push("mono".to_owned()),
            Some(2) => format.push("stereo".to_owned()),
            _ => {}
        }
        if !format.is_empty() {
            words.push(format.join(" "));
        }
        words.join(", ")
    }
}

fn round_seconds(seconds: f64) -> String {
    if seconds < 10.0 {
        format!("{seconds:.1}")
    } else {
        format!("{}", seconds.round())
    }
}

/// A length as a row shows it: "0.9 s", "12 s", "3:05".
pub(crate) fn length(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{} s", round_seconds(seconds))
    } else {
        let whole = seconds.round() as u64;
        format!("{}:{:02}", whole / 60, whole % 60)
    }
}

fn trim_rate(rate: u32) -> String {
    let khz = f64::from(rate) / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}")
    }
}

/// The cache name of `path`'s picture: its path, size and modified time.
fn key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(cache::hash(&format!(
        "lumen row picture {VERSION}\n{}\n{}\n{modified}\n",
        path.display(),
        meta.len()
    )))
}

/// The picture of `path`: from the cache when it has one, else made and
/// kept there.
pub(crate) fn thumb(
    path: &Path,
    cache: Option<&Cache>,
    cancel: &AtomicBool,
) -> Result<Thumb, String> {
    let key = key(path);
    if let (Some(cache), Some(key)) = (cache, &key)
        && let Some(bytes) = cache.load(Small::Thumb, key, COLUMNS as u32)
        && let Some(thumb) = decode(&bytes)
    {
        return Ok(thumb);
    }
    let thumb = make(path, cancel)?;
    if let (Some(cache), Some(key)) = (cache, &key) {
        let _ = cache.store(Small::Thumb, key, COLUMNS as u32, &encode(&thumb));
    }
    Ok(thumb)
}

/// Makes the picture of `path`.
pub(crate) fn make(path: &Path, cancel: &AtomicBool) -> Result<Thumb, String> {
    if crate::midi::files::is_midi_file(path) {
        return notes(path);
    }
    if let Some(layout) = lumen_media::stream::layout(path).map_err(|e| e.to_string())? {
        return sampled(path, &layout);
    }
    let header = lumen_media::probe(path).map_err(|e| e.to_string())?;
    let seconds = match (header.frames, header.sample_rate) {
        (Some(frames), Some(rate)) if rate > 0 => Some(frames as f64 / f64::from(rate)),
        _ => None,
    };
    let mut thumb = Thumb {
        picture: Picture::Flat,
        seconds,
        rate: header.sample_rate,
        channels: header.channels.and_then(|c| u16::try_from(c).ok()),
    };
    let size = std::fs::metadata(path).map_or(u64::MAX, |m| m.len());
    if size <= DECODE_BYTES {
        let (picture, frames, rate) = decoded(path, cancel)?;
        thumb.picture = picture;
        if thumb.seconds.is_none() && rate > 0 {
            thumb.seconds = Some(frames as f64 / f64::from(rate));
        }
        thumb.rate = thumb.rate.or(Some(rate));
    }
    Ok(thumb)
}

/// An uncompressed file, read at [`COLUMNS`] places.
fn sampled(path: &Path, layout: &lumen_media::stream::Layout) -> Result<Thumb, String> {
    let mut reader = lumen_media::stream::Reader::open(path).map_err(|e| e.to_string())?;
    let channels = layout.played();
    let mut planes = vec![vec![0.0_f32; WINDOW]; channels];
    let mut columns = Vec::with_capacity(COLUMNS);
    let frames = layout.frames;
    for column in 0..COLUMNS {
        let at = frames * column as u64 / COLUMNS as u64;
        let mut parts: Vec<&mut [f32]> = planes.iter_mut().map(|p| &mut p[..]).collect();
        reader.read(at, &mut parts).map_err(|e| e.to_string())?;
        let peak = planes
            .iter()
            .flat_map(|p| p.iter())
            .fold(0.0_f32, |m, s| m.max(s.abs()));
        columns.push(level(peak));
    }
    Ok(Thumb {
        picture: Picture::Wave(columns),
        seconds: (layout.sample_rate > 0).then(|| frames as f64 / f64::from(layout.sample_rate)),
        rate: Some(layout.sample_rate),
        channels: Some(layout.channels),
    })
}

/// A peak as a column's height, 0 to 255.
fn level(peak: f32) -> u8 {
    (peak.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A compressed file, decoded whole: the peak of each of [`COLUMNS`] equal
/// parts. Returns the picture, the frames and the rate.
fn decoded(path: &Path, cancel: &AtomicBool) -> Result<(Picture, u64, u32), String> {
    // Peaks of short spans, bucketed into columns once the length is known.
    const SPAN: usize = 256;
    let mut spans: Vec<f32> = Vec::new();
    let (mut current, mut in_span, mut frames, mut rate) = (0.0_f32, 0, 0_u64, 0);
    lumen_media::read_blocks(path, cancel, &mut |r, planes| {
        rate = r;
        let n = planes.first().map_or(0, |p| p.len());
        for i in 0..n {
            let peak = planes.iter().fold(0.0_f32, |m, p| m.max(p[i].abs()));
            current = current.max(peak);
            in_span += 1;
            if in_span == SPAN {
                spans.push(current);
                (current, in_span) = (0.0, 0);
            }
        }
        frames += n as u64;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    if in_span > 0 {
        spans.push(current);
    }
    let columns = (0..COLUMNS)
        .map(|c| {
            let from = spans.len() * c / COLUMNS;
            let to = (spans.len() * (c + 1) / COLUMNS)
                .max(from + 1)
                .min(spans.len());
            level(
                spans
                    .get(from..to)
                    .map_or(0.0, |s| s.iter().fold(0.0, |m, v| m.max(*v))),
            )
        })
        .collect();
    Ok((Picture::Wave(columns), frames, rate))
}

/// A MIDI file's notes, across its length and its range of pitches.
fn notes(path: &Path) -> Result<Thumb, String> {
    let file = crate::midi::files::read_file(path)?;
    let end = file.end().as_quarters().max(1.0) as f32;
    let all: Vec<_> = file
        .tracks
        .iter()
        .flat_map(|t| t.notes.iter().copied())
        .collect();
    let (low, high) = all.iter().fold((127_u8, 0_u8), |(lo, hi), n| {
        (lo.min(n.pitch()), hi.max(n.pitch()))
    });
    let span = f32::from(high.saturating_sub(low)).max(12.0);
    let notes = all
        .iter()
        .take(MOST_NOTES)
        .map(|n| {
            (
                n.start().as_quarters() as f32 / end,
                n.length().as_quarters() as f32 / end,
                f32::from(n.pitch().saturating_sub(low)) / span,
            )
        })
        .collect();
    let seconds = {
        // At the file's first tempo, or 120 BPM.
        let bpm = file.tempo.first().map_or(120.0, |t| t.bpm);
        Some(f64::from(end) * 60.0 / bpm)
    };
    Ok(Thumb {
        picture: Picture::Notes(notes),
        seconds,
        rate: None,
        channels: None,
    })
}

/// The picture file's bytes: a version, the header, then the picture.
fn encode(thumb: &Thumb) -> Vec<u8> {
    let mut out = vec![VERSION];
    out.extend_from_slice(&thumb.seconds.unwrap_or(-1.0).to_le_bytes());
    out.extend_from_slice(&thumb.rate.unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&thumb.channels.unwrap_or(0).to_le_bytes());
    match &thumb.picture {
        Picture::Flat => out.push(0),
        Picture::Wave(columns) => {
            out.push(1);
            out.extend_from_slice(&(columns.len() as u32).to_le_bytes());
            out.extend_from_slice(columns);
        }
        Picture::Notes(notes) => {
            out.push(2);
            out.extend_from_slice(&(notes.len() as u32).to_le_bytes());
            for (start, length, pitch) in notes {
                for v in [start, length, pitch] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    out
}

fn decode(bytes: &[u8]) -> Option<Thumb> {
    let mut at = 0;
    let mut take = |n: usize| -> Option<&[u8]> {
        let part = bytes.get(at..at + n)?;
        at += n;
        Some(part)
    };
    if take(1)? != [VERSION] {
        return None;
    }
    let seconds = f64::from_le_bytes(take(8)?.try_into().ok()?);
    let rate = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let channels = u16::from_le_bytes(take(2)?.try_into().ok()?);
    let picture = match take(1)?[0] {
        0 => Picture::Flat,
        1 => {
            let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
            Picture::Wave(take(n)?.to_vec())
        }
        2 => {
            let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
            let mut notes = Vec::with_capacity(n.min(MOST_NOTES));
            for _ in 0..n {
                let mut v = [0.0_f32; 3];
                for x in &mut v {
                    *x = f32::from_le_bytes(take(4)?.try_into().ok()?);
                }
                notes.push((v[0], v[1], v[2]));
            }
            Picture::Notes(notes)
        }
        _ => return None,
    };
    Some(Thumb {
        picture,
        seconds: (seconds >= 0.0).then_some(seconds),
        rate: (rate > 0).then_some(rate),
        channels: (channels > 0).then_some(channels),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_reads_back_as_it_was_written() {
        for picture in [
            Picture::Flat,
            Picture::Wave(vec![0, 128, 255]),
            Picture::Notes(vec![(0.0, 0.25, 0.5), (0.5, 0.5, 1.0)]),
        ] {
            let thumb = Thumb {
                picture,
                seconds: Some(1.5),
                rate: Some(44_100),
                channels: Some(2),
            };
            assert_eq!(decode(&encode(&thumb)), Some(thumb));
        }
        assert_eq!(decode(&[9, 9, 9]), None);
    }

    #[test]
    fn a_thumb_says_its_length_and_format() {
        let thumb = Thumb {
            picture: Picture::Flat,
            seconds: Some(0.94),
            rate: Some(48_000),
            channels: Some(1),
        };
        assert_eq!(thumb.summary(), "0.9 s \u{b7} 48 kHz \u{b7} mono");
        assert_eq!(thumb.said(), "0.9 seconds, 48 kHz mono");
        assert_eq!(length(185.0), "3:05");
        assert_eq!(trim_rate(44_100), "44.1");
    }
}
