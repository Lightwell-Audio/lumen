//! Render, encode, measure, then publish. All disk and plugin work stays on
//! this worker; each stem starts with fresh hosts and the same snapshot.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use lumen_core::loudness::{Blocks, Loudness, TruePeak};
use lumen_engine::Clip;
use lumen_media::MediaError;
use lumen_project::model::{Id, Project};

use super::publish::Stage;
use super::{Buses, Format, Options, Report};
use crate::export_live;
use crate::export_render;
use crate::hosting::OfflinePlugin;
use crate::media::streams::{self, Unread};
use crate::render_audio::Again;

pub struct Job {
    pub project: Project,
    pub clips: Vec<Clip>,
    pub again: Option<Arc<Again>>,
    pub plugins: Vec<OfflinePlugin>,
    pub rate: u32,
    pub from: i64,
    pub frames: u64,
    pub options: Options,
    pub destination: PathBuf,
    pub targets: Vec<(Option<Id>, String)>,
    pub live: Option<Arc<Mutex<export_live::Handle>>>,
    /// Existing project and media files may never be export destinations.
    pub protected: Vec<PathBuf>,
    /// What the clips could not read from disk; an export with a hole fails.
    pub unread: Unread,
}

fn safe_name(name: &str) -> String {
    let name: String = name
        .chars()
        .take(80)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        "Untitled".into()
    } else {
        name.into()
    }
}

fn destinations(job: &Job) -> Result<Vec<PathBuf>, String> {
    let paths: Vec<_> = if job.options.buses == Buses::Master {
        vec![
            job.destination
                .with_extension(job.options.format.extension()),
        ]
    } else {
        job.targets
            .iter()
            .enumerate()
            .map(|(i, (_, name))| {
                job.destination.join(format!(
                    "{:02} {}.{}",
                    i + 1,
                    safe_name(name),
                    job.options.format.extension()
                ))
            })
            .collect()
    };
    for path in &paths {
        if let Ok(canonical) = std::fs::canonicalize(path) {
            if job
                .protected
                .iter()
                .any(|p| std::fs::canonicalize(p).is_ok_and(|p| p == canonical))
            {
                return Err(format!(
                    "{} belongs to the project. Choose a separate export file",
                    path.display()
                ));
            }
            if job.options.buses != Buses::Master {
                return Err(format!(
                    "{} already exists. Choose a new folder for these stems",
                    path.display()
                ));
            }
        }
    }
    Ok(paths)
}

fn encoding(options: Options) -> lumen_media::export::Spec {
    use lumen_media::export::{Format as F, Spec};
    Spec {
        sample_rate: options.rate,
        format: match options.format {
            Format::Wav16 => F::WavI16,
            Format::Wav24 => F::WavI24,
            Format::WavFloat => F::WavF32,
            Format::Flac16 => F::FlacI16,
            Format::Flac24 => F::FlacI24,
        },
        normalize_peak: options.normalize,
        tpdf_dither: options.dither,
    }
}

/// The loudness and true peak of an exported file, read block by block so
/// the file is never held whole (M9 slice R6).
fn measure(path: &Path, cancel: &AtomicBool) -> Result<Option<Report>, String> {
    let mut found: Option<(u32, Blocks, Loudness, [TruePeak; 2])> = None;
    let mut peak = 0f32;
    let mut frames = 0u64;
    let read = lumen_media::read_blocks(path, cancel, &mut |rate, parts| {
        let (_, blocks, loudness, peaks) = found.get_or_insert_with(|| {
            (
                rate,
                Blocks::new(rate),
                Loudness::new(),
                [TruePeak::new(), TruePeak::new()],
            )
        });
        let (left, right) = (parts[0], parts.get(1).copied().unwrap_or(parts[0]));
        for from in (0..left.len()).step_by(4096) {
            let end = (from + 4096).min(left.len());
            let (left, right) = (&left[from..end], &right[from..end]);
            blocks.add(left, right, |mean| loudness.push(mean));
            peak = peak
                .max(peaks[0].process(left))
                .max(peaks[1].process(right));
            peak = left.iter().chain(right).fold(peak, |p, x| p.max(x.abs()));
        }
        frames += left.len() as u64;
        Ok(())
    });
    match read {
        Ok(()) => {}
        Err(MediaError::Cancelled) => return Ok(None),
        Err(why) => return Err(why.to_string()),
    }
    let Some((rate, _, loudness, mut peaks)) = found else {
        return Err(MediaError::Empty.to_string());
    };
    // Flush the finite interpolation filter so the final samples count.
    for p in &mut peaks {
        peak = peak.max(p.process(&[0.0; 64]));
    }
    Ok(Some(Report {
        path: path.into(),
        frames,
        rate,
        lufs: loudness.integrated(),
        true_peak: peak,
    }))
}

