//! Stream files by id (M9 slice R6, ADR-0108). The library gives every file
//! it streams an id of its own, never reused, and registers the stream file's
//! path under it. Whoever holds a [`StreamAudio`] can then read it: the disk
//! thread into the live pool, a render into its own pool, a scan or a
//! deep-zoom strip into a buffer. Ids are unique in the process, so several
//! apps in one test process never share one.
//!
//! Every read here blocks on the disk: for workers, never the UI or audio
//! thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use lumen_engine::{Clip, ClipAudio, OfflineLoader, PAGE, PagePool, StreamAudio};
use lumen_media::MediaError;
use lumen_media::stream::Reader;

static NEXT: AtomicU64 = AtomicU64::new(1);
static FILES: LazyLock<Mutex<HashMap<u64, PathBuf>>> = LazyLock::new(Default::default);

/// Pages of a render's own pool: 8 MB, which a render reuses as it goes.
const RENDER_PAGES: u32 = 256;

fn files() -> std::sync::MutexGuard<'static, HashMap<u64, PathBuf>> {
    FILES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Registers the stream file `path`, returning its new id.
pub fn register(path: PathBuf) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    files().insert(id, path);
    id
}

/// Forgets a file no longer played. Readers already open keep reading.
pub fn forget(id: u64) {
    files().remove(&id);
}

/// The stream files registered now: those playing, which the cache keeps.
pub fn in_use() -> Vec<PathBuf> {
    files().values().cloned().collect()
}

/// The stream file registered as `id`.
pub fn path(id: u64) -> Option<PathBuf> {
    files().get(&id).cloned()
}

/// Open stream files, by id, read with blocking reads.
#[derive(Default)]
pub struct Readers {
    open: HashMap<u64, Reader>,
    /// Paths fixed when the readers were made ([`Readers::fixed`]), else the
    /// registry is asked.
    paths: Option<HashMap<u64, PathBuf>>,
}

impl Readers {
    /// Readers of the files registered now, by path, so a file forgotten
    /// later (another rate) still reads for a render already under way.
    pub fn fixed(ids: impl IntoIterator<Item = u64>) -> Self {
        let files = files();
        let paths = ids
            .into_iter()
            .filter_map(|id| Some((id, files.get(&id)?.clone())))
            .collect();
        Self {
            open: HashMap::new(),
            paths: Some(paths),
        }
    }

    /// Reads `audio` from frame `from` into `left` and, for a file of two
    /// channels, `right` (all one length; a mono file leaves `right` alone).
    /// Returns the frames that came from the file; the rest are silence.
    pub fn read(
        &mut self,
        audio: &StreamAudio,
        from: u64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let id = audio.id();
        if !self.open.contains_key(&id) {
            let path = match &self.paths {
                Some(paths) => paths.get(&id).cloned(),
                None => path(id),
            }
            .ok_or_else(|| MediaError::Io(format!("no stream file for media {id}")))?;
            self.open.insert(id, Reader::open(&path)?);
        }
        let reader = self.open.get_mut(&id).expect("opened above");
        if audio.channels() == 1 || reader.layout().channels == 1 {
            reader.read(from, &mut [left])
        } else {
            reader.read(from, &mut [left, right])
        }
    }

    /// Closes the files not among `streams`.
    pub fn keep(&mut self, streams: &[StreamAudio]) {
        self.open
            .retain(|id, _| streams.iter().any(|s| s.id() == *id));
    }
}

/// The samples of `audio`, every channel it plays, whole: a copy of audio in
/// memory, or the stream file read from disk. Blocks.
pub fn read_all(audio: &ClipAudio) -> Result<Vec<Vec<f32>>, MediaError> {
    match audio {
        ClipAudio::Memory { channels, .. } => Ok(channels.clone()),
        ClipAudio::Stream(stream) => {
            let frames = stream.frames() as usize;
            let mut left = vec![0.0; frames];
            let mut right = vec![0.0; if stream.channels() == 2 { frames } else { 0 }];
            let mut readers = Readers::default();
            if stream.channels() == 2 {
                readers.read(stream, 0, &mut left, &mut right)?;
                Ok(vec![left, right])
            } else {
                readers.read(stream, 0, &mut left, &mut [])?;
                Ok(vec![left])
            }
        }
    }
}

