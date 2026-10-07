//! Audio files in the app (M2 slice 6): each one is loaded on its own
//! background thread (decode, then convert to the device rate) with progress
//! and cancel, and held in memory for the clip player. When the device rate
//! changes, ready files are converted again in the background; their clips
//! stay silent until then.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use lumen_engine::ClipAudio;
use lumen_media::MediaError;

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
    job: Option<Job>,
}

struct Job {
    cancel: Arc<AtomicBool>,
    /// Progress 0.0–1.0, as `f32` bits.
    progress: Arc<AtomicU32>,
    rate: u32,
}

struct Done {
    id: u64,
    rate: u32,
    /// The file's own rate, channels and length (frames, seconds).
    source: Option<(u32, usize, u64, f64)>,
    result: Result<lumen_media::Audio, MediaError>,
}

pub struct Library {
    items: Vec<Item>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
}

impl Default for Library {
    fn default() -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        Self {
            items: Vec::new(),
            done_tx,
            done_rx,
        }
    }
}

impl Library {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
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
            size_bytes: std::fs::metadata(&path).map_or(0, |m| m.len()),
            ..FileInfo::default()
        };
        self.items.push(Item {
            id,
            path,
            name,
            state: State::Loading,
            info,
            job: None,
        });
        self.start_job(id, rate, ctx);
    }

    fn start_job(&mut self, id: u64, rate: u32, ctx: &egui::Context) {
        let Some(item) = self.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        if let Some(old) = item.job.take() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let job = Job {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicU32::new(0)),
            rate,
        };
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
                // Decode and convert separately, to keep the file's own details.
                let decoded = lumen_media::decode(&path, &cancel, &mut |p| report(p * 0.8));
                let source = decoded
                    .as_ref()
                    .ok()
                    .map(|a| (a.sample_rate, a.channels(), a.frames() as u64, a.seconds()));
                let result = decoded.and_then(|audio| {
                    let converted = lumen_media::resample(&audio, rate, &cancel);
                    report(1.0);
                    converted
                });
                let _ = done.send(Done {
                    id,
                    rate,
                    source,
                    result,
                });
                ctx.request_repaint();
            })
            .expect("spawn media loader");
    }

    /// Applies finished loads, and converts ready files again when the
    /// device rate is no longer theirs. Returns true if any file became
    /// ready, failed or was removed, so the engine's clips need rebuilding.
    pub fn update(&mut self, rate: u32, ctx: &egui::Context) -> bool {
        let mut changed = false;
        while let Ok(done) = self.done_rx.try_recv() {
            let Some(item) = self.items.iter_mut().find(|i| i.id == done.id) else {
                continue;
            };
            // A load superseded by a newer one (another rate) is ignored.
            if item.job.as_ref().is_some_and(|j| j.rate != done.rate) {
                continue;
            }
            item.job = None;
            if let Some((rate, channels, frames, seconds)) = done.source {
                item.info.sample_rate = Some(rate);
                item.info.channels = Some(channels);
                item.info.frames = Some(frames);
                item.info.seconds = Some(seconds);
            }
            match done.result {
                Ok(audio) => {
                    crate::trace::log(|| {
                        format!("media: {} ready ({:.1} s)", item.name, audio.seconds())
                    });
                    item.state = State::Ready(Arc::new(ClipAudio {
                        sample_rate: audio.sample_rate,
                        channels: audio.into_channels(),
                    }));
                }
                Err(MediaError::Cancelled) => {}
                Err(error) => item.state = State::Failed(error.to_string()),
            }
            changed = true;
        }
        // Cancelled loads leave nothing behind.
        let before = self.items.len();
        self.items
            .retain(|i| !(matches!(i.state, State::Loading) && i.job.is_none()));
        changed |= self.items.len() != before;

        let stale: Vec<u64> = self
            .items
            .iter()
            .filter(|i| match (&i.state, &i.job) {
                (State::Ready(audio), None) => audio.sample_rate != rate,
                (_, Some(job)) => job.rate != rate && !matches!(i.state, State::Failed(_)),
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

    #[cfg(test)]
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

    /// Bytes of audio held in memory.
    pub fn memory(&self) -> usize {
        self.items
            .iter()
            .map(|i| match &i.state {
                State::Ready(audio) => audio.frames() * audio.channels.len() * 4,
                _ => 0,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(audio.sample_rate, 48_000);
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
        assert_eq!(audio.sample_rate, 44_100);
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
