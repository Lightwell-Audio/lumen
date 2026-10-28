//! The cache of converted audio (ADR-0108; owner's C A). A file that does not
//! stream as it is gets a converted copy here: a 32-bit float WAV at the
//! device rate, named after a hash of its key, with the key itself beside it
//! in a small text file.
//!
//! The key is the source's full path, size, modified time and the rate. A
//! file that changes, by any of those, has a different key, so a stale copy
//! is never played: the new key misses and the file is converted again. A
//! copy is found only when its key file matches word for word, so a hash
//! collision is a miss, not a wrong file.
//!
//! The copy is written under a temporary name and renamed when complete
//! ([`crate::wav::Writer`]); the key file is written after it. A crash during
//! a conversion leaves no key, so the copy is made again.
//!
//! Waveform peaks are kept here too, for every stream file (a converted
//! copy or a source that streams as it is), so a file opened again shows
//! its waveform without being read whole ([`Cache::peaks`]).
//!
//! The folder has a size limit ([`LIMIT`]); [`Cache::trim`] clears the
//! entries used longest ago, never one that is playing. An entry is the
//! files that share a name before the first dot; a hit refreshes its
//! modified time, which is its time of last use.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Layout, convert, layout};
use crate::wav::{Sample, Spec, Writer};
use crate::{MediaError, Peaks};

/// How much the cache holds before [`Cache::trim`] clears it: 20 GB (the
/// owner's C A). Choosing it comes with R7's Preferences.
pub const LIMIT: u64 = 20_000_000_000;

/// Frames a reversal reads and writes at a time.
const BLOCK: usize = 65_536;

fn io(error: std::io::Error) -> MediaError {
    MediaError::Io(error.to_string())
}

/// What identifies one conversion of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    text: String,
    rate: u32,
}

impl Key {
    /// The key of `source` converted to `rate`, from the file as it is now.
    pub fn of(source: &Path, rate: u32) -> Result<Self, MediaError> {
        let meta = fs::metadata(source).map_err(io)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let path = fs::canonicalize(source).unwrap_or_else(|_| source.to_owned());
        Ok(Self {
            text: format!(
                "lumen stream cache 1\n{}\n{}\n{modified}\n{rate}\n",
                path.display(),
                meta.len()
            ),
            rate,
        })
    }

    /// The key of the waveform peaks of the same file.
    fn peaks(mut self) -> Self {
        self.text.push_str("peaks\n");
        self
    }

    /// The key of the reversed copy of the same file.
    fn reversed(mut self) -> Self {
        self.text.push_str("reversed\n");
        self
    }

    /// The copy's file name, without its extension.
    fn stem(&self) -> String {
        // FNV-1a: stable across runs and platforms, unlike the std hasher.
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in self.text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}-{}", self.rate)
    }
}

/// The stream file that plays a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub path: PathBuf,
    pub layout: Layout,
    /// Whether `path` is a converted copy (else it is the source itself).
    pub converted: bool,
}