fn render(
    job: export_render::Job,
    live: Option<&Arc<Mutex<export_live::Handle>>>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<bool, String> {
    let Some(live) = live else {
        return export_render::render_job(job, cancel, progress).map(|made| made.is_some());
    };
    let Some(prepared) = export_render::prepare(&job, cancel)? else {
        return Ok(false);
    };
    let export_render::Prepared {
        source,
        manager,
        statuses,
        timeline,
        rate,
    } = prepared;
    let result = live
        .lock()
        .map_err(|_| "The real-time export controller failed".to_owned())?
        .capture(
            export_live::Request {
                source,
                statuses,
                timeline,
                rate,
                from: job.from,
                frames: job.frames,
                keepalive: Box::new(manager),
                most_tail: if job.tail { u64::from(rate) * 10 } else { 0 },
                file: job.file,
            },
            cancel,
            progress,
        );
    result.map(|made| made.is_some())
}

pub(super) fn run(
    mut job: Job,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<Vec<Report>>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let destinations = destinations(&job)?;
    if destinations.is_empty() {
        return Err("There are no buses to export".into());
    }
    let parent = if job.options.buses == Buses::Master {
        destinations[0]
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    } else {
        &job.destination
    };
    let stage = Stage::new(parent)?;
    if let Some(again) = &job.again {
        let Some(clips) = again.clips(cancel)? else {
            return Ok(None);
        };
        job.clips = clips;
    }
    if job.live.is_some() {
        // The audio callback plays a real-time export: nothing it plays may
        // read the disk, so streamed files are read into memory first.
        job.clips = streams::in_memory(std::mem::take(&mut job.clips))?;
    }
    let count = job.targets.len().max(1) as f32;
    let mut reports = Vec::new();
    let mut publishing = Vec::new();
    for (index, ((stem, _), destination)) in job.targets.iter().zip(destinations).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let scratch = stage.0.join(format!("render-{index}.wav"));
        let render_job = export_render::Job {
            project: job.project.clone(),
            clips: job.clips.clone(),
            plugins: job
                .plugins
                .iter()
                .map(|p| OfflinePlugin {
                    device: p.device,
                    name: p.name.clone(),
                    instrument: p.instrument,
                    start: p.start.clone(),
                })
                .collect(),
            rate: job.rate,
            from: job.from,
            frames: job.frames,
            tail: job.options.tail,
            file: scratch.clone(),
            stem: *stem,
        };
        let before = index as f32;
        if !render(render_job, job.live.as_ref(), cancel, &mut |p| {
            progress((before + p * 0.65) / count)
        })? {
            return Ok(None);
        }
        job.unread.check()?;
        let encoded = stage.0.join(format!(
            "encoded-{index}.{}",
            job.options.format.extension()
        ));
        match lumen_media::export::encode(
            &scratch,
            &encoded,
            encoding(job.options),
            cancel,
            &mut |p| progress((before + 0.65 + p * 0.3) / count),
        ) {
            Ok(_) => {}
            Err(MediaError::Cancelled) => return Ok(None),
            Err(why) => return Err(why.to_string()),
        }
        let Some(mut report) = measure(&encoded, cancel)? else {
            return Ok(None);
        };
        report.path = destination.clone();
        reports.push(report);
        publishing.push((encoded, destination));
        std::fs::remove_file(&scratch).map_err(|e| e.to_string())?;
        progress((before + 1.0) / count);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    stage.publish(&publishing)?;
    progress(1.0);
    Ok(Some(reports))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_names_cannot_escape_the_chosen_folder_or_end_in_a_dot() {
        assert_eq!(safe_name("../../a\\b: *?<>|\n."), "_.._a_b_ ______");
        assert_eq!(safe_name(" ... "), "Untitled");
    }
}
