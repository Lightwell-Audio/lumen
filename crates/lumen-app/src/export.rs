//! Export choices and the background task (M9 R5). A render uses an immutable
//! project snapshot. Files are staged together; cancellation never publishes
//! part of a set of stems. Project media and undo history are untouched.

mod publish;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Project, TrackKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Project,
    Loop,
    Selection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Buses {
    #[default]
    Master,
    Tracks,
    Groups,
    Returns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    Wav16,
    Wav24,
    #[default]
    WavFloat,
    Flac16,
    Flac24,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav16 | Self::Wav24 | Self::WavFloat => "wav",
            Self::Flac16 | Self::Flac24 => "flac",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub scope: Scope,
    pub buses: Buses,
    pub format: Format,
    pub rate: u32,
    pub dither: bool,
    pub normalize: bool,
    pub tail: bool,
    pub realtime: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            scope: Scope::Project,
            buses: Buses::Master,
            format: Format::WavFloat,
            rate: 48_000,
            dither: false,
            normalize: false,
            tail: true,
            realtime: false,
        }
    }
}

/// A scope is always a half-open range, shared by all files in a stem set.
pub fn span(
    project: &Project,
    scope: Scope,
    looping: (Ticks, Ticks),
    selection: Option<(Ticks, Ticks)>,
) -> Option<(Ticks, Ticks)> {
    let (start, end) = match scope {
        Scope::Project => (
            Ticks::ZERO,
            project
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| c.start + c.length)
                .max()?,
        ),
        Scope::Loop => looping,
        Scope::Selection => selection?,
    };
    (end > start).then_some((start, end))
}

pub fn targets(project: &Project, buses: Buses) -> Vec<(Option<Id>, String)> {
    if buses == Buses::Master {
        return vec![(None, "Master".into())];
    }
    project
        .tracks
        .iter()
        .filter(|t| match buses {
            Buses::Tracks => matches!(t.kind, TrackKind::Audio | TrackKind::Midi),
            Buses::Groups => t.kind == TrackKind::Group,
            Buses::Returns => t.kind == TrackKind::Return,
            Buses::Master => false,
        })
        .map(|t| (Some(t.id), t.name.clone()))
        .collect()
}

#[derive(Debug, Clone)]
pub struct Report {
    pub path: PathBuf,
    pub frames: u64,
    pub rate: u32,
    pub lufs: f64,
    pub true_peak: f32,
}

pub enum Poll {
    Running(f32),
    Done(Vec<Report>),
    Stopped,
    Failed(String),
}

pub struct Task {
    result: Receiver<Result<Option<Vec<Report>>, String>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl Task {
    pub fn start(job: worker::Job, ctx: &egui::Context) -> Result<Self, String> {
        let (tx, result) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (stop, shown, ctx) = (cancel.clone(), progress.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("lumen-export".into())
            .spawn(move || {
                let answer = worker::run(job, &stop, &mut |p| {
                    shown.store(p.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                    ctx.request_repaint();
                });
                let _ = tx.send(answer);
                ctx.request_repaint();
            })
            .map_err(|e| format!("Could not start the export worker: {e}"))?;
        Ok(Self {
            result,
            cancel,
            progress,
        })
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn poll(&self) -> Poll {
        match self.result.try_recv() {
            Ok(Ok(Some(made))) => Poll::Done(made),
            Ok(Ok(None)) => Poll::Stopped,
            Ok(Err(why)) => Poll::Failed(why),
            Err(TryRecvError::Empty) => {
                Poll::Running(f32::from_bits(self.progress.load(Ordering::Relaxed)))
            }
            Err(TryRecvError::Disconnected) => {
                Poll::Failed("The export worker stopped unexpectedly".into())
            }
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.stop();
    }
}

pub use worker::Job;
