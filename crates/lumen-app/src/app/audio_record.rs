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

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;
use lumen_project::edit::audio_take::Take;
use lumen_project::model::{Id, Media, TrackKind};

use crate::App;
use crate::arrangement::audio_take_look::AudioTakeLook;
use crate::recording::audio_input;
use crate::recording::loop_passes;
use crate::recording::take_notes::{TakeNotes, delete_later};
use crate::recording::take_worker::{Finished, Found, Job, Live, Plan, TrackResult, sidecar_path};

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
    /// Which recording (`Plan::id`).
    id: u64,
    document: u64,
    /// The recording was cut off (another project opened): the files are not
    /// wanted.
    discard: bool,
    since: Instant,
    /// The takes as they were drawn, kept drawn until the clips are placed.
    lanes: Vec<Lane>,
    /// What they recorded over, kept silent until the clips are placed, so
    /// the old audio is not heard there in between.
    replaced: Vec<crate::midi::play::Replacing>,
}

/// A recording given up on after [`FINISH_WAIT`]: its answer may still come
/// (M9 review H6).
struct Late {
    id: u64,
    document: u64,
    discard: bool,
}

/// Recovery's disk work, on a thread (H1).
struct Recovering {
    document: u64,
    dir: PathBuf,
    found: Receiver<(Found, Vec<PathBuf>)>,
}

