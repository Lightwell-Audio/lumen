use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use lumen_engine::{Clip, ClipAudio, OfflineLoader, PAGE, PagePool, StreamAudio};
use lumen_media::MediaError;
use lumen_media::stream::Reader;

use super::warped::{Origin, Rendering, Warped};

static NEXT: AtomicU64 = AtomicU64::new(1);
static FILES: LazyLock<Mutex<HashMap<u64, PathBuf>>> = LazyLock::new(Default::default);
static WARPED: LazyLock<Mutex<HashMap<u64, Arc<Warped>>>> = LazyLock::new(Default::default);

const RENDER_PAGES: u32 = 256;

fn files() -> std::sync::MutexGuard<'static, HashMap<u64, PathBuf>> {
    FILES.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn register(path: PathBuf) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    files().insert(id, path);
    id
}

pub fn register_warped(warped: Arc<Warped>) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    warped_streams().insert(id, warped);
    id
}

fn warped_streams() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<Warped>>> {
    WARPED.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn warped(id: u64) -> Option<Arc<Warped>> {
    warped_streams().get(&id).cloned()
}

pub fn forget(id: u64) {
    files().remove(&id);
    warped_streams().remove(&id);
}

pub fn in_use() -> Vec<PathBuf> {
    files().values().cloned().collect()
}

pub fn path(id: u64) -> Option<PathBuf> {
    files().get(&id).cloned()
}

#[derive(Default)]
pub struct Readers {
    open: HashMap<u64, Reader>,
    rendering: HashMap<u64, Rendering>,
    fixed: Option<Fixed>,
    polling: bool,
}

#[derive(Default)]
struct Fixed {
    paths: HashMap<u64, PathBuf>,
    warped: HashMap<u64, Arc<Warped>>,
}

impl Readers {
    pub fn polling() -> Self {
        Self {
            polling: true,
            ..Self::default()
        }
    }

    pub fn fixed(ids: impl IntoIterator<Item = u64>) -> Self {
        let files = files();
        let streams = warped_streams();
        let mut fixed = Fixed::default();
        for id in ids {
            if let Some(path) = files.get(&id) {
                fixed.paths.insert(id, path.clone());
            }
            if let Some(warped) = streams.get(&id) {
                if let Origin::File(inner) = &warped.origin
                    && let Some(path) = files.get(inner)
                {
                    fixed.paths.insert(*inner, path.clone());
                }
                fixed.warped.insert(id, Arc::clone(warped));
            }
        }
        Self {
            fixed: Some(fixed),
            ..Self::default()
        }
    }

    fn path_of(&self, id: u64) -> Option<PathBuf> {
        match &self.fixed {
            Some(fixed) => fixed.paths.get(&id).cloned(),
            None => path(id),
        }
    }

    fn warped_of(&self, id: u64) -> Option<Arc<Warped>> {
        match &self.fixed {
            Some(fixed) => fixed.warped.get(&id).cloned(),
            None => warped(id),
        }
    }

    fn open_rendering(&self, warped: Arc<Warped>) -> Result<Rendering, MediaError> {
        let file = match &warped.origin {
            Origin::Memory(_) => None,
            Origin::File(inner) => {
                let path = self
                    .path_of(*inner)
                    .ok_or_else(|| MediaError::Io(format!("no stream file for media {inner}")))?;
                Some(Reader::open(&path)?)
            }
        };
        Rendering::new(warped, file, self.polling)
            .ok_or_else(|| MediaError::Io("a warped stream has no audio to play".into()))
    }

    pub fn read(
        &mut self,
        audio: &StreamAudio,
        from: u64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let id = audio.id();
        if let Some(warped) = self.rendering_of(id)? {
            return warped.read(from, left, right);
        }
        if !self.open.contains_key(&id) {
            let path = self
                .path_of(id)
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

    fn rendering_of(&mut self, id: u64) -> Result<Option<&mut Rendering>, MediaError> {
        if !self.rendering.contains_key(&id) {
            let Some(warped) = self.warped_of(id) else {
                return Ok(None);
            };
            let rendering = self.open_rendering(warped)?;
            self.rendering.insert(id, rendering);
        }
        Ok(self.rendering.get_mut(&id))
    }

    pub fn keep(&mut self, streams: &[StreamAudio]) {
        self.open
            .retain(|id, _| streams.iter().any(|s| s.id() == *id));
        self.rendering
            .retain(|id, _| streams.iter().any(|s| s.id() == *id));
    }
}

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

pub fn in_memory(clips: Vec<Clip>) -> Result<Vec<Clip>, String> {
    let mut windows: HashMap<u64, Vec<(i64, i64)>> = HashMap::new();
    for clip in &clips {
        if let ClipAudio::Stream(stream) = &*clip.audio
            && warped(stream.id()).is_some()
        {
            let from = clip.offset.max(0);
            windows
                .entry(stream.id())
                .or_default()
                .push((from, from + clip.length.max(0)));
        }
    }
    for list in windows.values_mut() {
        list.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::with_capacity(list.len());
        for &(from, to) in list.iter() {
            match merged.last_mut() {
                Some(last) if from <= last.1 => last.1 = last.1.max(to),
                _ => merged.push((from, to)),
            }
        }
        *list = merged;
    }
    let mut read: HashMap<(u64, i64), Arc<ClipAudio>> = HashMap::new();
    clips
        .into_iter()
        .map(|mut clip| {
            if let ClipAudio::Stream(stream) = &*clip.audio {
                let window = windows.get(&stream.id()).and_then(|list| {
                    let from = clip.offset.max(0);
                    list.iter().find(|w| w.0 <= from && from <= w.1).copied()
                });
                let skipped = window.map_or(0, |w| w.0);
                let audio = match read.get(&(stream.id(), skipped)) {
                    Some(audio) => Arc::clone(audio),
                    None => {
                        let channels = read_window(stream, window).map_err(|why| {
                            format!("An audio file could not be read from disk ({why})")
                        })?;
                        let audio = Arc::new(ClipAudio::Memory {
                            sample_rate: stream.sample_rate(),
                            channels,
                        });
                        read.insert((stream.id(), skipped), Arc::clone(&audio));
                        audio
                    }
                };
                clip.audio = audio;
                clip.offset -= skipped;
            }
            Ok(clip)
        })
        .collect()
}

fn read_window(
    stream: &StreamAudio,
    window: Option<(i64, i64)>,
) -> Result<Vec<Vec<f32>>, MediaError> {
    let Some((from, to)) = window else {
        return read_all(&ClipAudio::Stream(stream.clone()));
    };
    let frames = (to - from).max(0) as usize;
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; if stream.channels() == 2 { frames } else { 0 }];
    Readers::default().read(stream, from as u64, &mut left, &mut right)?;
    Ok(if stream.channels() == 2 {
        vec![left, right]
    } else {
        vec![left]
    })
}

#[derive(Clone, Default)]
pub struct Unread(Arc<Mutex<Option<String>>>);

impl Unread {
    fn note(&self, why: String) {
        let mut first = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        first.get_or_insert(why);
    }

    pub fn check(&self) -> Result<(), String> {
        match &*self.0.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }
}

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
                    unread.note(format!("An audio file could not be read from disk ({why})"));
                })
                .ok()
        }),
    );
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
