//! Freeze, unfreeze and flatten (M7b slice E5, ADR-0098). Freeze renders each
//! selected audio and MIDI track, from its first clip to its last with the tail
//! that rings on, with its instrument, devices, trim and polarity (the owner's
//! FR1), into a file the track then plays; the render runs on the bounce worker
//! (`bounce::Render`, `Bake::Sound`) with the plugins read first, progress in
//! the toast and Esc to stop. A frozen track is read-only (FR2, enforced by the
//! history); Unfreeze gives it back exactly, and Flatten (FR3) turns it into a
//! plain audio track holding the file. Tracks whose sound depends on other
//! tracks are refused with the reason (FR4).

use std::time::Instant;

use eframe::egui;
use lumen_project::edit::freeze::{Baked, Part, Refusal, Violation, plan};
use lumen_project::model::{ClipContent, Device, Id, TrackKind};

use crate::App;
use crate::bounce::{Bake, Job, Poll, Render, remove_files};
use crate::render_audio::RenderAudio;

/// How long the plugins are waited for before a freeze starts with the states
/// it has.
const PLUGIN_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

/// A freeze while the plugins are being read.
pub(crate) struct Waiting {
    parts: Vec<Part>,
    refused: Vec<(String, Refusal)>,
    since: Instant,
}

/// A render under way.
pub(crate) struct Running {
    render: Render,
    parts: Vec<Part>,
    refused: Vec<(String, Refusal)>,
    /// Each track's devices with the plugin states the render started from.
    states: Vec<(Id, Vec<Device>)>,
}

/// The refusals as one sentence each, read in full.
fn refusals(refused: &[(String, Refusal)]) -> String {
    refused
        .iter()
        .map(|(name, why)| format!("{name} can't be frozen: {}", why.reason()))
        .collect::<Vec<_>>()
        .join(". ")
}

impl App {
    /// The tracks a track command acts on: the selected tracks, else the
    /// tracks of the selected clips, else the tracks of the time range.
    pub(crate) fn track_targets(&self) -> Vec<Id> {
        if !self.arrangement.selected.is_empty() {
            return self.arrangement.selected.clone();
        }
        if !self.arrangement.clips.is_empty() {
            return self
                .document
                .project()
                .tracks
                .iter()
                .filter(|t| {
                    t.clips
                        .iter()
                        .any(|c| self.arrangement.clips.contains(&c.id))
                })
                .map(|t| t.id)
                .collect();
        }
        self.arrangement
            .range
            .as_ref()
            .map(|r| r.tracks.clone())
            .unwrap_or_default()
    }

    /// Whether any target is a track that Freeze could act on.
    pub(crate) fn can_freeze_targets(&self) -> bool {
        let targets = self.track_targets();
        self.document.project().tracks.iter().any(|t| {
            targets.contains(&t.id)
                && matches!(t.kind, TrackKind::Audio | TrackKind::Midi)
                && t.frozen.is_none()
        })
    }

    /// The targets that are frozen.
    pub(crate) fn frozen_targets(&self) -> Vec<Id> {
        let targets = self.track_targets();
        self.document
            .project()
            .tracks
            .iter()
            .filter(|t| targets.contains(&t.id) && t.frozen.is_some())
            .map(|t| t.id)
            .collect()
    }

    /// Whether a bounce or a freeze is rendering, which a freeze waits for.
    pub(crate) fn rendering_tracks(&self) -> bool {
        self.bouncing.is_some()
            || self.bounce_waiting.is_some()
            || self.freezing.is_some()
            || self.freeze_waiting.is_some()
    }