/// The folder of converted copies.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn paths(&self, key: &Key) -> (PathBuf, PathBuf) {
        let stem = key.stem();
        (
            self.dir.join(format!("{stem}.wav")),
            self.dir.join(format!("{stem}.key")),
        )
    }

    /// The stream file for `source` at `rate` if one is ready: the source
    /// itself when it streams as it is, else a converted copy whose key
    /// matches. Never converts.
    pub fn find(&self, source: &Path, rate: u32) -> Result<Option<Prepared>, MediaError> {
        if let Some(found) = layout(source)?.filter(|l| l.sample_rate == rate) {
            return Ok(Some(Prepared {
                path: source.to_owned(),
                layout: found,
                converted: false,
            }));
        }
        let key = Key::of(source, rate)?;
        let (audio, key_file) = self.paths(&key);
        let matches = fs::read_to_string(&key_file).is_ok_and(|text| text == key.text);
        if !matches {
            return Ok(None);
        }
        match layout(&audio) {
            Ok(Some(found)) if found.sample_rate == rate => {
                used(&key_file);
                Ok(Some(Prepared {
                    path: audio,
                    layout: found,
                    converted: true,
                }))
            }
            // A copy that cannot be read is made again.
            _ => Ok(None),
        }
    }

    /// The stream file for `source` at `rate`, converting it now when there
    /// is none (slow: call on a background thread). `progress` gets 0 to 1.
    pub fn prepare(
        &self,
        source: &Path,
        rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32),
    ) -> Result<Prepared, MediaError> {
        if let Some(found) = self.find(source, rate)? {
            progress(1.0);
            return Ok(found);
        }
        let key = Key::of(source, rate)?;
        let (audio, key_file) = self.paths(&key);
        fs::create_dir_all(&self.dir).map_err(io)?;
        // An old key would vouch for the new copy before it is complete.
        let _ = fs::remove_file(&key_file);
        convert::convert(source, rate, &audio, cancel, progress)?;
        crate::write_new(&key_file, key.text.as_bytes())?;
        let found = layout(&audio)?
            .ok_or_else(|| MediaError::Io("the converted copy could not be read".into()))?;
        Ok(Prepared {
            path: audio,
            layout: found,
            converted: true,
        })
    }

    /// A stream file with the frames of the stream file `stream` in the other
    /// order (a reversed clip's audio), made now when there is none (slow:
    /// call on a background thread). Read and written in blocks, so neither
    /// file is held whole. Keyed as a conversion is, so a changed file is
    /// reversed again.
    pub fn reversed(&self, stream: &Path, cancel: &AtomicBool) -> Result<Prepared, MediaError> {
        let mut reader = super::Reader::open(stream)?;
        let source = *reader.layout();
        let key = Key::of(stream, source.sample_rate)?.reversed();
        let (audio, key_file) = self.paths(&key);
        if fs::read_to_string(&key_file).is_ok_and(|text| text == key.text)
            && let Ok(Some(found)) = layout(&audio)
            && found.frames == source.frames
        {
            used(&key_file);
            return Ok(Prepared {
                path: audio,
                layout: found,
                converted: true,
            });
        }
        fs::create_dir_all(&self.dir).map_err(io)?;
        let _ = fs::remove_file(&key_file);
        let channels = source.played();
        let mut writer = Writer::create_long(
            &audio,
            Spec {
                sample_rate: source.sample_rate,
                channels: channels as u16,
                sample: Sample::F32,
            },
        )?;
        let mut blocks = vec![vec![0.0; BLOCK]; channels];
        let mut end = source.frames;
        while end > 0 {
            crate::cancelled(cancel)?;
            let from = end.saturating_sub(BLOCK as u64);
            let count = (end - from) as usize;
            let mut parts: Vec<&mut [f32]> = blocks.iter_mut().map(|b| &mut b[..count]).collect();
            reader.read(from, &mut parts)?;
            for block in &mut blocks {
                block[..count].reverse();
            }
            let parts: Vec<&[f32]> = blocks.iter().map(|b| &b[..count]).collect();
            writer.write(&parts)?;
            end = from;
        }
        writer.finish()?;
        crate::write_new(&key_file, key.text.as_bytes())?;
        let found = layout(&audio)?
            .ok_or_else(|| MediaError::Io("the reversed copy could not be read".into()))?;
        Ok(Prepared {
            path: audio,
            layout: found,
            converted: true,
        })
    }

    /// The waveform peaks of the stream file `stream`: read from the cache
    /// when they are there for the file as it is now, else read from the
    /// file in blocks and kept for next time (slow: call on a background
    /// thread). `progress` gets 0 to 1; `None` if `cancel` was set. Failing
    /// to keep them is not an error: they are made again next time.
    pub fn peaks(
        &self,
        stream: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32),
    ) -> Result<Option<Peaks>, MediaError> {
        let rate = layout(stream)?.map_or(0, |l| l.sample_rate);
        let key = Key::of(stream, rate)?.peaks();
        let file = self.dir.join(format!("{}.peaks", key.stem()));
        if let Ok(bytes) = fs::read(&file)
            && let Some(peaks) = bytes
                .strip_prefix(key.text.as_bytes())
                .and_then(Peaks::from_bytes)
        {
            used(&file);
            progress(1.0);
            return Ok(Some(peaks));
        }
        let Some(peaks) = super::peaks(stream, cancel, progress)? else {
            return Ok(None);
        };
        let mut bytes = key.text.into_bytes();
        bytes.extend(peaks.to_bytes());
        if fs::create_dir_all(&self.dir).is_ok() {
            let _ = crate::write_new(&file, &bytes);
        }
        Ok(Some(peaks))
    }

    /// Clears the entries used longest ago until the folder holds at most
    /// `limit` bytes, never an entry of a stream file in `keep` (those
    /// playing now: their converted copy, and their peaks). Files being
    /// written are neither counted nor cleared. Returns the bytes cleared.
    pub fn trim(&self, limit: u64, keep: &[PathBuf]) -> u64 {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return 0;
        };
        // Entries by name: their files, bytes and time of last use.
        let mut entries: std::collections::HashMap<String, (Vec<PathBuf>, u64, SystemTime)> =
            Default::default();
        for file in read.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() || name.ends_with(".writing") {
                continue;
            }
            let stem = name.split('.').next().unwrap_or_default().to_owned();
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            let entry = entries
                .entry(stem)
                .or_insert_with(|| (Vec::new(), 0, UNIX_EPOCH));
            entry.0.push(file.path());
            entry.1 += meta.len();
            entry.2 = entry.2.max(modified);
        }
        let mut total: u64 = entries.values().map(|e| e.1).sum();
        if total <= limit {
            return 0;
        }
        let kept: std::collections::HashSet<String> =
            keep.iter().flat_map(|path| self.stems_of(path)).collect();
        let mut oldest: Vec<_> = entries
            .into_iter()
            .filter(|(stem, _)| !kept.contains(stem))
            .collect();
        oldest.sort_by_key(|(_, (_, _, used))| *used);
        let mut cleared = 0;
        for (_, (files, bytes, _)) in oldest {
            if total <= limit {
                break;
            }
            for file in files {
                let _ = fs::remove_file(file);
            }
            total -= bytes;
            cleared += bytes;
        }
        cleared
    }

    /// The entries that belong to the stream file `path`: its own, when it
    /// is a copy in this folder, and its peaks.
    fn stems_of(&self, path: &Path) -> Vec<String> {
        let mut stems = Vec::new();
        if path.parent() == Some(self.dir.as_path())
            && let Some(name) = path.file_name()
        {
            let name = name.to_string_lossy();
            stems.push(name.split('.').next().unwrap_or_default().to_owned());
        }
        if let Ok(Some(found)) = layout(path)
            && let Ok(key) = Key::of(path, found.sample_rate)
        {
            stems.push(key.peaks().stem());
        }
        stems
    }
}

/// Marks a cache file as used now (its modified time), so [`Cache::trim`]
/// clears it last. A file that cannot be touched only ages sooner.
fn used(file: &Path) {
    if let Ok(handle) = fs::OpenOptions::new().write(true).open(file) {
        let _ = handle.set_modified(SystemTime::now());
    }
}