/// `clips` with every streamed file read whole into memory, each file once.
/// For a render the audio callback plays (a real-time export): an offline
/// stream loads a missing page on the thread that plays it, which must
/// never be the audio thread. Blocks on the disk.
pub fn in_memory(clips: Vec<Clip>) -> Result<Vec<Clip>, String> {
    let mut read: HashMap<u64, Arc<ClipAudio>> = HashMap::new();
    clips
        .into_iter()
        .map(|mut clip| {
            if let ClipAudio::Stream(stream) = &*clip.audio {
                let audio = match read.get(&stream.id()) {
                    Some(audio) => Arc::clone(audio),
                    None => {
                        let channels = read_all(&clip.audio).map_err(|why| {
                            format!("An audio file could not be read from disk ({why})")
                        })?;
                        let audio = Arc::new(ClipAudio::Memory {
                            sample_rate: stream.sample_rate(),
                            channels,
                        });
                        read.insert(stream.id(), Arc::clone(&audio));
                        audio
                    }
                };
                clip.audio = audio;
            }
            Ok(clip)
        })
        .collect()
}

/// What an offline render could not read from disk, shared by its loader
/// and its worker. A page that cannot be read plays silence, so the worker
/// asks [`Unread::check`] before it keeps what it made: a render with a
/// hole in it fails rather than leave a file that sounds wrong.
#[derive(Clone, Default)]
pub struct Unread(Arc<Mutex<Option<String>>>);

impl Unread {
    /// Keeps the first failure.
    fn note(&self, why: String) {
        let mut first = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        first.get_or_insert(why);
    }

    /// `Err` with a message for the user when a page could not be read.
    pub fn check(&self) -> Result<(), String> {
        match &*self.0.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }
}

