//! Audio for the project's clips (M3 slices 3–4): files loading for import,
//! and the engine's clip list derived from the project. Drawing is in
//! `arrangement.rs`.
//!
//! Dropped files load in the background as placeholders that are not part
//! of the project. A file joins the project, as one undo step, once it has
//! loaded and its exact length is known; a failed or cancelled load never
//! touches it. The engine's clip list is derived from the project after
//! every change, undo and redo.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Pos2, RichText};
use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipsHandle};
use lumen_project::model::{ClipContent, Id, Project};

use crate::document::{Document, Imported};
use crate::media::{self, Item, Library, State};
use crate::theme;

#[derive(Default)]
pub struct Lanes {
    pub library: Library,
    /// Files loading for import: library id and where they were dropped.
    pending: Vec<(u64, Ticks)>,
    /// The project state the engine's clip list was built from.
    built: Option<u64>,
    /// The engine's clip list needs rebuilding.
    dirty: bool,
    /// A message about files that could not be added.
    notice: Option<String>,
    /// When each clip was first drawn (egui time), for its fade-in.
    seen: HashMap<u64, f64>,
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Lanes {
    /// Files are still loading for import.
    pub fn is_loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Files loading for import: library id and where they were dropped.
    pub fn pending(&self) -> &[(u64, Ticks)] {
        &self.pending
    }

    /// How far a clip has faded in (motion: BASE), by a stable key (the
    /// media id for audio, so a loading placeholder that becomes its clip
    /// does not fade in twice).
    pub fn shown(&mut self, key: u64, ctx: &egui::Context) -> f32 {
        let now = ctx.input(|i| i.time);
        let since = *self.seen.entry(key).or_insert(now);
        crate::motion::appear(ctx, since)
    }

    pub fn is_empty(&self, project: &Project) -> bool {
        project.tracks.is_empty() && self.pending.is_empty()
    }

    /// Starts loading each audio file, to be added at `at`; anything else is
    /// reported, not added.
    pub fn add_files(
        &mut self,
        paths: Vec<PathBuf>,
        at: Ticks,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
    ) {
        let mut rejected = Vec::new();
        for path in paths {
            if media::is_audio_file(&path) {
                let id = doc.fresh_id().0;
                self.library.import_as(id, path, rate, ctx);
                self.pending.push((id, at));
            } else {
                rejected.push(
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                );
            }
        }
        if !rejected.is_empty() {
            self.notice = Some(format!(
                "Not added: {}. Lumen imports WAV, AIFF, FLAC, MP3 and Ogg Vorbis files",
                rejected.join(", ")
            ));
        }
    }

    /// Applies finished loads (adding them to the project), loads media the
    /// project needs, and keeps the engine's clips in step. Call every
    /// frame.
    pub fn update(
        &mut self,
        rate: u32,
        ctx: &egui::Context,
        doc: &mut Document,
        clips: &mut ClipsHandle,
    ) {
        self.dirty |= self.library.update(rate, ctx);

        // Loads that finished become tracks, in drop order, as one step.
        let mut imported = Vec::new();
        let library = &self.library;
        self.pending.retain(|&(id, at)| match library.get(id) {
            Some(item) => match (&item.state, item.info.frames, item.info.sample_rate) {
                (State::Ready(_), Some(frames), Some(sample_rate)) => {
                    imported.push(Imported {
                        media: Id(id),
                        path: item.path.clone(),
                        name: item.name.clone(),
                        sample_rate,
                        channels: item.info.channels.unwrap_or(2) as u16,
                        frames,
                        size_bytes: item.info.size_bytes,
                        at,
                    });
                    false
                }
                (State::Failed(_), ..) => false,
                _ => true,
            },
            // Cancelled.
            None => false,
        });
        doc.import(imported);

        // Media the project refers to but that is not loaded (an opened
        // project, slice 3c).
        for media in &doc.project().media {
            if self.library.get(media.id.0).is_none()
                && let Some(path) = doc.media_files.get(&media.id)
            {
                self.library.import_as(media.id.0, path.clone(), rate, ctx);
            }
        }

        if self.dirty || self.built != Some(doc.state()) {
            // A full queue means the engine has not caught up; retry next frame.
            let list = self.engine_clips(doc.project(), rate);
            self.dirty = clips.set(list).is_err();
            self.built = Some(doc.state());
        }
    }

    /// What the engine plays: every unmuted audio clip whose file is loaded,
    /// for the shorter of the clip and the file.
    fn engine_clips(&self, project: &Project, rate: u32) -> Vec<Clip> {
        let tempo = lumen_core::time::TempoMap::new(project.tempo.clone()).ok();
        let audible = crate::document::audible(project);
        let mut list = Vec::new();
        for track in project.tracks.iter().filter(|t| audible(t)) {
            for clip in track.clips.iter().filter(|c| !c.muted) {
                let ClipContent::Audio {
                    media,
                    offset,
                    gain_db,
                } = &clip.content
                else {
                    continue;
                };
                let Some(item) = self.library.get(media.0) else {
                    continue;
                };
                let State::Ready(audio) = &item.state else {
                    continue;
                };
                // The offset is in the file's frames; the audio is at the
                // device rate.
                let native = item.info.sample_rate.unwrap_or(audio.sample_rate).max(1);
                let offset =
                    (*offset as f64 * f64::from(audio.sample_rate) / f64::from(native)) as i64;
                let region = tempo.as_ref().map_or(i64::MAX, |t| {
                    t.sample_at(clip.start + clip.length, rate) - t.sample_at(clip.start, rate)
                });
                let length = (audio.frames() as i64 - offset).min(region);
                if length > 0 {
                    list.push(Clip {
                        start: clip.start,
                        offset,
                        length,
                        // Track volume and pan (M3 slice 4); the mixer
                        // takes over in M4.
                        gain: db_to_gain(*gain_db) * db_to_gain(track.gain_db),
                        pan: track.pan,
                        audio: audio.clone(),
                    });
                }
            }
        }
        list
    }

    /// The panel for loads in progress (with Cancel), failures (with
    /// Dismiss) and notices, in the arrangement's bottom-left corner.
    pub fn panel(&mut self, ctx: &egui::Context, corner: Pos2) {
        let loading: Vec<(String, f32)> = self
            .library
            .items()
            .iter()
            .filter_map(|i| Some((i.name.clone(), self.library.progress(i.id)?)))
            .collect();
        let failed: Vec<(u64, String, String)> = self
            .library
            .items()
            .iter()
            .filter_map(|i| match &i.state {
                State::Failed(why) => Some((i.id, i.name.clone(), why.clone())),
                _ => None,
            })
            .collect();
        if loading.is_empty() && failed.is_empty() && self.notice.is_none() {
            return;
        }
        egui::Area::new(egui::Id::new("imports"))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(corner)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(420.0);
                    if !loading.is_empty() {
                        ui.horizontal(|ui| {
                            let total: f32 = loading.iter().map(|(_, p)| p).sum();
                            ui.add(
                                egui::ProgressBar::new(total / loading.len() as f32)
                                    .desired_width(160.0)
                                    .show_percentage(),
                            );
                            ui.label(match loading.as_slice() {
                                [(name, _)] => format!("Loading {name}"),
                                many => format!("Loading {} files", many.len()),
                            });
                            if ui.button("Cancel").on_hover_text("Stop loading").clicked() {
                                self.library.cancel_all();
                            }
                        });
                    }
                    for (id, name, why) in failed {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⚠").color(theme::p().warning));
                            ui.label(format!("{name}: {why}"));
                            if ui.small_button("Dismiss").clicked() {
                                self.library.dismiss(id);
                                self.dirty = true;
                            }
                        });
                    }
                    if let Some(notice) = self.notice.clone() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("ⓘ").color(theme::p().text_secondary));
                            ui.label(notice);
                            if ui.small_button("Dismiss").clicked() {
                                self.notice = None;
                            }
                        });
                    }
                });
            });
    }
}

/// The hover card for a clip's file.
pub fn file_card(ui: &mut egui::Ui, item: &Item) {
    ui.label(RichText::new(&item.name).strong());
    let info = &item.info;
    let mut facts = vec![info.format.clone()];
    if let Some(rate) = info.sample_rate {
        facts.push(format!("{:.1} kHz", f64::from(rate) / 1000.0));
    }
    if let Some(channels) = info.channels {
        facts.push(match channels {
            1 => "mono".into(),
            2 => "stereo".into(),
            n => format!("{n} channels"),
        });
    }
    if let Some(seconds) = info.seconds {
        facts.push(crate::position::format_time(seconds));
    }
    facts.push(format!("{:.1} MB", info.size_bytes as f64 / 1_048_576.0));
    ui.label(facts.join(" · "));
    match &item.state {
        State::Ready(audio) if Some(audio.sample_rate) != info.sample_rate => {
            ui.label(format!(
                "Playing at {:.1} kHz (converted)",
                f64::from(audio.sample_rate) / 1000.0
            ));
        }
        State::Failed(why) => {
            ui.label(RichText::new(why).color(theme::p().warning));
        }
        _ => {}
    }
    ui.label(
        RichText::new(item.path.display().to_string())
            .small()
            .weak(),
    );
}
