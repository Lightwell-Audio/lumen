//! Recording audio (M9 slice R2, ADR-0102): the audio half of Record.
//!
//! Record (F9) records the armed MIDI tracks as before and, at the same time,
//! the armed audio tracks that have an input. For those the engine keeps what
//! arrives (`lumen_engine::capture`), the take worker writes one file per
//! track (`take_worker.rs`), and when recording ends the files become clips
//! over the span they were played in, as one undo step.
//!
//! The files live in a `Recordings` folder beside the autosaves, so a crash
//! leaves them where recovery finds them, and Save copies them into the
//! project (`Samples/Recorded`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;
use lumen_project::edit::audio_take::Take;
use lumen_project::model::{Id, Media, TrackKind};

use crate::App;
use crate::arrangement::audio_take_look::AudioTakeLook;
use crate::audio_input;
use crate::loop_passes;
use crate::take_worker::{Job, Live, Plan, TrackResult, sidecar_path, unfinished};

/// How long the worker has to hand the files over after recording ends
/// before the take is given up on.
const FINISH_WAIT: Duration = Duration::from_secs(20);

/// An armed audio track that records, and what its take looks like so far.
#[derive(Debug, Clone)]
pub(crate) struct Lane {
    pub track: Id,
    pub live: Arc<Mutex<Live>>,
}

/// The files of a recording that has ended, waited for.
struct Finishing {
    document: u64,
    /// The recording was cut off (another project opened): the files are not
    /// wanted.
    discard: bool,
    since: Instant,
}

/// What the app keeps for audio recording.
#[derive(Default)]
pub(crate) struct AudioRecord {
    finishing: Option<Finishing>,
    /// The loss of input has been told for the recording under way.
    warned: bool,
}

/// A track's name as part of a file name.
fn file_stem(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.').trim();
    if clean.is_empty() {
        "Audio".to_owned()
    } else {
        clean.chars().take(60).collect()
    }
}

/// The first `<name> <n>.wav` in `dir` that no earlier take uses.
fn free_file(dir: &Path, name: &str, taken: &[PathBuf]) -> PathBuf {
    let stem = file_stem(name);
    (1..)
        .map(|n| dir.join(format!("{stem} {n}.wav")))
        .find(|p| !p.exists() && !sidecar_path(p).exists() && !taken.contains(p))
        .expect("an unused take name exists")
}

/// Removes a take's file and its sidecar.
fn remove(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(sidecar_path(path));
}

impl App {
    /// Where recorded audio is written until a save copies it into the
    /// project: beside the autosaves, or the system's temporary folder.
    pub(crate) fn recordings_dir(&self) -> PathBuf {
        self.autosave
            .as_ref()
            .and_then(|a| a.dir())
            .and_then(|dir| dir.parent())
            .map_or_else(
                || std::env::temp_dir().join("lumen-recordings"),
                |dir| dir.join("Recordings"),
            )
    }

