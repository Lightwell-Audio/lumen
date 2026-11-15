use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Layout, convert, layout};
use crate::detect::Analysis;
use crate::wav::{Sample, Spec, Writer};
use crate::{MediaError, Peaks};

pub const LIMIT: u64 = 20_000_000_000;

pub const MARKER: &str = "lumen-cache.json";

pub const FRESH: Duration = Duration::from_secs(120);

static HITS: RwLock<()> = RwLock::new(());

pub fn is_cache_file(name: &str) -> bool {
    let Some((stem, extension)) = name.split_once('.') else {
        return false;
    };
    let Some((hash, rate)) = stem.split_once('-') else {
        return false;
    };
    hash.len() == 16
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && !rate.is_empty()
        && rate.bytes().all(|b| b.is_ascii_digit())
        && (matches!(extension, "wav" | "key" | "peaks" | "beats")
            || Small::ALL.iter().any(|k| k.extension() == extension))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Small {
    Index,
    Thumb,
}

impl Small {
    const ALL: [Small; 2] = [Small::Index, Small::Thumb];

    fn extension(self) -> &'static str {
        match self {
            Small::Index => "index",
            Small::Thumb => "thumb",
        }
    }
}

pub fn hash(text: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

const BLOCK: usize = 65_536;

fn io(error: std::io::Error) -> MediaError {
    MediaError::Io(error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    text: String,
    rate: u32,
}

impl Key {
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

    fn peaks(mut self) -> Self {
        self.text.push_str("peaks\n");
        self
    }

    fn analysis(mut self) -> Self {
        self.text.push_str(
            "analysis 1
",
        );
        self
    }

    fn reversed(mut self) -> Self {
        self.text.push_str("reversed\n");
        self
    }

    fn stem(&self) -> String {
        format!("{}-{}", hash(&self.text), self.rate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub path: PathBuf,
    pub layout: Layout,
    pub converted: bool,
}

#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
    limit: AtomicU64,
    fresh: Duration,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            limit: AtomicU64::new(LIMIT),
            fresh: FRESH,
        }
    }

    #[cfg(test)]
    pub(crate) fn fresh_for(mut self, fresh: Duration) -> Self {
        self.fresh = fresh;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn set_limit(&self, bytes: u64) {
        self.limit.store(bytes, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u64 {
        self.limit.load(Ordering::Relaxed)
    }

    fn ready(&self) -> Result<(), MediaError> {
        fs::create_dir_all(&self.dir).map_err(io)?;
        self.claim();
        Ok(())
    }

    fn claim(&self) -> bool {
        let marker = self.dir.join(MARKER);
        if marker.is_file() {
            return true;
        }
        let Ok(read) = fs::read_dir(&self.dir) else {
            return false;
        };
        let only_ours = read.flatten().all(|file| {
            let name = file.file_name().to_string_lossy().into_owned();
            file.file_type().is_ok_and(|t| t.is_file())
                && (is_cache_file(&name) || is_writing(&name))
        });
        if !only_ours {
            return false;
        }
        let text = "{\"what\": \"converted audio and waveform peaks\", \"version\": 1}\n";
        crate::write_new(&marker, text.as_bytes()).is_ok()
    }

    pub fn store(
        &self,
        kind: Small,
        hash: &str,
        variant: u32,
        bytes: &[u8],
    ) -> Result<(), MediaError> {
        self.ready()?;
        let path = self.small_path(kind, hash, variant);
        let mut part = path.clone().into_os_string();
        part.push(format!(".{}.writing", std::process::id()));
        let part = PathBuf::from(part);
        fs::write(&part, bytes).map_err(io)?;
        fs::rename(&part, &path).map_err(|e| {
            let _ = fs::remove_file(&part);
            io(e)
        })
    }

    pub fn load(&self, kind: Small, hash: &str, variant: u32) -> Option<Vec<u8>> {
        let path = self.small_path(kind, hash, variant);
        let bytes = fs::read(&path).ok()?;
        if let Ok(file) = fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(bytes)
    }

    fn small_path(&self, kind: Small, hash: &str, variant: u32) -> PathBuf {
        self.dir
            .join(format!("{hash}-{variant}.{}", kind.extension()))
    }

    fn paths(&self, key: &Key) -> (PathBuf, PathBuf) {
        let stem = key.stem();
        (
            self.dir.join(format!("{stem}.wav")),
            self.dir.join(format!("{stem}.key")),
        )
    }

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
        let _hit = HITS.read().unwrap_or_else(|e| e.into_inner());
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
            _ => Ok(None),
        }
    }

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
        self.ready()?;
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
        self.ready()?;
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
        if self.ready().is_ok() {
            let _ = crate::write_new(&file, &bytes);
        }
        Ok(Some(peaks))
    }

    pub fn analysis(
        &self,
        stream: &Path,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32),
    ) -> Result<Option<Analysis>, MediaError> {
        let rate = layout(stream)?.map_or(0, |l| l.sample_rate);
        let key = Key::of(stream, rate)?.analysis();
        let file = self.dir.join(format!("{}.beats", key.stem()));
        if let Ok(bytes) = fs::read(&file)
            && let Some(found) = bytes
                .strip_prefix(key.text.as_bytes())
                .and_then(Analysis::from_bytes)
        {
            used(&file);
            progress(1.0);
            return Ok(Some(found));
        }
        let Some(found) = super::analysis(stream, cancel, progress)? else {
            return Ok(None);
        };
        let mut bytes = key.text.into_bytes();
        bytes.extend(found.to_bytes());
        if self.ready().is_ok() {
            let _ = crate::write_new(&file, &bytes);
        }
        Ok(Some(found))
    }

    pub fn trim_to_limit(&self, keep: &[PathBuf]) -> u64 {
        self.trim(self.limit(), keep)
    }

    pub fn clear(&self, keep: &[PathBuf]) -> u64 {
        self.trim(0, keep)
    }

    pub fn size(&self) -> u64 {
        self.entries().values().map(|e| e.1).sum()
    }

    pub(super) fn entries(&self) -> std::collections::HashMap<String, Entry> {
        let mut entries: std::collections::HashMap<String, Entry> = Default::default();
        if !self.claim() {
            return entries;
        }
        let Ok(read) = fs::read_dir(&self.dir) else {
            return entries;
        };
        for file in read.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if !is_cache_file(&name) {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let stem = name.split('.').next().unwrap_or_default().to_owned();
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            let entry = entries
                .entry(stem)
                .or_insert_with(|| (Vec::new(), 0, UNIX_EPOCH));
            entry.0.push((file.path(), meta.len()));
            entry.1 += meta.len();
            entry.2 = entry.2.max(modified);
        }
        entries
    }

    pub fn trim(&self, limit: u64, keep: &[PathBuf]) -> u64 {
        self.trim_entries(self.entries(), limit, keep)
    }

    pub(super) fn trim_entries(
        &self,
        entries: std::collections::HashMap<String, Entry>,
        limit: u64,
        keep: &[PathBuf],
    ) -> u64 {
        let mut total: u64 = entries.values().map(|e| e.1).sum();
        if total <= limit {
            return 0;
        }
        let kept: std::collections::HashSet<String> =
            keep.iter().flat_map(|path| self.stems_of(path)).collect();
        let now = SystemTime::now();
        let fresh = |used: SystemTime| {
            now.duration_since(used)
                .map_or(true, |age| age < self.fresh)
        };
        let mut oldest: Vec<_> = entries
            .into_iter()
            .filter(|(stem, (_, _, used))| !kept.contains(stem) && !fresh(*used))
            .collect();
        oldest.sort_by_key(|(_, (_, _, used))| *used);
        let mut cleared = 0;
        for (_, (mut files, _, _)) in oldest {
            if total <= limit {
                break;
            }
            let _removing = HITS.write().unwrap_or_else(|e| e.into_inner());
            let used_now = files
                .iter()
                .filter_map(|(file, _)| fs::metadata(file).and_then(|m| m.modified()).ok())
                .max()
                .unwrap_or(UNIX_EPOCH);
            if fresh(used_now) {
                continue;
            }
            files.sort_by_key(|(file, _)| file.extension().is_some_and(|e| e == "key"));
            let mut whole = true;
            for (file, bytes) in files {
                let key = file.extension().is_some_and(|e| e == "key");
                if (whole || !key) && fs::remove_file(&file).is_ok() {
                    total -= bytes;
                    cleared += bytes;
                } else if !key {
                    whole = false;
                }
            }
        }
        cleared
    }

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
            stems.push(key.clone().peaks().stem());
            stems.push(key.analysis().stem());
        }
        stems
    }
}

fn is_writing(name: &str) -> bool {
    let Some(rest) = name.strip_suffix(".writing") else {
        return false;
    };
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    let Some((file, pid)) = rest.rsplit_once('.') else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|b| b.is_ascii_digit())
        && (is_cache_file(file)
            || is_cache_file(&format!("{file}.wav"))
            || MARKER.strip_suffix(".json") == Some(file))
}

pub(super) type Entry = (Vec<(PathBuf, u64)>, u64, SystemTime);

fn used(file: &Path) {
    if let Ok(handle) = fs::OpenOptions::new().write(true).open(file) {
        let _ = handle.set_modified(SystemTime::now());
    }
}
