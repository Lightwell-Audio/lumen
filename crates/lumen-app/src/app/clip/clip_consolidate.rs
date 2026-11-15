use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use lumen_project::edit::consolidate::{Baked, midi_clips, plan};
use lumen_project::model::Id;

use crate::App;
use crate::consolidate::{Job, Poll, Render, remove_files};
use crate::export::render_audio::RenderAudio;

pub(crate) struct Running {
    render: Render,
    midi: Vec<Id>,
}

impl App {
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

    pub(crate) fn consolidate_selected_clips(&mut self, ctx: &egui::Context) {
        if self.consolidating.is_some() {
            self.say("Already joining".to_owned());
            return;
        }
        let ids = self.selected_clip_ids();
        let project = self.document.project();
        let parts = plan(project, &ids);
        let midi = midi_clips(project, &ids);
        if parts.is_empty() && midi.is_empty() {
            self.say("Select clips to join".to_owned());
            return;
        }
        if parts.is_empty() {
            self.finish_consolidation(&[], &midi);
            return;
        }
        let audio = RenderAudio::new(
            &self.lanes,
            self.document.project(),
            parts.iter().flat_map(|p| p.clips.iter().map(|c| &**c)),
        );
        let rate = audio.rate;
        let timeline = self.transport.timeline.clone();
        let dir = self.renders_dir();
        let mut jobs = Vec::new();
        for part in parts {
            if !part.clips.iter().all(|c| audio.has(c.id)) {
                self.say("Wait for the audio to finish loading, then join".to_owned());
                return;
            }
            let clips: Vec<_> = audio
                .clips()
                .into_iter()
                .filter(|c| part.clips.iter().any(|p| p.id.0 == c.id))
                .collect();
            let from = timeline.sample_at(part.start, rate);
            let frames = timeline.sample_at(part.end, rate) - from;
            if frames <= 0 {
                self.say("These clips are too short to join".to_owned());
                return;
            }
            let media = self.document.fresh_id();
            jobs.push(Job {
                part,
                clips,
                from,
                frames: frames as u64,
                file: dir.join(format!("joined-{}.wav", media.0)),
                media,
                unread: audio.unread(),
            });
        }
        self.consolidating = Some(Running {
            render: Render::start(jobs, timeline, rate, audio.again(), ctx),
            midi,
        });
    }

    pub(crate) fn poll_consolidate(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.consolidating else {
            return;
        };
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            running.render.stop();
            self.consolidating = None;
            self.say("Join stopped".to_owned());
            return;
        }
        match running.render.poll() {
            Poll::Running(done) => {
                let shown = (done * 100.0).floor() as u32;
                self.toast = Some((
                    format!("Joining {shown}% \u{b7} Esc to stop"),
                    Instant::now(),
                ));
            }
            Poll::Stopped => {
                self.consolidating = None;
                self.say("Join stopped".to_owned());
            }
            Poll::Failed(why) => {
                self.consolidating = None;
                self.say(format!("Could not join: {why}"));
            }
            Poll::Done(baked) => {
                let midi = self
                    .consolidating
                    .take()
                    .map(|r| r.midi)
                    .unwrap_or_default();
                self.finish_consolidation(&baked, &midi);
            }
        }
    }

    fn finish_consolidation(&mut self, baked: &[Baked], midi: &[Id]) {
        let made = self.document.consolidate_clips(baked, midi);
        if made.is_empty() {
            remove_files(baked);
            self.say(if baked.is_empty() {
                "Nothing to join: each track already has one clip".to_owned()
            } else {
                "The clips changed while joining: nothing was replaced".to_owned()
            });
            return;
        }
        self.arrangement.clips = made.into_iter().collect();
        self.arrangement.clip_cursor = None;
        self.say("Joined".to_owned());
    }
}
