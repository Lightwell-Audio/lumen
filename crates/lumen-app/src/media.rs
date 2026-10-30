//! Audio files in the app (M2 slice 6): each one is loaded on its own
//! background thread (decode, convert to the device rate, then build its
//! waveform peaks, slice 4c) with progress and cancel, and held in memory
//! for the clip player. When the device rate
//! changes, ready files are converted again in the background; their clips
//! stay silent until then. With "Play audio from disk" on (M9 slice R6),
//! files of half a minute or more play from disk instead ([`streaming`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use lumen_engine::{ClipAudio, StreamAudio};
use lumen_media::{MediaError, Peaks};

pub mod streaming;
pub mod streams;
mod twins;
pub use twins::{mirror, reversed_key};

/// What the user sees about a file: from its name and size at once, the
/// rest when it has been decoded.
#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    /// "WAV", "FLAC", ...
    pub format: String,
    pub size_bytes: u64,
    /// The file's own rate, before conversion.
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
    pub seconds: Option<f64>,
    /// Length in the file's own frames.
    pub frames: Option<u64>,
}

/// Every file extension Lumen imports, for file dialogs' filters.
pub const EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg"];

/// The format name shown for `path`, or `None` if Lumen cannot import it.
pub fn format_name(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "wav" | "wave" => "WAV",
        "aif" | "aiff" => "AIFF",
        "flac" => "FLAC",
        "mp3" => "MP3",
        "ogg" => "Ogg Vorbis",
        _ => return None,
    })
}

pub fn is_audio_file(path: &Path) -> bool {
    format_name(path).is_some()
}

#[derive(Debug, Clone)]
pub enum State {
    Loading,
    Ready(Arc<ClipAudio>),
    Failed(String),
}

pub struct Item {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub state: State,
    pub info: FileInfo,
    /// Waveform peaks of the audio at the device rate, once ready.
    pub peaks: Option<Arc<Peaks>>,
    job: Option<Job>,
    /// Whether streaming was on when the file loaded: it loads again when
    /// that changes. `None` for audio given ready, which never loads.
    streamed: Option<bool>,
}

impl Drop for Item {
    fn drop(&mut self) {
        streaming::release(&self.state);
    }
}

struct Job {
    cancel: Arc<AtomicBool>,
    /// Progress 0.0–1.0, as `f32` bits.
    progress: Arc<AtomicU32>,
    rate: u32,
    /// Streaming was on when it started.
    stream: bool,
}

struct Done {
    id: u64,
    rate: u32,
    stream: bool,
    /// The file's own rate, channels and length (frames, seconds).
    source: Option<Source>,
    /// The file's size on disk, read here rather than on the UI thread
    /// (M4 review L8: a stat on a network path can stall a frame).
    size_bytes: u64,
    result: Result<streaming::Loaded, MediaError>,
    peaks: Option<Peaks>,
}

pub struct Library {
    items: Vec<Item>,
    /// The page pool, disk thread and cache while streaming is on.
    streaming: Option<streaming::Streaming>,
    /// Reversed copies of files, by `twins::reversed_key` (M7b slice E1).
    twins: Vec<twins::Twin>,
    twin_tx: Sender<twins::Mirrored>,
    twin_rx: Receiver<twins::Mirrored>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
}

impl Default for Library {
    fn default() -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        let (twin_tx, twin_rx) = mpsc::channel();
        Self {
            items: Vec::new(),
            streaming: None,
            twins: Vec::new(),
            twin_tx,
            twin_rx,
            done_tx,
            done_rx,
        }
    }
}