/// What the app keeps for audio recording.
#[derive(Default)]
pub(crate) struct AudioRecord {
    finishing: Option<Finishing>,
    /// The loss of input has been told for the recording under way.
    warned: bool,
    /// The number of the last recording begun.
    recording: u64,
    late: Vec<Late>,
    recovering: Option<Recovering>,
    /// Sidecars of placed takes, kept until a save holds them (H3).
    pub(crate) notes: TakeNotes,
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

/// Removes takes' files and their sidecars, on a thread of their own.
fn remove(paths: impl IntoIterator<Item = PathBuf>) {
    delete_later(
        paths
            .into_iter()
            .flat_map(|p| [sidecar_path(&p), p])
            .collect(),
    );
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
    /// that is kept, if there is one; `looped` is the loop when Loop is on, so
    /// jumps back are passes and a take begun inside it lines up with it.
    pub(super) fn begin_audio_takes(
        &mut self,
        punch: Option<(Ticks, Ticks)>,
        looped: Option<(Ticks, Ticks)>,
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
            // The worker takes the next free number from this (H1).
            jobs.push(Job {
                track: track.id.0,
                path: dir.join(format!("{} 1.wav", file_stem(&track.name))),
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
        self.record.audio.recording += 1;
        self.transport.audio.takes.begin(Plan {
            id: self.record.audio.recording,
            jobs,
            rate,
            shift: i64::from(info.input_latency) + i64::from(info.output_latency) + offset as i64,
            window,
            looped: looped.map(|(a, b)| (timeline.sample_at(a, rate), timeline.sample_at(b, rate))),
        });
        lanes
    }

    /// After a recovered autosave opens: the takes that session left are
    /// repaired on a thread and put on their tracks, as one step, when they
    /// arrive (`poll_audio_takes`). A take the recovered project already
    /// holds (an autosave was written after it was placed) is not placed
    /// again; a take whose track the project no longer has stays in the
    /// recordings folder.
    pub(crate) fn recover_takes(&mut self, session: &str) {
        let dir = self.recordings_dir();
        let used: Vec<PathBuf> = self.document.media_files.values().cloned().collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let (folder, session) = (dir.clone(), session.to_owned());
        let spawned = std::thread::Builder::new()
            .name("lumen-take-recovery".into())
            .spawn(move || {
                let mut found = crate::recording::take_worker::recover(&folder, &session);
                let canonical = |p: &PathBuf| std::fs::canonicalize(p).ok();
                let used: Vec<PathBuf> = used.iter().filter_map(canonical).collect();
                let (already, takes): (Vec<_>, Vec<_>) = found
                    .takes
                    .into_iter()
                    .partition(|t| canonical(&t.path).is_some_and(|c| used.contains(&c)));
                found.takes = takes;
                let _ = tx.send((found, already.into_iter().map(|t| t.path).collect()));
            });
        if spawned.is_ok() {
            self.record.audio.recovering = Some(Recovering {
                document: self.document.session(),
                dir,
                found: rx,
            });
        }
    }

    /// A normal close: deletes every take's sidecar, waiting for the disk,
    /// as Lumen is exiting.
    pub(crate) fn close_take_notes(&mut self) {
        for sidecar in self.record.audio.notes.close() {
            let _ = std::fs::remove_file(sidecar);
        }
    }

    /// Recovery's takes have been read: places them and says so.
    fn takes_recovered(&mut self, found: Found, already: Vec<PathBuf>, dir: &std::path::Path) {
        let session = self.document.session();
        self.record.audio.notes.hold(session, already);
        let (takes, kept): (Vec<_>, Vec<_>) = found.takes.into_iter().partition(|t| {
            self.document
                .project()
                .tracks
                .iter()
                .any(|track| track.id.0 == t.track && track.kind == TrackKind::Audio)
        });
        let kept = kept.len() + found.unreadable;
        let count = takes.len();
        if count > 0 {
            self.place_audio_takes(&takes);
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
                    // With Loop on, the clip is the loop long from the first
                    // wrap: the pass before shows where this one has not
                    // reached.
                    let loop_frames = live.previous.as_ref().map_or(0, |(f, _)| *f);
                    (live.start?, live.frames.max(loop_frames))
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
    pub(super) fn end_audio_takes(
        &mut self,
        lanes: Vec<Lane>,
        discard: bool,
        replaced: Vec<crate::midi::play::Replacing>,
    ) {
        if lanes.is_empty() {
            return;
        }
        self.transport.audio.takes.finish();
        self.record.audio.finishing = Some(Finishing {
            id: self.record.audio.recording,
            document: self.document.session(),
            discard,
            since: Instant::now(),
            lanes,
            replaced,
        });
    }

    /// What the takes being finished recorded over (see [`Finishing`]).
    pub(super) fn finishing_replaced(&self) -> Vec<crate::midi::play::Replacing> {
        self.record
            .audio
            .finishing
            .as_ref()
            .map_or_else(Vec::new, |f| f.replaced.clone())
    }

    /// Recording has stopped and its takes are not placed yet, or a
    /// recovery's takes are being read.
    pub(crate) fn audio_takes_finishing(&self) -> bool {
        self.record.audio.finishing.is_some() || self.record.audio.recovering.is_some()
    }

    /// The takes to draw while their files are closed: what the arrangement
    /// showed while recording, until the clips are placed.
    pub(super) fn finishing_lanes(&self) -> Vec<Lane> {
        self.record
            .audio
            .finishing
            .as_ref()
            .map_or_else(Vec::new, |f| f.lanes.clone())
    }

    /// Every frame: when the worker has handed over the files of a recording
    /// that ended, puts them on their tracks. Once recording has stopped the
    /// transport is idle, so this asks for the frames it needs itself: to
    /// keep looking while the worker closes the files, and one more after the
    /// clips are placed, which starts loading their audio and waveforms.
    pub(crate) fn poll_audio_takes(&mut self, ctx: &eframe::egui::Context) {
        self.poll_recovered_takes(ctx);
        while let Some(finished) = self.transport.audio.takes.poll() {
            self.takes_finished(finished);
            ctx.request_repaint();
        }
        let Some(finishing) = self.record.audio.finishing.as_ref() else {
            return;
        };
        ctx.request_repaint_after(crate::motion::FADE_FRAME);
        if finishing.since.elapsed() > FINISH_WAIT {
            let Some(given_up) = self.record.audio.finishing.take() else {
                return;
            };
            // Its answer may still come; it is placed then (H6).
            self.record.audio.late.push(Late {
                id: given_up.id,
                document: given_up.document,
                discard: given_up.discard,
            });
            self.file_notice = Some(
                "The recording is taking long to finish. Its audio is placed when the disk is done with it."
                    .into(),
            );
        }
    }

    /// A recording's files have been handed over: placed, if the recording
    /// and its document are still the ones waited for; a late one is placed
    /// when nothing else is recording, and its files are otherwise kept for
    /// recovery.
    fn takes_finished(&mut self, finished: Finished) {
        let Finished { id, results } = finished;
        let (document, discard) = match self.record.audio.finishing.as_ref() {
            Some(f) if f.id == id => {
                let f = self.record.audio.finishing.take().expect("checked above");
                (f.document, f.discard)
            }
            _ => match self.record.audio.late.iter().position(|l| l.id == id) {
                Some(at) => {
                    let late = self.record.audio.late.remove(at);
                    (late.document, late.discard)
                }
                // Not a recording this app waits for: its files stay, with
                // their sidecars, for recovery.
                None => return,
            },
        };
        if discard {
            remove(results.into_iter().map(|r| r.path));
            return;
        }
        if document != self.document.session() {
            // Its project was closed: the files stay in the recordings folder.
            return;
        }
        if self.recording() {
            // A late take while another records: the files stay, with their
            // sidecars, and recovery would find them.
            self.file_notice = Some(format!(
                "A recording that took long to finish could not be placed while recording. Its audio stays in {}.",
                self.recordings_dir().display()
            ));
            return;
        }
        self.place_audio_takes(&results);
    }

    fn poll_recovered_takes(&mut self, ctx: &eframe::egui::Context) {
        let Some(recovering) = self.record.audio.recovering.as_ref() else {
            return;
        };
        let Ok((found, already)) = recovering.found.try_recv() else {
            ctx.request_repaint_after(crate::motion::FADE_FRAME);
            return;
        };
        let Some(recovering) = self.record.audio.recovering.take() else {
            return;
        };
        if recovering.document == self.document.session() {
            self.takes_recovered(found, already, &recovering.dir);
        }
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
                remove([result.path.clone()]);
                continue;
            };
            // One rule makes the clip, with Loop on or off (`loop_passes`), in
            // the loop the recording carries. A take left by a build that did
            // not keep it is cut in the project's loop.
            let looped = result.looped.or_else(|| {
                let region = self.document.project().transport;
                let timeline = &self.transport.timeline;
                (!result.wraps.is_empty()).then(|| {
                    (
                        timeline.sample_at(region.loop_start, rate),
                        timeline.sample_at(region.loop_end, rate),
                    )
                })
            });
            let loop_passes::Cut {
                start,
                frames,
                passes,
                active,
                lead,
            } = loop_passes::cut(
                loop_passes::Recorded {
                    start,
                    silence: result.silence,
                    frames: result.frames,
                    wraps: &result.wraps,
                    looped,
                },
                rate,
            );
            let timeline = &self.transport.timeline;
            let from = timeline.ticks_at(start, rate);
            let to = timeline.ticks_at(start + frames as i64, rate);
            let size = result.size_bytes;
            let media = Media {
                id: self.document.fresh_id(),
                path: result.path.to_string_lossy().into_owned(),
                original_path: None,
                frames: result.frames,
                channels: result.channels,
                sample_rate: result.rate,
                size_bytes: size,
            };
            let name = result
                .path
                .file_stem()
                .map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned());
            // What was recorded before the loop began is a clip of its own,
            // from the start of the file, where it was played.
            if let Some((at, lead_frames)) = lead {
                let lead_from = timeline.ticks_at(at, rate);
                let lead_to = timeline.ticks_at(at + lead_frames as i64, rate);
                takes.push(Take {
                    track: Id(result.track),
                    media: media.clone(),
                    name: format!("{name} lead-in"),
                    start: lead_from,
                    length: lead_to - lead_from,
                    passes: Vec::new(),
                    active: 0,
                    follow: None,
                });
            }
            takes.push(Take {
                track: Id(result.track),
                media,
                name,
                start: from,
                length: to - from,
                passes,
                active,
                follow: None,
            });
        }
        if !takes.is_empty() {
            let placed = self.document.record_audio_takes(&takes);
            if placed.clips.is_empty() {
                remove(takes.iter().map(|t| PathBuf::from(&t.media.path)));
                notes.push(
                    "The track was removed while recording, so the audio was not placed.".into(),
                );
            } else {
                let session = self.document.session();
                let files = takes.iter().map(|t| PathBuf::from(&t.media.path));
                self.record.audio.notes.hold(session, files);
                if !placed.new_tracks.is_empty() {
                    notes.push(
                        "A locked clip was in the way, so the take went on a new track.".into(),
                    );
                }
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
}
