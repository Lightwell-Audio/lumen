use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use lumen_media::MediaError;
use lumen_media::detect::Analysis;
use lumen_media::stream::cache::Cache;
use lumen_project::model::{Clip, ClipContent};

pub(crate) const DEFAULT_SENSITIVITY: u8 = 50;

pub(crate) fn file_key(clip: &Clip) -> Option<u64> {
    let ClipContent::Audio { media, .. } = &clip.content else {
        return None;
    };
    Some(if clip.reversed {
        crate::media::reversed_key(media.0)
    } else {
        media.0
    })
}

pub(crate) enum State {
    Idle,
    Working(Job),
    Ready(Arc<Analysis>),
    Failed(String),
}

pub(crate) struct Job {
    done: Receiver<Result<Option<Analysis>, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl Job {
    pub(crate) fn fraction(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(crate) struct Detection {
    pub media: Option<u64>,
    path: Option<PathBuf>,
    pub state: State,
    pub sensitivity: u8,
    pub previewing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    Strip,
    Wave,
    Nowhere,
}

impl Shown {
    pub(crate) fn of(warped: bool, previewing: bool) -> Self {
        match (warped, previewing) {
            (true, _) => Self::Strip,
            (false, true) => Self::Wave,
            (false, false) => Self::Nowhere,
        }
    }
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            media: None,
            path: None,
            state: State::Idle,
            sensitivity: DEFAULT_SENSITIVITY,
            previewing: false,
        }
    }
}

impl Detection {
    pub(crate) fn found(&self, clip: &Clip) -> Option<(u64, Arc<Analysis>, u8)> {
        let key = file_key(clip)?;
        self.analysis(key)
            .map(|found| (key, Arc::clone(found), self.sensitivity))
    }

    pub(crate) fn analysis(&self, media: u64) -> Option<&Arc<Analysis>> {
        match &self.state {
            State::Ready(found) if self.media == Some(media) => Some(found),
            _ => None,
        }
    }

    pub(crate) fn working(&self, media: u64) -> Option<&Job> {
        match &self.state {
            State::Working(job) if self.media == Some(media) => Some(job),
            _ => None,
        }
    }

    pub(crate) fn wants(&self, media: u64, path: &std::path::Path) -> bool {
        self.media != Some(media)
            || self.path.as_deref() != Some(path)
            || matches!(self.state, State::Idle)
    }

    pub(crate) fn start(
        &mut self,
        key: u64,
        (path, rate): (PathBuf, u32),
        cache: PathBuf,
        ctx: &eframe::egui::Context,
    ) {
        let (tx, done) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let job = Job {
            done,
            cancel: Arc::clone(&cancel),
            progress: Arc::clone(&progress),
        };
        let ctx = ctx.clone();
        let reversed = crate::media::media_of(key).1;
        self.path = Some(path.clone());
        let spawned = std::thread::Builder::new()
            .name("lumen-app::detect".into())
            .spawn(move || {
                let result = work(&path, (rate, reversed), cache, &cancel, &progress);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        self.media = Some(key);
        self.state = match spawned {
            Ok(_) => State::Working(job),
            Err(e) => State::Failed(e.to_string()),
        };
    }

    pub(crate) fn poll(&mut self) -> bool {
        let State::Working(job) = &self.state else {
            return false;
        };
        let next = match job.done.try_recv() {
            Ok(Ok(Some(found))) => State::Ready(Arc::new(found)),
            Ok(Ok(None)) => State::Idle,
            Ok(Err(why)) => State::Failed(why),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                State::Failed("the detection stopped without an answer".to_owned())
            }
        };
        self.state = next;
        true
    }

    pub(crate) fn clear(&mut self) {
        self.media = None;
        self.path = None;
        self.state = State::Idle;
    }
}

pub(crate) fn in_view(
    analysis: &Analysis,
    sensitivity: u8,
    seconds: (f64, f64),
) -> impl Iterator<Item = &lumen_media::detect::Transient> {
    let all = &analysis.transients;
    let first = all.partition_point(|t| t.seconds < seconds.0);
    let last = all.partition_point(|t| t.seconds <= seconds.1);
    let least = lumen_media::detect::threshold(sensitivity);
    all[first..last.max(first)]
        .iter()
        .filter(move |t| t.strength >= least)
}

pub(crate) fn listen(
    path: &std::path::Path,
    rate: u32,
    cache: PathBuf,
    cancel: &AtomicBool,
) -> Result<Option<Analysis>, String> {
    work(
        path,
        (rate, false),
        cache,
        cancel,
        &AtomicU32::new(0.0_f32.to_bits()),
    )
}

fn work(
    path: &std::path::Path,
    (rate, reversed): (u32, bool),
    cache: PathBuf,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Option<Analysis>, String> {
    let cache = Cache::new(cache);
    let say = |from: f32, span: f32, fraction: f32| {
        progress.store((from + span * fraction).to_bits(), Ordering::Relaxed);
    };
    let prepared = cache
        .prepare(path, rate, cancel, &mut |f| say(0.0, 0.5, f))
        .map_err(|e| e.to_string())?;
    let played = if reversed {
        match cache.reversed(&prepared.path, cancel) {
            Ok(twin) => twin.path,
            Err(MediaError::Cancelled) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
    } else {
        prepared.path
    };
    cache
        .analysis(&played, cancel, &mut |f| say(0.5, 0.5, f))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(detection: &mut Detection) {
        lumen_testkit::wait_until("detection answers", || detection.poll());
    }

    #[test]
    fn a_clip_that_is_not_warped_shows_its_transients_only_while_previewing() {
        assert_eq!(Shown::of(true, false), Shown::Strip);
        assert_eq!(Shown::of(true, true), Shown::Strip);
        assert_eq!(Shown::of(false, true), Shown::Wave);
        assert_eq!(Shown::of(false, false), Shown::Nowhere);
    }

    #[test]
    fn only_the_transients_in_view_are_offered() {
        use lumen_media::detect::Transient;
        let transients = (0..1000)
            .map(|i| Transient {
                seconds: f64::from(i) * 0.5,
                strength: if i % 2 == 0 { 0.9 } else { 0.2 },
            })
            .collect();
        let analysis = Analysis {
            seconds: 500.0,
            transients,
            tempo: None,
        };
        let at = |s, a, b| in_view(&analysis, s, (a, b)).count();
        assert_eq!(at(100, 10.0, 20.0), 21, "both ends are in view");
        assert_eq!(at(0, 10.0, 20.0), 11, "the soft ones are hidden");
        assert_eq!(at(100, 600.0, 700.0), 0, "past the end");
        assert_eq!(at(100, 20.0, 10.0), 0, "backwards is empty");
    }

    #[test]
    fn a_file_that_is_not_there_fails_with_words() {
        let mut detection = Detection::default();
        let dir = std::env::temp_dir().join("lumen-detect-missing");
        detection.start(
            7,
            (dir.join("none.wav"), 48_000),
            dir.join("cache"),
            &eframe::egui::Context::default(),
        );
        assert!(!detection.wants(7, &dir.join("none.wav")), "asked now");
        assert!(detection.wants(7, &dir.join("other.wav")), "another file");
        assert!(detection.wants(8, &dir.join("none.wav")), "another media");
        wait(&mut detection);
        assert!(matches!(detection.state, State::Failed(ref why) if !why.is_empty()));
        assert!(detection.analysis(7).is_none());
        detection.clear();
        assert!(
            detection.wants(7, &dir.join("none.wav")),
            "cleared: asked again"
        );
    }
}