impl Library {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// A file by media id, or its reversed twin by `reversed_key`.
    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items
            .iter()
            .find(|i| i.id == id)
            .or_else(|| self.twins.iter().map(|t| &t.item).find(|i| i.id == id))
    }

    /// Every file and every reversed twin, for the waveforms' atlas.
    pub fn with_twins(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().chain(self.twins.iter().map(|t| &t.item))
    }

    /// Starts loading `path` at `rate` as `id` (the project's media id).
    pub fn import_as(&mut self, id: u64, path: PathBuf, rate: u32, ctx: &egui::Context) {
        if self.get(id).is_some() {
            return;
        }
        let name = path.file_stem().map_or_else(
            || path.display().to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        let info = FileInfo {
            format: format_name(&path).unwrap_or("?").to_owned(),
            ..FileInfo::default()
        };
        self.items.push(Item {
            id,
            path,
            name,
            state: State::Loading,
            info,
            peaks: None,
            job: None,
            streamed: None,
        });
        self.start_job(id, rate, ctx);
    }

    /// Adds audio that is already in memory as `id`, ready at once
    /// (`--stress`: generated audio for every file of the stress project).
    pub fn insert_ready(&mut self, id: u64, name: &str, audio: Arc<ClipAudio>, peaks: Arc<Peaks>) {
        self.items.retain(|i| i.id != id);
        self.items.push(Item {
            id,
            path: PathBuf::from(name),
            name: name.to_owned(),
            info: FileInfo {
                format: "Generated".into(),
                sample_rate: Some(audio.sample_rate()),
                channels: Some(audio.channels()),
                frames: Some(audio.frames() as u64),
                seconds: Some(audio.frames() as f64 / f64::from(audio.sample_rate().max(1))),
                ..FileInfo::default()
            },
            state: State::Ready(audio),
            peaks: Some(peaks),
            job: None,
            streamed: None,
        });
    }

    /// Adds `id` as a file still loading, until `insert_ready` replaces it
    /// (tests: loads that finish in a chosen order).
    #[cfg(test)]
    pub fn insert_loading(&mut self, id: u64, name: &str, rate: u32) {
        self.items.retain(|i| i.id != id);
        self.items.push(Item {
            id,
            path: PathBuf::from(name),
            name: name.to_owned(),
            info: FileInfo::default(),
            state: State::Loading,
            peaks: None,
            job: Some(Job {
                cancel: Arc::new(AtomicBool::new(false)),
                progress: Arc::new(AtomicU32::new(0)),
                rate,
                stream: false,
            }),
            streamed: None,
        });
    }

    /// Turns playing from disk on (with the pool, disk thread and cache it
    /// uses) or off. Files whose way of playing changes load again at the
    /// next [`Self::update`]. Until then, clips of files in memory play as
    /// before; turned off, clips of streamed files are silent (their disk
    /// thread has stopped).
    pub fn set_streaming(&mut self, streaming: Option<streaming::Streaming>) {
        // The old disk thread stops here.
        self.streaming = streaming;
    }

    /// The pool, disk thread and cache while streaming is on.
    pub fn streaming(&self) -> Option<&streaming::Streaming> {
        self.streaming.as_ref()
    }

    fn start_job(&mut self, id: u64, rate: u32, ctx: &egui::Context) {
        let Some(item) = self.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        if let Some(old) = item.job.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let cache = self.streaming.as_ref().map(|s| Arc::clone(s.cache()));
        let job = Job {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicU32::new(0)),
            rate,
            stream: cache.is_some(),
        };
        let stream = job.stream;
        let (cancel, progress) = (Arc::clone(&job.cancel), Arc::clone(&job.progress));
        let (path, done, ctx) = (item.path.clone(), self.done_tx.clone(), ctx.clone());
        item.job = Some(job);
        crate::trace::log(|| format!("media: loading {} at {rate} Hz", path.display()));
        std::thread::Builder::new()
            .name("lumen-media-load".into())
            .spawn(move || {
                let mut shown = 0.0;
                let mut report = |p: f32| {
                    progress.store(p.to_bits(), Ordering::Relaxed);
                    // Repaint for every 2% so the bar moves without
                    // flooding the UI.
                    if p - shown >= 0.02 {
                        shown = p;
                        ctx.request_repaint();
                    }
                };
                let size_bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                let started = std::time::Instant::now();
                // A long file plays from disk when streaming is on (M B).
                let header = cache
                    .as_ref()
                    .and_then(|_| lumen_media::probe(&path).ok())
                    .filter(streaming::streams);
                let (source, result, peaks) = match (&cache, header) {
                    (Some(cache), Some(header)) => {
                        match streaming::load(cache, &path, &header, rate, &cancel, &mut report) {
                            Ok(streamed) => (
                                Some(streamed.source),
                                Ok(streaming::Loaded::Stream(streamed.prepared)),
                                streamed.peaks,
                            ),
                            Err(why) => (None, Err(why), None),
                        }
                    }
                    _ => {
                        let (source, result) = decode(&path, rate, &cancel, &mut report);
                        let peaks = result.as_ref().ok().and_then(|audio| audio.peaks(&cancel));
                        (source, result.map(streaming::Loaded::Memory), peaks)
                    }
                };
                crate::trace::log(|| {
                    format!(
                        "media: {} loaded in {:.1} ms",
                        path.display(),
                        started.elapsed().as_secs_f64() * 1000.0
                    )
                });
                report(1.0);
                let _ = done.send(Done {
                    id,
                    rate,
                    stream,
                    source,
                    size_bytes,
                    result,
                    peaks,
                });
                ctx.request_repaint();
            })
            .expect("spawn media loader");
    }

    /// Applies finished loads, and loads ready files again when the device
    /// rate is no longer theirs or streaming was turned on or off. Returns
    /// true if any file became ready, failed or was removed, so the engine's
    /// clips need rebuilding.
    pub fn update(&mut self, rate: u32, ctx: &egui::Context) -> bool {
        let mut changed = false;
        while let Ok(done) = self.done_rx.try_recv() {
            let Some(item) = self.items.iter_mut().find(|i| i.id == done.id) else {
                continue;
            };
            // A load superseded by a newer one (another rate, streaming
            // turned on or off) is ignored.
            if item
                .job
                .as_ref()
                .is_some_and(|j| j.rate != done.rate || j.stream != done.stream)
            {
                continue;
            }
            item.job = None;
            item.streamed = Some(done.stream);
            item.info.size_bytes = done.size_bytes;
            if let Some((rate, channels, frames, seconds)) = done.source {
                item.info.sample_rate = Some(rate);
                item.info.channels = Some(channels);
                item.info.frames = Some(frames);
                item.info.seconds = Some(seconds);
            }
            let ready = match done.result {
                Ok(streaming::Loaded::Memory(audio)) => Some(ClipAudio::Memory {
                    sample_rate: audio.sample_rate,
                    channels: audio.into_channels(),
                }),
                Ok(streaming::Loaded::Stream(prepared)) => {
                    self.streaming.as_ref().map(|streaming| {
                        let layout = prepared.layout;
                        ClipAudio::Stream(StreamAudio::new(
                            streaming.pool(),
                            streams::register(prepared.path),
                            layout.sample_rate,
                            layout.played() as u16,
                            layout.frames,
                        ))
                    })
                }
                Err(MediaError::Cancelled) => None,
                Err(error) => {
                    streaming::release(&item.state);
                    item.state = State::Failed(error.to_string());
                    None
                }
            };
            if let Some(audio) = ready {
                crate::trace::log(|| {
                    format!(
                        "media: {} ready ({:.1} s, {})",
                        item.name,
                        audio.frames() as f64 / f64::from(audio.sample_rate().max(1)),
                        if audio.memory().is_some() {
                            "in memory"
                        } else {
                            "from disk"
                        }
                    )
                });
                streaming::release(&item.state);
                item.state = State::Ready(Arc::new(audio));
                item.peaks = done.peaks.map(Arc::new);
            }
            changed = true;
        }
        changed |= self.take_twins();
        // Cancelled loads leave nothing behind.
        let before = self.items.len();
        self.items
            .retain(|i| !(matches!(i.state, State::Loading) && i.job.is_none()));
        changed |= self.items.len() != before;

        let on = self.streaming.is_some();
        let stale: Vec<u64> = self
            .items
            .iter()
            .filter(|i| match (&i.state, &i.job) {
                (State::Ready(audio), None) => {
                    audio.sample_rate() != rate || i.streamed.is_some_and(|was| was != on)
                }
                (_, Some(job)) => {
                    (job.rate != rate || job.stream != on) && !matches!(i.state, State::Failed(_))
                }
                _ => false,
            })
            .map(|i| i.id)
            .collect();
        for id in stale {
            self.start_job(id, rate, ctx);
        }
        changed
    }

    /// Loading progress, 0.0–1.0, while a file loads or converts.
    pub fn progress(&self, id: u64) -> Option<f32> {
        let job = self.get(id)?.job.as_ref()?;
        Some(f32::from_bits(job.progress.load(Ordering::Relaxed)))
    }

    /// Any file is loading or converting.
    pub fn is_loading(&self) -> bool {
        self.items.iter().any(|i| i.job.is_some())
    }

    /// Stops every load in progress; files that were never ready disappear.
    pub fn cancel_all(&mut self) {
        for item in &mut self.items {
            if let Some(job) = &item.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Forgets a failed file.
    pub fn dismiss(&mut self, id: u64) {
        self.items
            .retain(|i| !(i.id == id && matches!(i.state, State::Failed(_))));
    }

    /// Bytes of audio held in memory: files in memory, and the page pool
    /// while streaming is on.
    pub fn memory(&self) -> usize {
        let files: usize = self
            .with_twins()
            .map(|i| match &i.state {
                State::Ready(audio) => audio.memory().map_or(0, |c| c.len() * audio.frames() * 4),
                _ => 0,
            })
            .sum();
        files + self.streaming.as_ref().map_or(0, |s| s.pool().memory())
    }
}

/// The file's own rate, channels and length (frames, seconds).
type Source = (u32, usize, u64, f64);

/// Decodes `path` and converts it to `rate`, keeping the file's own details.
/// Blocks.
fn decode(
    path: &Path,
    rate: u32,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(f32),
) -> (Option<Source>, Result<lumen_media::Audio, MediaError>) {
    let decoded = lumen_media::decode(path, cancel, &mut |p| report(p * 0.8));
    let source = decoded
        .as_ref()
        .ok()
        .map(|a| (a.sample_rate, a.channels(), a.frames() as u64, a.seconds()));
    let result = decoded.and_then(|audio| {
        let converted = lumen_media::resample(&audio, rate, cancel);
        report(0.98);
        converted
    });
    (source, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_filter_offers_every_format_lumen_imports() {
        for extension in EXTENSIONS {
            let file = format!("a.{extension}");
            assert!(is_audio_file(Path::new(&file)), "{extension}");
        }
    }

    #[test]
    fn audio_files_are_recognised_by_extension() {
        assert!(is_audio_file(Path::new("drums.WAV")));
        assert!(is_audio_file(Path::new("C:/x/bass.flac")));
        assert!(is_audio_file(Path::new("vox.mp3")));
        assert!(!is_audio_file(Path::new("song.m4a")), "AAC deferred");
        assert!(!is_audio_file(Path::new("notes.txt")));
        assert!(!is_audio_file(Path::new("noextension")));
        assert_eq!(format_name(Path::new("a.AIF")), Some("AIFF"));
        assert_eq!(format_name(Path::new("a.ogg")), Some("Ogg Vorbis"));
        assert_eq!(format_name(Path::new("a.m4a")), None);
    }

    fn wav(path: &Path, rate: u32, frames: usize) {
        let data: Vec<u8> = (0..frames).flat_map(|_| [0u8, 0, 0, 0]).collect();
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16_u32.to_le_bytes());
        out.extend(1_u16.to_le_bytes());
        out.extend(2_u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 4).to_le_bytes());
        out.extend(4_u16.to_le_bytes());
        out.extend(16_u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        std::fs::write(path, out).unwrap();
    }

    fn settle(library: &mut Library, rate: u32, ctx: &egui::Context) {
        let started = std::time::Instant::now();
        while library.is_loading() {
            library.update(rate, ctx);
            assert!(started.elapsed().as_secs() < 20, "load did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        library.update(rate, ctx);
    }

    #[test]
    fn files_load_in_the_background_and_follow_the_device_rate() {
        let path = std::env::temp_dir().join(format!("lumen-app-media-{}.wav", std::process::id()));
        wav(&path, 44_100, 44_100);
        let ctx = egui::Context::default();
        let mut library = Library::default();
        let id = 1;
        library.import_as(id, path.clone(), 48_000, &ctx);
        settle(&mut library, 48_000, &ctx);
        let State::Ready(audio) = &library.get(id).unwrap().state else {
            panic!("not ready: {:?}", library.get(id).unwrap().state);
        };
        assert_eq!(audio.sample_rate(), 48_000);
        assert!(library.memory() > 0);
        let info = &library.get(id).unwrap().info;
        assert_eq!(info.format, "WAV");
        assert_eq!(info.sample_rate, Some(44_100), "the file's own rate");
        assert_eq!(info.channels, Some(2));
        assert!((info.seconds.unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(info.size_bytes, 44 + 44_100 * 4);

        // The device switches to 44.1 kHz: the file is converted again.
        library.update(44_100, &ctx);
        assert!(library.is_loading());
        settle(&mut library, 44_100, &ctx);
        let State::Ready(audio) = &library.get(id).unwrap().state else {
            panic!("not ready after the rate change");
        };
        assert_eq!(audio.sample_rate(), 44_100);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bad_files_fail_with_a_reason_and_can_be_dismissed() {
        let path = std::env::temp_dir().join(format!("lumen-app-bad-{}.wav", std::process::id()));
        std::fs::write(&path, b"not audio").unwrap();
        let ctx = egui::Context::default();
        let mut library = Library::default();
        let id = 1;
        library.import_as(id, path.clone(), 48_000, &ctx);
        settle(&mut library, 48_000, &ctx);
        assert!(matches!(library.get(id).unwrap().state, State::Failed(_)));
        library.dismiss(id);
        assert!(library.get(id).is_none());
        let _ = std::fs::remove_file(path);
    }
}