    /// The Freeze command.
    pub(crate) fn freeze_selected_tracks(&mut self, ctx: &egui::Context) {
        if self.rendering_tracks() {
            self.say(crate::app::commands::wait::FREEZE.to_owned());
            return;
        }
        let targets = self.track_targets();
        let found = plan(self.document.project(), &targets);
        let name = |id: Id| {
            self.document
                .project()
                .tracks
                .iter()
                .find(|t| t.id == id)
                .map_or_else(String::new, |t| t.name.clone())
        };
        let refused: Vec<(String, Refusal)> = found
            .refused
            .iter()
            .map(|&(id, why)| (name(id), why))
            .collect();
        if found.parts.is_empty() {
            self.say(if refused.is_empty() {
                "Select an audio or MIDI track to freeze".to_owned()
            } else {
                refusals(&refused)
            });
            return;
        }
        // Audio has to be loaded for the clip player to render it.
        let audio = RenderAudio::of_tracks(
            &self.lanes,
            self.document.project(),
            found.parts.iter().map(|p| &*p.planned),
        );
        for part in &found.parts {
            let missing = part.planned.clips.iter().any(|c| {
                matches!(c.content, ClipContent::Audio { .. }) && !c.muted && !audio.has(c.id)
            });
            if missing {
                self.say("Wait for the audio to finish loading, then freeze".to_owned());
                return;
            }
        }
        if self.hosting.request_captures() {
            self.freeze_waiting = Some(Waiting {
                parts: found.parts,
                refused,
                since: Instant::now(),
            });
            ctx.request_repaint();
            return;
        }
        self.launch_freeze(found.parts, refused, ctx);
    }

    /// Hands the parts to the worker, with the project as the plugins hold it.
    fn launch_freeze(
        &mut self,
        parts: Vec<Part>,
        refused: Vec<(String, Refusal)>,
        ctx: &egui::Context,
    ) {
        // The files are made at the project's rate; the worker reads the
        // audio again when the device runs at another.
        let audio = RenderAudio::of_tracks(
            &self.lanes,
            self.document.project(),
            parts.iter().map(|p| &*p.planned),
        );
        let rate = audio.rate;
        let timeline = self.transport.timeline.clone();
        let mut project = self.document.project().clone();
        self.hosting.stamp(&mut project);
        let dir = self.renders_dir();
        let mut jobs = Vec::new();
        let mut states = Vec::new();
        for part in &parts {
            let plugins = match self.hosting.offline_starts(
                &project,
                part.track,
                true,
                self.plugins.catalog(),
                rate,
            ) {
                Ok(plugins) => plugins,
                Err(why) => {
                    self.say(format!("Could not freeze {}: {why}", part.planned.name));
                    return;
                }
            };
            let from = timeline.sample_at(part.start, rate);
            let frames = timeline.sample_at(part.end, rate) - from;
            if frames <= 0 {
                self.say(format!("{} is too short to freeze", part.planned.name));
                return;
            }
            if let Some(track) = project.tracks.iter().find(|t| t.id == part.track) {
                states.push((part.track, track.devices.clone()));
            }
            let media = self.document.fresh_id();
            jobs.push(Job {
                project: project.clone(),
                track: part.track,
                clips: audio.clips(),
                plugins,
                bake: Bake::Sound,
                mono: part.mono,
                tail: true,
                timeline: timeline.clone(),
                rate,
                from,
                frames: frames as u64,
                file: dir.join(format!("frozen-{}.wav", media.0)),
                media,
            });
        }
        self.freezing = Some(Running {
            render: Render::start(jobs, audio.again(), ctx),
            parts,
            refused,
            states,
        });
    }

