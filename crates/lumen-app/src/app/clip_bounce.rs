//! Bounce (M7b slice E4): renders the selected tracks, with their own sound,
//! into new audio files and puts them on the tracks or on new ones, as one
//! undo step. The popover (a small window, like History) holds the owner's
//! choices (B1 to B3): Replace or New track, Include effects, Include tail,
//! remembered from one bounce to the next. What is bounced (B4) is the time
//! range if there is one, else the selected clips' span, else the whole of
//! each selected track. The render runs on a worker (`bounce::Render`) with
//! progress in the toast and Esc to stop; the plugins are read first so the
//! worker's own hosts start from their states as they are now.

use std::time::Instant;

use eframe::egui::{self, Align2, Key, RichText, pos2};
use lumen_core::time::Ticks;
use lumen_project::edit::bounce::{Baked, Mode, Moved, Options, Part, Scope, plan};
use lumen_project::model::{ClipContent, Id};

use crate::bounce::{Bake, Job, Poll, Render, Rendered, remove_files};
use crate::render_audio::RenderAudio;
use crate::widgets::{self, Choice, Dialog, raised};
use crate::{App, theme};

/// How long the plugins are waited for before a bounce starts with the
/// states it has.
const PLUGIN_READ_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

/// A bounce while the plugins are being read.
pub(crate) struct Waiting {
    parts: Vec<Part>,
    since: Instant,
}

/// A render under way.
pub(crate) struct Running {
    render: Render,
    parts: Vec<Part>,
}

impl App {
    /// What a bounce would take: the tracks, and what of them. A time range
    /// first, then selected clips, then selected tracks.
    fn bounce_scope(&self) -> (Vec<Id>, Option<(Ticks, Ticks)>, Vec<Id>) {
        let project = self.document.project();
        if let Some(range) = &self.arrangement.range {
            return (range.tracks.clone(), Some((range.start, range.end)), vec![]);
        }
        if !self.arrangement.clips.is_empty() {
            let clips: Vec<Id> = self.arrangement.clips.iter().copied().collect();
            let tracks = project
                .tracks
                .iter()
                .filter(|t| t.clips.iter().any(|c| clips.contains(&c.id)))
                .map(|t| t.id)
                .collect();
            return (tracks, None, clips);
        }
        (self.arrangement.selected.clone(), None, vec![])
    }

    fn bounce_parts(&self, options: Options) -> Vec<Part> {
        let (tracks, range, clips) = self.bounce_scope();
        let scope = match range {
            Some((start, end)) => Scope::Range(start, end),
            None if !clips.is_empty() => Scope::Clips(&clips),
            None => Scope::Track,
        };
        plan(self.document.project(), &tracks, scope, options)
    }

    /// The tracks that a bounce would render now.
    pub(crate) fn bounce_targets(&self) -> Vec<Id> {
        let options = self.bounce_options();
        self.bounce_parts(options)
            .into_iter()
            .map(|p| p.track)
            .collect()
    }

    fn bounce_options(&self) -> Options {
        let choices = self.settings.bounce;
        Options {
            mode: if choices.replace {
                Mode::Replace
            } else {
                Mode::NewTrack
            },
            effects: choices.effects,
            tail: choices.tail,
        }
    }

    /// The Bounce command: opens the popover with what will be bounced.
    pub(crate) fn open_bounce(&mut self) {
        if self.bouncing.is_some() || self.bounce_waiting.is_some() {
            self.say("Already bouncing".to_owned());
            return;
        }
        if self.bounce_targets().is_empty() {
            self.say("Nothing to bounce: select a clip, a time range or a track".to_owned());
            return;
        }
        self.show_bounce = true;
    }

