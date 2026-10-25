//! The peak scan behind Normalize (M7b slice E1). Reading every sample of a
//! clip's stretch of its file is work for a worker: it reports its progress,
//! stops when asked and says nothing if it was stopped. The stretch (which
//! seconds of the file a clip plays) is `lumen_project::edit::normalize`;
//! what gain a peak asks for is there too.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use lumen_engine::ClipAudio;
use lumen_project::edit::normalize::{Source, Window};
use lumen_project::model::Id;

/// Frames read between looks at the cancel flag and the progress counter.
const CHUNK: usize = 1 << 16;

/// One clip's stretch to read.
pub struct Job {
    pub clip: Id,
    /// The audio the clip plays (its reversed twin, if it is reversed).
    pub audio: Arc<ClipAudio>,
    /// What the scan reads, kept to tell afterwards whether the clip still
    /// plays it.
    pub source: Source,
}

/// What a scan found for one clip: the largest absolute sample over every
/// channel of its stretch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub clip: Id,
    pub peak: f32,
    /// What it read.
    pub source: Source,
}

/// Where a scan stands.
#[derive(Debug, PartialEq)]
pub enum Poll {
    /// Still reading, this far (0.0 to 1.0).
    Running(f32),
    Done(Vec<Peak>),
    /// Stopped before it finished; nothing was found.
    Stopped,
}

/// A scan on its worker. Dropping it stops the worker.
pub struct Scan {
    results: Receiver<Vec<Peak>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The frames of `audio` that `window` covers: from its start to the end of
/// the stretch or the audio, whichever comes first.
fn frames_of(audio: &ClipAudio, window: Window) -> std::ops::Range<usize> {
    let rate = f64::from(audio.sample_rate);
    let first = (window.from * rate).round().max(0.0);
    let last = ((window.from + window.span) * rate).round().max(0.0);
    let total = audio.frames() as f64;
    (first.min(total) as usize)..(last.min(total) as usize)
}

/// The largest absolute sample of `window` in `audio`, over every channel.
/// `read` is told how many frames were read after each chunk; the scan
/// stops, returning `None`, once `cancel` is set.
pub fn peak_of(
    audio: &ClipAudio,
    window: Window,
    cancel: &AtomicBool,
    mut read: impl FnMut(usize),
) -> Option<f32> {
    let frames = frames_of(audio, window);
    let mut peak = 0.0f32;
    let mut at = frames.start;
    while at < frames.end {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let to = (at + CHUNK).min(frames.end);
        for channel in &audio.channels {
            for &sample in &channel[at..to] {
                peak = peak.max(sample.abs());
            }
        }
        read(to - at);
        at = to;
    }
    Some(peak)
}

impl Scan {
    /// Starts reading `jobs` on a worker; `ctx` is woken as it goes.
    pub fn start(jobs: Vec<Job>, ctx: &egui::Context) -> Self {
        let (tx, results) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (stop, shown, ctx) = (Arc::clone(&cancel), Arc::clone(&progress), ctx.clone());
        let total: usize = jobs
            .iter()
            .map(|j| frames_of(&j.audio, j.source.window).len())
            .sum::<usize>()
            .max(1);
        std::thread::Builder::new()
            .name("lumen-normalize".into())
            .spawn(move || {
                let mut done = 0usize;
                let mut found = Vec::with_capacity(jobs.len());
                for job in &jobs {
                    let peak = peak_of(&job.audio, job.source.window, &stop, |n| {
                        done += n;
                        shown.store((done as f32 / total as f32).to_bits(), Ordering::Relaxed);
                        ctx.request_repaint();
                    });
                    let Some(peak) = peak else {
                        return;
                    };
                    found.push(Peak {
                        clip: job.clip,
                        peak,
                        source: job.source,
                    });
                }
                let _ = tx.send(found);
                ctx.request_repaint();
            })
            .expect("spawn normalize scan");
        Self {
            results,
            cancel,
            progress,
        }
    }

