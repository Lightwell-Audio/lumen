//! The worker behind Consolidate (M7b slice E3). Each part (one track's
//! selected clips) is rendered through the clip player, as it plays, into a
//! 32-bit float WAV, with progress and a way to stop. Which clips go in which
//! part, and what replaces them, is `lumen_project::edit::consolidate`; this
//! module only turns clips into files, off the UI thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use lumen_engine::{ClipRender, Finished, Timeline, render_clips};
use lumen_media::wav::{Sample, Spec, Writer};
use lumen_project::edit::consolidate::{Baked, Part};
use lumen_project::model::{Id, Media};

/// One file to render.
pub struct Job {
    pub part: Part,
    /// The part's clips as the engine plays them.
    pub clips: Vec<lumen_engine::Clip>,
    /// Timeline sample of the file's first frame, and its length.
    pub from: i64,
    pub frames: u64,
    /// Where the file goes, and the media id it will have.
    pub file: PathBuf,
    pub media: Id,
}

/// Where a render stands.
#[derive(Debug)]
pub enum Poll {
    /// Still rendering, this far (0.0 to 1.0).
    Running(f32),
    Done(Vec<Baked>),
    /// Stopped before it finished; the files are removed.
    Stopped,
    Failed(String),
}

/// A render on its worker. Dropping it stops the worker.
pub struct Render {
    results: Receiver<Result<Vec<Baked>, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl Drop for Render {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Renders one job to its file. `done` is told how many frames were written
/// after each block. `None` means `cancel` stopped it and no file is left.
pub fn render_job(
    job: Job,
    timeline: &Timeline,
    rate: u32,
    cancel: &AtomicBool,
    done: &mut dyn FnMut(u64),
) -> Result<Option<Baked>, String> {
    let Job {
        part,
        clips,
        from,
        frames,
        file,
        media,
    } = job;
    let channels = if part.mono { 1 } else { 2 };
    let mut writer = Writer::create(
        &file,
        Spec {
            sample_rate: rate,
            channels,
            sample: Sample::F32,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut failed = None;
    let mut written = 0;
    let ended = render_clips(
        ClipRender {
            clips,
            timeline: timeline.clone(),
            sample_rate: rate,
            from,
            frames,
            bare_ends: true,
            block: lumen_engine::MOST_BLOCK,
        },
        cancel,
        &mut |_| {},
        &mut |left, right| {
            if failed.is_some() {
                return;
            }
            let block: &[&[f32]] = if channels == 1 {
                &[left]
            } else {
                &[left, right]
            };
            match writer.write(block) {
                Ok(()) => {
                    written += left.len() as u64;
                    done(written);
                }
                Err(e) => failed = Some(e.to_string()),
            }
        },
    );
    if let Some(error) = failed {
        return Err(error);
    }
    if ended == Finished::Cancelled {
        return Ok(None);
    }
    let frames = writer.finish().map_err(|e| e.to_string())?;
    let size_bytes = std::fs::metadata(&file).map_or(0, |m| m.len());
    Ok(Some(Baked {
        media: Media {
            id: media,
            path: file.to_string_lossy().into_owned(),
            original_path: None,
            frames,
            channels,
            sample_rate: rate,
            size_bytes,
        },
        part,
    }))
}

impl Render {
    /// Starts rendering `jobs` one after another on a worker; `ctx` is woken
    /// as it goes.
    pub fn start(jobs: Vec<Job>, timeline: Timeline, rate: u32, ctx: &egui::Context) -> Self {
        let (tx, results) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (stop, shown, ctx) = (Arc::clone(&cancel), Arc::clone(&progress), ctx.clone());
        let total: u64 = jobs.iter().map(|j| j.frames).sum::<u64>().max(1);
        std::thread::Builder::new()
            .name("lumen-consolidate".into())
            .spawn(move || {
                let mut before = 0;
                let mut baked: Vec<Baked> = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let frames = job.frames;
                    let result = render_job(job, &timeline, rate, &stop, &mut |n| {
                        let fraction = (before + n) as f32 / total as f32;
                        shown.store(fraction.min(1.0).to_bits(), Ordering::Relaxed);
                        ctx.request_repaint();
                    });
                    match result {
                        Ok(Some(done)) => baked.push(done),
                        // Stopped: drop the files already made and say nothing.
                        Ok(None) => {
                            remove_files(&baked);
                            return;
                        }
                        Err(e) => {
                            remove_files(&baked);
                            let _ = tx.send(Err(e));
                            ctx.request_repaint();
                            return;
                        }
                    }
                    before += frames;
                }
                let _ = tx.send(Ok(baked));
                ctx.request_repaint();
            })
            .expect("spawn consolidate render");
        Self {
            results,
            cancel,
            progress,
        }
    }

    /// Where the render stands; a finished render answers `Done` once.
    pub fn poll(&self) -> Poll {
        match self.results.try_recv() {
            Ok(Ok(baked)) => Poll::Done(baked),
            Ok(Err(why)) => Poll::Failed(why),
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

/// Removes the files of renders that will not be used.
pub fn remove_files(baked: &[Baked]) {
    for done in baked {
        let _ = std::fs::remove_file(&done.media.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_engine::{Clip, ClipAudio, ClipShape};
    use lumen_project::testing::TempDir;

    const RATE: u32 = 48_000;

    /// A part for one audio clip with the id `1`.
    fn part(mono: bool) -> Part {
        use lumen_core::time::Ticks;
        use lumen_project::model::{Clip, ClipContent};
        let clip = Clip {
            id: Id(1),
            name: "Take".into(),
            start: Ticks::ZERO,
            length: Ticks(960),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            content: ClipContent::Audio {
                media: Id(9),
                offset: 0,
                gain_db: 0.0,
            },
        };
        Part {
            track: Id(2),
            clips: vec![Arc::new(clip)],
            start: Ticks::ZERO,
            end: Ticks(960),
            name: "Take (joined)".into(),
            color: None,
            mono,
        }
    }

    /// A job for one clip over a ramp.
    fn job(dir: &std::path::Path, mono: bool) -> (Job, Arc<ClipAudio>) {
        let part = part(mono);
        let left: Vec<f32> = (0..20_000)
            .map(|i| (i % 500) as f32 / 1_000.0 - 0.25)
            .collect();
        let audio = Arc::new(ClipAudio {
            sample_rate: RATE,
            channels: if mono {
                vec![left]
            } else {
                vec![left.clone(), left.iter().map(|v| -v).collect()]
            },
        });
        let clip = Clip {
            id: 1,
            track: 1,
            start: Timeline::default().ticks_at(1_000, RATE),
            offset: 0,
            length: 10_000,
            gain: 1.0,
            audio: Arc::clone(&audio),
            shape: ClipShape::default(),
            envelope: Default::default(),
        };
        let job = Job {
            part,
            clips: vec![clip],
            from: 1_000,
            frames: 10_000,
            file: dir.join("out.wav"),
            media: Id(777),
        };
        (job, audio)
    }

    #[test]
    fn a_job_writes_a_float_file_that_is_the_clip_as_it_plays() {
        let dir = TempDir::new("consolidate-job");
        let (job, audio) = job(dir.path(), false);
        let file = job.file.clone();
        let mut last = 0;
        let baked = render_job(
            job,
            &Timeline::default(),
            RATE,
            &AtomicBool::new(false),
            &mut |n| last = n,
        )
        .unwrap()
        .expect("not cancelled");
        assert_eq!(last, 10_000);
        assert_eq!((baked.media.frames, baked.media.channels), (10_000, 2));
        assert_eq!(baked.media.sample_rate, RATE);
        assert_eq!(baked.media.path, file.to_string_lossy());
        assert!(baked.media.size_bytes > 40_000);
        let back = lumen_media::decode(&file, &AtomicBool::new(false), &mut |_| {}).unwrap();
        // Bare outer ends: the first frame is the source exactly (32-bit
        // float), and so is the last.
        assert_eq!(back.channel(0)[0], audio.channels[0][0]);
        assert_eq!(back.channel(1)[0], audio.channels[1][0]);
        assert_eq!(back.channel(0)[9_999], audio.channels[0][9_999]);
    }

    #[test]
    fn a_mono_part_writes_a_mono_file() {
        let dir = TempDir::new("consolidate-mono");
        let (job, _) = job(dir.path(), true);
        let baked = render_job(
            job,
            &Timeline::default(),
            RATE,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(baked.media.channels, 1);
    }

    #[test]
    fn a_cancelled_job_leaves_no_file() {
        let dir = TempDir::new("consolidate-cancel");
        let (job, _) = job(dir.path(), false);
        let file = job.file.clone();
        let done = render_job(
            job,
            &Timeline::default(),
            RATE,
            &AtomicBool::new(true),
            &mut |_| {},
        )
        .unwrap();
        assert!(done.is_none());
        assert!(!file.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_file_that_cannot_be_made_is_an_error_not_a_panic() {
        let dir = TempDir::new("consolidate-fail");
        let (mut job, _) = job(dir.path(), false);
        job.file = dir.path().join("missing").join("out.wav");
        let result = render_job(
            job,
            &Timeline::default(),
            RATE,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert!(result.is_err());
    }
}