    /// The popover. Call every frame.
    pub(crate) fn bounce_window(&mut self, ctx: &egui::Context) {
        if !self.show_bounce {
            return;
        }
        let mut choices = self.settings.bounce;
        let parts = self.bounce_parts(self.bounce_options());
        let names: Vec<(String, Moved)> = parts
            .iter()
            .filter_map(|p| Some((p.planned.name.clone(), p.moved()?)))
            .collect();
        let mut go = false;
        let mut close = Dialog::new("bounce", "Bounce", 300.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                let what = match parts.len() {
                    0 => "Nothing to bounce".to_owned(),
                    1 => format!("Bounces {}", parts[0].planned.name),
                    n => format!("Bounces {n} tracks"),
                };
                ui.label(RichText::new(what).small().weak());
                ui.add_space(theme::space::S);
                let id = egui::Id::new("bounce choices");
                ui.horizontal(|ui| {
                    ui.label("Result");
                    let result = [
                        Choice::new(false, "New track")
                            .named("Put the file on a new track")
                            .tip("The file goes on a new track after this one; this one stays as it is"),
                        Choice::new(true, "Replace")
                            .named("Replace the track's audio with the file")
                            .tip("The file takes the place of what it was made from"),
                    ];
                    if let Some(replace) = widgets::segmented(ui, id.with("mode"), choices.replace, &result) {
                        choices.replace = replace;
                    }
                });
                ui.add_space(theme::space::S);
                widgets::switch(ui, &mut choices.effects, "Include effects", true)
                    .on_hover_text("The track's devices, fader and pan are in the file");
                widgets::switch(ui, &mut choices.tail, "Include tail", true)
                    .on_hover_text("What rings on after the end, such as a reverb, is kept");
                for (name, why) in &names {
                    let why = match why {
                        Moved::Midi => "plays notes, so its sound goes on a new audio track",
                        Moved::PartOfTrack => {
                            "plays more than this, so the effects stay on it and the file goes on a new track"
                        }
                        Moved::Locked => "has a locked clip here, so the file goes on a new track",
                    };
                    ui.label(RichText::new(format!("{name} {why}")).small().weak());
                }
                ui.add_space(theme::space::S);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!parts.is_empty(), |ui| {
                        if raised::primary_button(ui, "Bounce").clicked() {
                            go = true;
                        }
                    });
                });
            });
        if !go && !self.typing && ctx.input(|i| i.key_pressed(Key::Enter)) {
            go = !parts.is_empty();
        }
        if choices != self.settings.bounce {
            self.settings.bounce = choices;
            self.settings_changed = Some(Instant::now());
        }
        if go {
            close = true;
            self.start_bounce(ctx);
        }
        if close {
            self.show_bounce = false;
        }
    }

    /// Starts the bounce: reads the plugins first, when there are any.
    fn start_bounce(&mut self, ctx: &egui::Context) {
        let parts = self.bounce_parts(self.bounce_options());
        if parts.is_empty() {
            self.say("Nothing to bounce there".to_owned());
            return;
        }
        // Audio has to be loaded for the clip player to render it.
        let audio = RenderAudio::of_tracks(
            &self.lanes,
            self.document.project(),
            parts.iter().map(|p| &*p.planned),
        );
        for part in &parts {
            let missing = part.planned.clips.iter().any(|c| {
                c.start < part.end
                    && c.start + c.length > part.start
                    && matches!(c.content, ClipContent::Audio { .. })
                    && !c.muted
                    && !audio.has(c.id)
            });
            if missing {
                self.say("Wait for the audio to finish loading, then bounce".to_owned());
                return;
            }
        }
        if self.hosting.request_captures() {
            self.bounce_waiting = Some(Waiting {
                parts,
                since: Instant::now(),
            });
            ctx.request_repaint();
            return;
        }
        self.launch_bounce(parts, ctx);
    }

    /// Hands the parts to the worker, with the project as the plugins hold it.
    fn launch_bounce(&mut self, parts: Vec<Part>, ctx: &egui::Context) {
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
        for part in &parts {
            let plugins = match self.hosting.offline_starts(
                &project,
                part.track,
                part.effects,
                self.plugins.catalog(),
                rate,
            ) {
                Ok(plugins) => plugins,
                Err(why) => {
                    self.say(format!("Could not bounce {}: {why}", part.planned.name));
                    return;
                }
            };
            let from = timeline.sample_at(part.start, rate);
            let frames = timeline.sample_at(part.end, rate) - from;
            if frames <= 0 {
                self.say("That is too short to bounce".to_owned());
                return;
            }
            let media = self.document.fresh_id();
            jobs.push(Job {
                project: project.clone(),
                track: part.track,
                clips: audio.clips(),
                plugins,
                bake: if part.effects { Bake::Mix } else { Bake::Plain },
                mono: false,
                tail: part.tail,
                timeline: timeline.clone(),
                rate,
                from,
                frames: frames as u64,
                file: dir.join(format!("bounced-{}.wav", media.0)),
                media,
                unread: audio.unread(),
            });
        }
        self.bouncing = Some(Running {
            render: Render::start(jobs, audio.again(), ctx),
            parts,
        });
    }

    /// Follows the bounce: waits for the plugins to be read, shows progress,
    /// stops on Esc, and puts the files in when it is done. Call every frame.
    pub(crate) fn poll_bounce(&mut self, ctx: &egui::Context) {
        if let Some(waiting) = &self.bounce_waiting {
            if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.bounce_waiting = None;
                self.say("Bounce stopped".to_owned());
                return;
            }
            if self.hosting.captures_pending() && waiting.since.elapsed() < PLUGIN_READ_WAIT {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
                return;
            }
            if let Some(waiting) = self.bounce_waiting.take() {
                self.launch_bounce(waiting.parts, ctx);
            }
            return;
        }
        let Some(running) = &self.bouncing else {
            return;
        };
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            running.render.stop();
            self.bouncing = None;
            self.say("Bounce stopped".to_owned());
            return;
        }
        match running.render.poll() {
            Poll::Running(done) => {
                let shown = (done * 100.0).floor() as u32;
                self.toast = Some((
                    format!("Bouncing {shown}% \u{b7} Esc to stop"),
                    Instant::now(),
                ));
            }
            Poll::Stopped => {
                self.bouncing = None;
                self.say("Bounce stopped".to_owned());
            }
            Poll::Failed(why) => {
                self.bouncing = None;
                self.say(format!("Could not bounce: {why}"));
            }
            Poll::Done(made) => {
                let parts = self.bouncing.take().map(|r| r.parts).unwrap_or_default();
                self.finish_bounce(parts, made);
            }
        }
    }

    /// Puts the rendered files into the project as one step, selects the
    /// result, and says what happened.
    fn finish_bounce(&mut self, parts: Vec<Part>, made: Vec<Rendered>) {
        let timeline = self.transport.timeline.clone();
        let baked: Vec<Baked> = made
            .iter()
            .filter_map(|r| {
                // The file's own rate: the project's, not the device's.
                let rate = r.media.sample_rate;
                let part = parts.iter().find(|p| p.track == r.track)?.clone();
                let from = timeline.sample_at(part.end, rate);
                let end = timeline.ticks_at(from + r.tail_frames as i64, rate);
                Some(Baked {
                    part,
                    media: r.media.clone(),
                    end: end.max(parts.iter().find(|p| p.track == r.track)?.end),
                })
            })
            .collect();
        let done = self.document.bounce_tracks(&baked);
        if done.clips.is_empty() {
            remove_files(&made);
            self.say("The tracks changed while bouncing: nothing was changed".to_owned());
            return;
        }
        self.arrangement.range = None;
        self.arrangement.selected.clear();
        self.arrangement.clips = done.clips.into_iter().collect();
        self.arrangement.clip_cursor = None;
        let moved = baked.iter().find_map(|b| b.part.moved());
        self.say(match moved {
            Some(Moved::Midi) => "Bounced to a new audio track".to_owned(),
            Some(_) => "Bounced to a new track: the track plays more than the bounce".to_owned(),
            None => "Bounced".to_owned(),
        });
    }
}