    /// Where the scan stands; a finished scan answers `Done` once.
    pub fn poll(&self) -> Poll {
        match self.results.try_recv() {
            Ok(found) => Poll::Done(found),
            Err(TryRecvError::Empty) => {
                Poll::Running(f32::from_bits(self.progress.load(Ordering::Relaxed)))
            }
            Err(TryRecvError::Disconnected) => Poll::Stopped,
        }
    }

    /// Asks the worker to stop.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn audio(frames: usize) -> Arc<ClipAudio> {
        let left: Vec<f32> = (0..frames).map(|i| (i % 100) as f32 / 1_000.0).collect();
        let mut right = vec![0.0; frames];
        // The loudest sample is negative, on the second channel.
        right[frames / 2] = -0.9;
        Arc::new(ClipAudio {
            sample_rate: 1_000,
            channels: vec![left, right],
        })
    }

    fn whole(seconds: f64) -> Window {
        Window {
            from: 0.0,
            span: seconds,
        }
    }

    fn wait(scan: &Scan) -> Poll {
        let started = Instant::now();
        loop {
            match scan.poll() {
                Poll::Running(_) => {
                    assert!(started.elapsed() < Duration::from_secs(20), "scan hung");
                    std::thread::sleep(Duration::from_millis(1));
                }
                other => return other,
            }
        }
    }

    #[test]
    fn the_peak_is_the_largest_absolute_sample_of_any_channel() {
        let audio = audio(200_000);
        let mut read = 0;
        let peak = peak_of(&audio, whole(200.0), &AtomicBool::new(false), |n| read += n);
        assert_eq!(peak, Some(0.9));
        assert_eq!(read, 200_000, "every frame was read and counted once");
    }

    #[test]
    fn only_the_clips_stretch_is_read() {
        let audio = audio(200_000);
        let quiet = Window {
            from: 0.0,
            span: 50.0,
        };
        let peak = peak_of(&audio, quiet, &AtomicBool::new(false), |_| {});
        assert_eq!(peak, Some(0.099), "the loud sample is at 100 s");
        let past = Window {
            from: 500.0,
            span: 10.0,
        };
        assert_eq!(
            peak_of(&audio, past, &AtomicBool::new(false), |_| {}),
            Some(0.0),
            "a stretch past the end of the file holds nothing"
        );
        let before = Window {
            from: -5.0,
            span: 10.0,
        };
        assert!(peak_of(&audio, before, &AtomicBool::new(false), |_| {}).is_some());
    }

    #[test]
    fn a_stopped_read_gives_nothing() {
        let audio = audio(200_000);
        let stop = AtomicBool::new(true);
        assert_eq!(peak_of(&audio, whole(200.0), &stop, |_| {}), None);
    }

    fn source(window: Window) -> Source {
        Source {
            media: Id(7),
            reversed: false,
            window,
        }
    }

    #[test]
    fn a_scan_finds_each_clips_peak_on_its_worker() {
        let ctx = egui::Context::default();
        let second = Window {
            from: 0.0,
            span: 50.0,
        };
        let jobs = vec![
            Job {
                clip: Id(1),
                audio: audio(200_000),
                source: source(whole(200.0)),
            },
            Job {
                clip: Id(2),
                audio: audio(200_000),
                source: source(second),
            },
        ];
        let scan = Scan::start(jobs, &ctx);
        let Poll::Done(found) = wait(&scan) else {
            panic!("the scan stopped")
        };
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0],
            Peak {
                clip: Id(1),
                peak: 0.9,
                source: source(whole(200.0)),
            }
        );
        assert_eq!(
            found[1],
            Peak {
                clip: Id(2),
                peak: 0.099,
                source: source(second),
            }
        );
    }

    #[test]
    fn a_scan_that_is_stopped_ends_without_an_answer() {
        let ctx = egui::Context::default();
        let jobs = vec![Job {
            clip: Id(1),
            audio: audio(40_000_000),
            source: source(whole(40_000.0)),
        }];
        let scan = Scan::start(jobs, &ctx);
        scan.stop();
        assert_eq!(wait(&scan), Poll::Stopped);
    }
}