/// `clips` as an offline render plays them: each streamed file gets a table
/// in a pool of the render's own, filled from disk on the render's thread
/// when the clip player reaches a page (so a render never starves). Clips
/// in memory are left as they are. Does not touch the disk itself. What
/// cannot be read is noted in `unread`.
pub fn offline(clips: Vec<Clip>, unread: &Unread) -> Vec<Clip> {
    let streams: Vec<StreamAudio> = clips
        .iter()
        .filter_map(|c| match &*c.audio {
            ClipAudio::Stream(s) => Some(s.clone()),
            ClipAudio::Memory { .. } => None,
        })
        .collect();
    if streams.is_empty() {
        return clips;
    }
    let mut readers = Readers::fixed(streams.iter().map(StreamAudio::id));
    let pool = PagePool::new(RENDER_PAGES);
    let unread = unread.clone();
    let loader = OfflineLoader::new(
        &pool,
        Box::new(move |audio, page, left, right| {
            readers
                .read(audio, (page * PAGE) as u64, left, right)
                .map_err(|why| {
                    crate::trace::log(|| {
                        format!("render: stream {} unreadable: {why}", audio.id())
                    });
                    // The stream file may be a cache copy with a hashed
                    // name, so the message names no file.
                    unread.note(format!("An audio file could not be read from disk ({why})"));
                })
                .ok()
        }),
    );
    // One table per file, shared by every clip that plays it.
    let mut tables: HashMap<u64, Arc<ClipAudio>> = HashMap::new();
    clips
        .into_iter()
        .map(|mut clip| {
            if let ClipAudio::Stream(live) = &*clip.audio {
                let audio = tables
                    .entry(live.id())
                    .or_insert_with(|| Arc::new(ClipAudio::Stream(loader.stream(live))));
                clip.audio = Arc::clone(audio);
            }
            clip
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use lumen_media::wav::{Sample, Spec, Writer};

    /// A float WAV of `channels`, removed when the guard drops.
    struct Removed(PathBuf);

    impl Drop for Removed {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn wav(frames: usize, channels: usize) -> (Removed, PathBuf, Vec<Vec<f32>>) {
        let path = std::env::temp_dir().join(format!(
            "lumen-streams-{}-{}.wav",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = Removed(path.clone());
        let data: Vec<Vec<f32>> = (0..channels)
            .map(|c| {
                (0..frames)
                    .map(|i| ((i * (c + 3)) % 997) as f32 / 1_000.0 - 0.5)
                    .collect()
            })
            .collect();
        let mut writer = Writer::create(
            &path,
            Spec {
                sample_rate: 48_000,
                channels: channels as u16,
                sample: Sample::F32,
            },
        )
        .unwrap();
        let parts: Vec<&[f32]> = data.iter().map(Vec::as_slice).collect();
        writer.write(&parts).unwrap();
        writer.finish().unwrap();
        (dir, path, data)
    }

    #[test]
    fn a_registered_stream_reads_whole_and_is_forgotten() {
        for channels in [1, 2] {
            let (_dir, path, data) = wav(10_000, channels);
            let id = register(path.clone());
            assert_eq!(super::path(id), Some(path));
            let pool = PagePool::new(1);
            let stream = StreamAudio::new(&pool, id, 48_000, channels as u16, 10_000);
            assert_eq!(read_all(&ClipAudio::Stream(stream.clone())).unwrap(), data);
            forget(id);
            assert!(read_all(&ClipAudio::Stream(stream)).is_err());
        }
    }

    #[test]
    fn ids_are_never_reused() {
        let a = register(PathBuf::from("a.wav"));
        forget(a);
        let b = register(PathBuf::from("a.wav"));
        assert_ne!(a, b);
        forget(b);
    }

    #[test]
    fn fixed_readers_still_read_a_file_forgotten_after_they_were_made() {
        let (_dir, path, data) = wav(5_000, 2);
        let id = register(path);
        let pool = PagePool::new(1);
        let stream = StreamAudio::new(&pool, id, 48_000, 2, 5_000);
        let mut readers = Readers::fixed([id]);
        forget(id);
        let (mut l, mut r) = (vec![0.0; 100], vec![0.0; 100]);
        assert_eq!(readers.read(&stream, 4_950, &mut l, &mut r), Ok(50));
        assert_eq!(l[..50], data[0][4_950..]);
        assert!(l[50..].iter().all(|v| *v == 0.0));
    }

    fn clip(id: u64, audio: ClipAudio) -> Clip {
        Clip {
            envelope: Default::default(),
            id,
            track: 0,
            start: lumen_core::time::Ticks::ZERO,
            offset: 0,
            length: audio.frames() as i64,
            gain: 1.0,
            audio: Arc::new(audio),
            shape: Default::default(),
        }
    }

    fn offline_stream(clips: &[Clip]) -> &StreamAudio {
        clips[0].audio.stream().expect("streamed")
    }

    /// A page that cannot be read plays silence, so the render must hear of
    /// it and fail rather than keep the file (review of slice R6).
    #[test]
    fn a_render_that_cannot_read_a_page_is_told() {
        let (_dir, path, _) = wav(3 * PAGE, 2);
        let id = register(path.clone());
        let pool = PagePool::new(1);
        let live = || {
            clip(
                1,
                ClipAudio::Stream(StreamAudio::new(&pool, id, 48_000, 2, 3 * PAGE as u64)),
            )
        };

        let read = Unread::default();
        let clips = offline(vec![live()], &read);
        assert!(offline_stream(&clips).ensure(0), "the file reads");
        assert_eq!(read.check(), Ok(()));

        std::fs::remove_file(&path).unwrap();
        let unread = Unread::default();
        let clips = offline(vec![live()], &unread);
        assert!(!offline_stream(&clips).ensure(0));
        let why = unread.check().unwrap_err();
        assert!(why.starts_with("An audio file could not be read"), "{why}");
        forget(id);
    }

    #[test]
    fn in_memory_reads_each_streamed_file_whole_once() {
        let (_dir, path, data) = wav(PAGE + 10, 2);
        let id = register(path);
        let pool = PagePool::new(1);
        let stream = StreamAudio::new(&pool, id, 48_000, 2, (PAGE + 10) as u64);
        let kept = ClipAudio::Memory {
            sample_rate: 48_000,
            channels: vec![vec![0.5; 4]],
        };
        let clips = in_memory(vec![
            clip(1, ClipAudio::Stream(stream.clone())),
            clip(2, ClipAudio::Stream(stream)),
            clip(3, kept),
        ])
        .unwrap();
        assert_eq!(clips[0].audio.memory(), Some(data.as_slice()));
        assert!(Arc::ptr_eq(&clips[0].audio, &clips[1].audio), "read once");
        assert_eq!(clips[2].audio.memory().unwrap()[0], vec![0.5; 4]);
        forget(id);
    }
}
