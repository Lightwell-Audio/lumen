//! Consolidate (M7b slice E3): renders the selected audio clips of each track
//! into one new file and replaces them with a clip that plays it, gaps as
//! silence, as one undo step. The render runs on a worker
//! (`consolidate::Render`) with progress in the toast and Esc to stop. The
//! file holds the clips as the clip player plays them: clip gain, polarity,
//! reverse, fades, crossfades and envelopes; not the track's fader, pan,
//! sends or devices.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_project::edit::consolidate::plan;

use crate::App;
use crate::consolidate::{Job, Poll, Render, remove_files};

/// A render under way.
pub(crate) struct Running {
    render: Render,
}

impl App {
    /// Where rendered audio is written until a save copies it into the
    /// project: beside the autosaves, or the system's temporary folder.
    pub(crate) fn renders_dir(&self) -> PathBuf {
        self.autosave
            .as_ref()
            .and_then(|a| a.dir())
            .and_then(|dir| dir.parent())
            .map_or_else(
                || std::env::temp_dir().join("lumen-renders"),
                |dir| dir.join("Renders"),
            )
    }

    /// Starts rendering the selected audio clips (Consolidate).
    pub(crate) fn consolidate_selected_clips(&mut self, ctx: &egui::Context) {
        if self.consolidating.is_some() {
            self.say("Already consolidating".to_owned());
            return;
        }
        let ids = self.selected_clip_ids();
        let parts = plan(self.document.project(), &ids);
        if parts.is_empty() {
            self.say("Select audio clips to consolidate".to_owned());
            return;
        }
        let rate = self.lanes.rate();
        let timeline = self.transport.timeline.clone();
        let playing = self.lanes.engine_clips(self.document.project(), rate);
        let dir = self.renders_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.say(format!("Could not make {}: {e}", dir.display()));
            return;
        }
        let mut jobs = Vec::new();
        for part in parts {
            let clips: Vec<_> = playing
                .iter()
                .filter(|c| part.clips.iter().any(|p| p.id.0 == c.id))
                .cloned()
                .collect();
            if clips.len() != part.clips.len() {
                self.say("Wait for the audio to finish loading, then consolidate".to_owned());
                return;
            }
            let from = timeline.sample_at(part.start, rate);
            let frames = timeline.sample_at(part.end, rate) - from;
            if frames <= 0 {
                self.say("These clips are too short to consolidate".to_owned());
                return;
            }
            let media = self.document.fresh_id();
            jobs.push(Job {
                part,
                clips,
                from,
                frames: frames as u64,
                file: dir.join(format!("consolidated-{}.wav", media.0)),
                media,
            });
        }
        self.consolidating = Some(Running {
            render: Render::start(jobs, timeline, rate, ctx),
        });
    }

    /// Follows the render: shows its progress, stops it on Esc and replaces
    /// the clips when it is done. Call every frame.
    pub(crate) fn poll_consolidate(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.consolidating else {
            return;
        };
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            running.render.stop();
            self.consolidating = None;
            self.say("Consolidate stopped".to_owned());
            return;
        }
        match running.render.poll() {
            Poll::Running(done) => {
                let shown = (done * 100.0).floor() as u32;
                // Keeps the note up while the render goes.
                self.toast = Some((
                    format!("Consolidating {shown}% \u{b7} Esc to stop"),
                    Instant::now(),
                ));
            }
            Poll::Stopped => {
                self.consolidating = None;
                self.say("Consolidate stopped".to_owned());
            }
            Poll::Failed(why) => {
                self.consolidating = None;
                self.say(format!("Could not consolidate: {why}"));
            }
            Poll::Done(baked) => {
                self.consolidating = None;
                let made = self.document.consolidate_clips(&baked);
                if made.is_empty() {
                    remove_files(&baked);
                    self.say("The clips changed while rendering: nothing was replaced".to_owned());
                } else {
                    let left = baked.len() - made.len();
                    self.arrangement.clips = made.into_iter().collect();
                    self.say(if left > 0 {
                        "Consolidated \u{b7} some clips changed while rendering and were left"
                            .to_owned()
                    } else {
                        "Consolidated".to_owned()
                    });
                }
            }
        }
    }
}