    /// Follows the freeze: waits for the plugins to be read, shows progress,
    /// stops on Esc, and freezes the tracks when it is done. Call every frame.
    pub(crate) fn poll_freeze(&mut self, ctx: &egui::Context) {
        if let Some(waiting) = &self.freeze_waiting {
            if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.freeze_waiting = None;
                self.say("Freeze stopped".to_owned());
                return;
            }
            if self.hosting.captures_pending() && waiting.since.elapsed() < PLUGIN_READ_WAIT {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
                return;
            }
            if let Some(waiting) = self.freeze_waiting.take() {
                self.launch_freeze(waiting.parts, waiting.refused, ctx);
            }
            return;
        }
        let Some(running) = &self.freezing else {
            return;
        };
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            running.render.stop();
            self.freezing = None;
            self.say("Freeze stopped".to_owned());
            return;
        }
        match running.render.poll() {
            Poll::Running(done) => {
                let shown = (done * 100.0).floor() as u32;
                self.toast = Some((
                    format!("Freezing {shown}% \u{b7} Esc to stop"),
                    Instant::now(),
                ));
            }
            Poll::Stopped => {
                self.freezing = None;
                self.say("Freeze stopped".to_owned());
            }
            Poll::Failed(why) => {
                self.freezing = None;
                self.say(format!("Could not freeze: {why}"));
            }
            Poll::Done(made) => {
                if let Some(running) = self.freezing.take() {
                    self.finish_freeze(running, made);
                }
            }
        }
    }

    /// Freezes the tracks with their files as one step, and says what
    /// happened.
    fn finish_freeze(&mut self, running: Running, made: Vec<crate::bounce::Rendered>) {
        let Running {
            parts,
            refused,
            states,
            ..
        } = running;
        let timeline = self.transport.timeline.clone();
        let baked: Vec<Baked> = made
            .iter()
            .filter_map(|r| {
                // The file's own rate: the project's, not the device's.
                let rate = r.media.sample_rate;
                let part = parts.iter().find(|p| p.track == r.track)?.clone();
                let from = timeline.sample_at(part.end, rate);
                let end = timeline.ticks_at(from + r.tail_frames as i64, rate);
                let devices = states
                    .iter()
                    .find(|(t, _)| *t == r.track)
                    .map(|(_, d)| d.clone())
                    .unwrap_or_default();
                Some(Baked {
                    end: end.max(part.end),
                    part,
                    media: r.media.clone(),
                    devices,
                })
            })
            .collect();
        let done = self.document.freeze_tracks(&baked);
        if done.is_empty() {
            remove_files(&made);
            self.say("The tracks changed while freezing: nothing was changed".to_owned());
            return;
        }
        // The files of tracks that changed meanwhile are not kept.
        let kept: Vec<_> = made
            .iter()
            .filter(|r| !done.contains(&r.track))
            .cloned()
            .collect();
        remove_files(&kept);
        let mut words = match done.len() {
            1 => format!("Froze {}", self.track_name(done[0])),
            n => format!("Froze {n} tracks"),
        };
        if !refused.is_empty() {
            words = format!("{words}. {}", refusals(&refused));
        }
        self.say(words);
    }

    pub(crate) fn track_name(&self, id: Id) -> String {
        self.document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(String::new, |t| t.name.clone())
    }

    /// The Unfreeze command.
    pub(crate) fn unfreeze_selected_tracks(&mut self) {
        let targets = self.frozen_targets();
        let done = self.document.unfreeze_tracks(&targets);
        self.say(match done.len() {
            0 => "No frozen track is selected".to_owned(),
            1 => format!("Unfroze {}", self.track_name(done[0])),
            n => format!("Unfroze {n} tracks"),
        });
    }

    /// The Flatten command.
    pub(crate) fn flatten_selected_tracks(&mut self) {
        let targets = self.frozen_targets();
        let done = self.document.flatten_tracks(&targets);
        self.say(match done.len() {
            0 => "Flatten works on a frozen track: freeze it first".to_owned(),
            1 => format!("Flattened {} to audio", self.track_name(done[0])),
            n => format!("Flattened {n} tracks to audio"),
        });
    }

    /// Says why an edit did nothing when it tried to change a frozen track
    /// (the owner's FR2). Call every frame.
    pub(crate) fn say_frozen_refusals(&mut self) {
        let refused = self.document.take_refused();
        if refused.is_empty() {
            return;
        }
        let names: Vec<String> = refused
            .iter()
            .filter_map(|v| match v {
                Violation::Track(id) => Some(self.track_name(*id)),
                Violation::Timing | Violation::Recording(_) => None,
            })
            .collect();
        let recording: Vec<String> = refused
            .iter()
            .filter_map(|v| match v {
                Violation::Recording(id) => Some(self.track_name(*id)),
                _ => None,
            })
            .collect();
        let timing = refused.contains(&Violation::Timing);
        let mut sentences = Vec::new();
        if !recording.is_empty() {
            sentences.push(format!(
                "{} is recording, so its clips can't be changed until the recording stops \
                 and nothing was",
                recording.join(", ")
            ));
        }
        if !names.is_empty() {
            sentences.push(format!(
                "{} is frozen, so its clips, notes and devices can't be changed and nothing was. \
                 Click the lit Frozen badge on its header (or use Unfreeze track) to unfreeze it",
                names.join(", ")
            ));
        }
        if timing {
            sentences.push(
                "Tempo and time signature can't change while a track is frozen, \
                 as its file would stay behind the music. Unfreeze the frozen tracks first"
                    .to_owned(),
            );
        }
        self.say(sentences.join(". "));
    }
}
