use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use lumen_engine::ClipAudio;
use lumen_project::edit::normalize::{Source, Window};
use lumen_project::model::Id;

const CHUNK: usize = 1 << 16;

pub struct Job {
    pub clip: Id,
    pub audio: Arc<ClipAudio>,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub clip: Id,
    pub peak: f32,
    pub source: Source,
}

#[derive(Debug, PartialEq)]
pub enum Poll {
    Running(f32),
    Done(Vec<Peak>),
    Stopped,
    Failed(String),
}

pub struct Scan {
    results: Receiver<Vec<Peak>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    failure: Option<String>,
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn frames_of(audio: &ClipAudio, window: Window) -> std::ops::Range<usize> {
    let rate = f64::from(audio.sample_rate());
    let first = (window.from * rate).round().max(0.0);
    let last = ((window.from + window.span) * rate).round().max(0.0);
    let total = audio.frames() as f64;
    (first.min(total) as usize)..(last.min(total) as usize)
}

pub fn peak_of(
    audio: &ClipAudio,
    window: Window,
    cancel: &AtomicBool,
    mut read: impl FnMut(usize),
) -> Option<f32> {
    let frames = frames_of(audio, window);
    let mut peak = 0.0f32;
    let mut at = frames.start;
    let mut readers = crate::media::streams::Readers::default();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while at < frames.end {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let to = (at + CHUNK).min(frames.end);
        let mut scan = |samples: &[f32]| {
            for &sample in samples {
                peak = peak.max(sample.abs());
            }
        };
        match audio {
            ClipAudio::Memory { channels, .. } => {
                for channel in channels {
                    scan(&channel[at..to]);
                }
            }
            ClipAudio::Stream(stream) => {
                left.resize(to - at, 0.0);
                right.resize(if stream.channels() == 2 { to - at } else { 0 }, 0.0);
                let got = readers
                    .read(stream, at as u64, &mut left, &mut right)
                    .unwrap_or(0);
                scan(&left[..got]);
                scan(&right[..got.min(right.len())]);
            }
        }
        read(to - at);
        at = to;
    }
    Some(peak)
}

impl Scan {
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
        let spawned = crate::spawn::spawn_named("lumen-normalize", move || {
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
        });
        let failure = match spawned {
            Ok(_) => None,
            Err(error) => {
                let why = crate::spawn::start_failed("reading the audio to normalize", &error);
                crate::trace::log(|| format!("normalize: {why}"));
                Some(why)
            }
        };
        Self {
            results,
            cancel,
            progress,
            failure,
        }
    }

    pub fn poll(&self) -> Poll {
        if let Some(why) = &self.failure {
            return Poll::Failed(why.clone());
        }
        match self.results.try_recv() {
            Ok(found) => Poll::Done(found),
            Err(TryRecvError::Empty) => {
                Poll::Running(f32::from_bits(self.progress.load(Ordering::Relaxed)))
            }
            Err(TryRecvError::Disconnected) => Poll::Stopped,
        }
    }

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
        right[frames / 2] = -0.9;
        Arc::new(ClipAudio::Memory {
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