    /// The armed audio tracks that can record, with the worker told what to
    /// write. Tracks that cannot are named in a notice. `punch` is the range
    /// that is kept, if there is one; `looped` says Loop is on, so jumps back are
    /// passes.
    pub(super) fn begin_audio_takes(
        &mut self,
        punch: Option<(Ticks, Ticks)>,
        looped: bool,
    ) -> Vec<Lane> {
        let project = self.document.project();
        let armed: Vec<_> = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && t.armed && t.frozen.is_none())
            .collect();
        if armed.is_empty() {
            return Vec::new();
        }
        let open = self.transport.audio.open_inputs();
        let routes = audio_input::routes(project, &open);
        let Some(info) = self.transport.audio.info.clone() else {
            self.file_notice =
                Some("There is no audio device running, so audio cannot be recorded.".into());
            return Vec::new();
        };
        let dir = self.recordings_dir();
        let session = self
            .autosave
            .as_ref()
            .map_or_else(String::new, |a| a.session().to_owned());
        let mut jobs: Vec<Job> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for track in armed {
            let Some(route) = routes.iter().find(|r| r.track == track.id.0) else {
                skipped.push(match audio_input::unavailable(track, &open) {
                    Some(why) => format!("{}: {}", track.name, why.to_lowercase()),
                    None => format!("{} has no input", track.name),
                });
                continue;
            };
            let taken: Vec<PathBuf> = jobs.iter().map(|j| j.path.clone()).collect();
            jobs.push(Job {
                track: track.id.0,
                path: free_file(&dir, &track.name, &taken),
                channels: if route.stereo { 2 } else { 1 },
                live: Arc::default(),
                session: session.clone(),
            });
        }
        if !skipped.is_empty() {
            self.file_notice = Some(format!("Not recorded: {}.", skipped.join("; ")));
        }
        if jobs.is_empty() {
            return Vec::new();
        }
        let rate = info.sample_rate;
        let timeline = &self.transport.timeline;
        let offset = (f64::from(self.settings.record.offset_ms) * f64::from(rate) / 1000.0).round();
        let window = punch.map(|(a, b)| {
            let end = if b >= Ticks(i64::MAX / 8) {
                i64::MAX
            } else {
                timeline.sample_at(b, rate)
            };
            (timeline.sample_at(a, rate).max(0), end)
        });
        let lanes = jobs
            .iter()
            .map(|job| Lane {
                track: Id(job.track),
                live: Arc::clone(&job.live),
            })
            .collect();
        self.record.audio.warned = false;
        self.transport.audio.takes.begin(Plan {
            jobs,
            rate,
            shift: i64::from(info.input_latency) + i64::from(info.output_latency) + offset as i64,
            window,
            looped,
        });
        lanes
    }

    /// After a recovered autosave opens: the takes that session left
    /// unfinished are repaired and put on their tracks, as one step. A take
    /// whose track the recovered project no longer has stays in the
    /// recordings folder.
    pub(crate) fn recover_takes(&mut self, session: &str) {
        let dir = self.recordings_dir();
        let mut results = Vec::new();
        let mut kept = 0;
        for lost in unfinished(&dir, session) {
            let track = lost.sidecar.track;
            let has_track = self
                .document
                .project()
                .tracks
                .iter()
                .any(|t| t.id.0 == track && t.kind == TrackKind::Audio);
            let repaired = lumen_media::take::repair(&lost.file).ok();
            match repaired {
                Some(r) if r.frames > 0 && has_track => results.push(TrackResult {
                    track,
                    path: lost.file.clone(),
                    rate: r.sample_rate,
                    channels: r.channels,
                    start: Some(lost.sidecar.start),
                    frames: r.frames,
                    dropped: 0,
                    error: None,
                    wraps: lost.sidecar.wraps.clone(),
                }),
                Some(r) if r.frames == 0 => remove(&lost.file),
                _ => {
                    kept += 1;
                    continue;
                }
            }
            let _ = fs::remove_file(sidecar_path(&lost.file));
        }
        let count = results.len();
        if count > 0 {
            self.place_audio_takes(&results);
        }
        let mut words = Vec::new();
        if count > 0 {
            words.push(format!(
                "{PRODUCT_NAME} stopped while recording. {count} recording{} placed on {} track{}, as far as {} written.",
                if count == 1 { " was" } else { "s were" },
                if count == 1 { "its" } else { "their" },
                if count == 1 { "" } else { "s" },
                if count == 1 { "it was" } else { "they were" },
            ));
        }
        if kept > 0 {
            words.push(format!(
                "{kept} unfinished recording{} could not be placed and stay in {}.",
                if kept == 1 { "" } else { "s" },
                dir.display()
            ));
        }
        if !words.is_empty() {
            let own = self
                .file_notice
                .take()
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            self.file_notice = Some(format!("{}{own}", words.join(" ")));
        }
    }

    /// While recording: says so, once, if the disk cannot keep up and input
    /// is being lost, so the player can stop rather than find out at the end.
    pub(super) fn watch_audio_takes(&mut self, lanes: &[Lane]) {
        if lanes.is_empty() || self.record.audio.warned || self.transport.audio.takes.dropped() == 0
        {
            return;
        }
        self.record.audio.warned = true;
        self.file_notice = Some(
            "The disk cannot keep up with the recording, so some input is being lost. Stop and record to a faster drive, or arm fewer tracks."
                .into(),
        );
    }

    /// What the arrangement draws of the takes under way: each as far as
    /// the worker has written it.
    pub(super) fn audio_take_looks(&self, lanes: &[Lane]) -> Vec<AudioTakeLook> {
        let Some(rate) = self.transport.audio.info.as_ref().map(|i| i.sample_rate) else {
            return Vec::new();
        };
        let timeline = &self.transport.timeline;
        lanes
            .iter()
            .filter_map(|lane| {
                let (start, frames) = {
                    let live = lane.live.lock().ok()?;
                    (live.start?, live.frames)
                };
                Some(AudioTakeLook {
                    track: lane.track,
                    from: timeline.ticks_at(start, rate),
                    to: timeline.ticks_at(start + frames as i64, rate),
                    live: Arc::clone(&lane.live),
                })
            })
            .collect()
    }

    /// Recording has ended: the worker closes the files and
    /// [`Self::poll_audio_takes`] puts them in. With `discard` they are
    /// thrown away instead.
    pub(super) fn end_audio_takes(&mut self, lanes: Vec<Lane>, discard: bool) {
        if lanes.is_empty() {
            return;
        }
        self.transport.audio.takes.finish();
        self.record.audio.finishing = Some(Finishing {
            document: self.document.session(),
            discard,
            since: Instant::now(),
        });
    }

    /// Every frame: when the worker has handed over the files of a recording
    /// that ended, puts them on their tracks. Once recording has stopped the
    /// transport is idle, so this asks for the frames it needs itself: to
    /// keep looking while the worker closes the files, and one more after the
    /// clips are placed, which starts loading their audio and waveforms.
    pub(crate) fn poll_audio_takes(&mut self, ctx: &eframe::egui::Context) {
        let Some(finishing) = self.record.audio.finishing.as_ref() else {
            return;
        };
        let Some(results) = self.transport.audio.takes.poll() else {
            ctx.request_repaint_after(Duration::from_millis(16));
            if finishing.since.elapsed() > FINISH_WAIT {
                self.record.audio.finishing = None;
                self.file_notice = Some(
                    "The recording could not be finished, so the audio was not placed.".into(),
                );
            }
            return;
        };
        let Some(finishing) = self.record.audio.finishing.take() else {
            return;
        };
        if finishing.discard || finishing.document != self.document.session() {
            for result in &results {
                remove(&result.path);
            }
            return;
        }
        self.place_audio_takes(&results);
        ctx.request_repaint();
    }

    fn place_audio_takes(&mut self, results: &[TrackResult]) {
        let mut takes = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        for result in results {
            let name = self
                .document
                .project()
                .tracks
                .iter()
                .find(|t| t.id.0 == result.track)
                .map_or_else(|| "a track".to_owned(), |t| t.name.clone());
            let rate = result.rate.max(1);
            if let Some(why) = &result.error {
                notes.push(format!(
                    "Recording on {name} stopped early: {why} What was recorded is kept."
                ));
            }
            if result.dropped > 0 {
                notes.push(format!(
                    "{} ms of input on {name} was lost because the disk was too slow; silence fills the gap.",
                    result.dropped * 1000 / u64::from(rate)
                ));
            }
            let Some(start) = result.start else {
                remove(&result.path);
                continue;
            };
            // With Loop on and two or more whole passes, the clip is the last
            // pass over the loop and the others are its takes.
            let cut = loop_passes::cut(start, result.frames, &result.wraps, rate);
            let (start, frames, passes) = match cut {
                Some(c) => (c.start, c.frames, c.passes),
                None => (start, result.frames, Vec::new()),
            };
            let timeline = &self.transport.timeline;
            let from = timeline.ticks_at(start, rate);
            let to = timeline.ticks_at(start + frames as i64, rate);
            let size = fs::metadata(&result.path).map_or(0, |m| m.len());
            takes.push(Take {
                track: Id(result.track),
                media: Media {
                    id: self.document.fresh_id(),
                    path: result.path.to_string_lossy().into_owned(),
                    original_path: None,
                    frames: result.frames,
                    channels: result.channels,
                    sample_rate: result.rate,
                    size_bytes: size,
                },
                name: result
                    .path
                    .file_stem()
                    .map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned()),
                start: from,
                length: to - from,
                passes,
            });
        }
        if !takes.is_empty() {
            let placed = self.document.record_audio_takes(&takes);
            if placed.clips.is_empty() {
                for take in &takes {
                    remove(Path::new(&take.media.path));
                }
                notes.push(
                    "The track was removed while recording, so the audio was not placed.".into(),
                );
            } else if !placed.new_tracks.is_empty() {
                notes.push("A locked clip was in the way, so the take went on a new track.".into());
            }
        }
        if !notes.is_empty() {
            self.file_notice = Some(notes.join(" "));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_drops_what_a_path_cannot_hold() {
        assert_eq!(file_stem("Vox"), "Vox");
        assert_eq!(file_stem("Lead: A/B?"), "Lead- A-B-");
        assert_eq!(file_stem("  .. "), "Audio");
        assert_eq!(file_stem(""), "Audio");
        assert_eq!(file_stem(&"x".repeat(200)).len(), 60);
    }

    #[test]
    fn the_next_free_take_name_skips_files_sidecars_and_names_taken_now() {
        let dir = std::env::temp_dir().join(format!("lumen-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_file(&dir, "Vox", &[]), dir.join("Vox 1.wav"));
        fs::write(dir.join("Vox 1.wav"), b"x").unwrap();
        fs::write(sidecar_path(&dir.join("Vox 2.wav")), b"x").unwrap();
        let now = [dir.join("Vox 3.wav")];
        assert_eq!(free_file(&dir, "Vox", &now), dir.join("Vox 4.wav"));
        fs::remove_dir_all(dir).unwrap();
    }
}
